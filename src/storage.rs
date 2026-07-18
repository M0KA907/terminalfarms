use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use rusqlite::{Connection, OptionalExtension, params};
use thiserror::Error;

use crate::{CropCatalog, GameState, TileState, UpgradeCatalog, UpgradeState, game::GameError};

const LATEST_SCHEMA: i32 = 2;

const MIGRATION_1: &str = r#"
CREATE TABLE player (
    id                INTEGER PRIMARY KEY CHECK (id = 1),
    coins             INTEGER NOT NULL CHECK (coins >= 0),
    run_earnings      INTEGER NOT NULL CHECK (run_earnings >= 0),
    rebirth_tokens    INTEGER NOT NULL CHECK (rebirth_tokens >= 0),
    rebirth_count     INTEGER NOT NULL CHECK (rebirth_count >= 0),
    farm_rows         INTEGER NOT NULL CHECK (farm_rows >= 1),
    farm_cols         INTEGER NOT NULL CHECK (farm_cols >= 1),
    selected_crop     INTEGER NOT NULL CHECK (selected_crop >= 0),
    last_seen_utc     INTEGER NOT NULL
);

CREATE TABLE farm_tiles (
    row_index         INTEGER NOT NULL CHECK (row_index >= 0),
    col_index         INTEGER NOT NULL CHECK (col_index >= 0),
    tile_state        TEXT NOT NULL CHECK (tile_state IN ('untilled', 'tilled', 'planted')),
    crop_id           TEXT,
    growth_seconds    REAL NOT NULL DEFAULT 0 CHECK (growth_seconds >= 0),
    PRIMARY KEY (row_index, col_index),
    CHECK ((tile_state = 'planted' AND crop_id IS NOT NULL) OR
           (tile_state != 'planted' AND crop_id IS NULL))
);

CREATE TABLE inventory (
    item_kind         TEXT NOT NULL CHECK (item_kind IN ('seed', 'produce')),
    item_id           TEXT NOT NULL,
    quantity          INTEGER NOT NULL CHECK (quantity >= 0),
    PRIMARY KEY (item_kind, item_id)
);

CREATE TABLE purchased_upgrades (
    upgrade_id        TEXT PRIMARY KEY,
    level             INTEGER NOT NULL CHECK (level >= 0)
);

CREATE TABLE settings (
    setting_key       TEXT PRIMARY KEY,
    setting_value     TEXT NOT NULL
);

CREATE TABLE schema_migrations (
    version           INTEGER PRIMARY KEY,
    applied_at_utc    INTEGER NOT NULL
);
"#;

const MIGRATION_2: &str = r#"
ALTER TABLE purchased_upgrades
ADD COLUMN elapsed_seconds REAL NOT NULL DEFAULT 0 CHECK (elapsed_seconds >= 0);
"#;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("database operation failed: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("database backup failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("database schema {found} is newer than this executable supports ({supported})")]
    NewerSchema { found: i32, supported: i32 },
    #[error("save contains an invalid tile state `{0}`")]
    InvalidTileState(String),
    #[error("save value `{0}` exceeds SQLite's integer range")]
    NumericOverflow(&'static str),
    #[error("save data is invalid: {0}")]
    InvalidGame(#[from] GameError),
}

pub struct Database {
    connection: Connection,
    path: PathBuf,
}

impl Database {
    pub fn open(path: impl AsRef<Path>, now_utc: i64) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let existed = path.exists();
        let mut connection = Connection::open(&path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        let current: i32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if current > LATEST_SCHEMA {
            return Err(StorageError::NewerSchema {
                found: current,
                supported: LATEST_SCHEMA,
            });
        }

        if existed && current > 0 && current < LATEST_SCHEMA {
            let _ = connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
            fs::copy(&path, backup_path(&path, now_utc))?;
        }

        migrate(&mut connection, current, now_utc)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;

        Ok(Self { connection, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn load_or_create(
        &mut self,
        catalog: &CropCatalog,
        upgrade_catalog: &UpgradeCatalog,
        now_utc: i64,
    ) -> Result<GameState, StorageError> {
        let player = self
            .connection
            .query_row(
                "SELECT coins, run_earnings, rebirth_tokens, rebirth_count,
                        farm_rows, farm_cols, selected_crop, last_seen_utc
                 FROM player WHERE id = 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, u32>(2)?,
                        row.get::<_, u32>(3)?,
                        row.get::<_, u32>(4)?,
                        row.get::<_, u32>(5)?,
                        row.get::<_, u32>(6)?,
                        row.get::<_, i64>(7)?,
                    ))
                },
            )
            .optional()?;

        let Some((
            saved_coins,
            saved_run_earnings,
            rebirth_tokens,
            rebirth_count,
            rows,
            cols,
            saved_selected_crop,
            last_seen_utc,
        )) = player
        else {
            let state = GameState::new(catalog, now_utc);
            self.save(&state, now_utc)?;
            return Ok(state);
        };
        let coins =
            u64::try_from(saved_coins).map_err(|_| StorageError::NumericOverflow("coins"))?;
        let run_earnings = u64::try_from(saved_run_earnings)
            .map_err(|_| StorageError::NumericOverflow("run_earnings"))?;
        let selected_crop = saved_selected_crop as usize;

        let mut tiles = vec![TileState::Untilled; (rows * cols) as usize];
        {
            let mut statement = self.connection.prepare(
                "SELECT row_index, col_index, tile_state, crop_id, growth_seconds
                 FROM farm_tiles ORDER BY row_index, col_index",
            )?;
            let saved_tiles = statement.query_map([], |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, f64>(4)?,
                ))
            })?;
            for saved in saved_tiles {
                let (row, col, state, crop_id, progress) = saved?;
                if row >= rows || col >= cols {
                    return Err(StorageError::InvalidGame(GameError::InvalidDimensions));
                }
                let state = match (state.as_str(), crop_id) {
                    ("untilled", None) => TileState::Untilled,
                    ("tilled", None) => TileState::Tilled,
                    ("planted", Some(crop_id)) => TileState::Planted { crop_id, progress },
                    _ => return Err(StorageError::InvalidTileState(state)),
                };
                tiles[(row * cols + col) as usize] = state;
            }
        }

        let mut seeds = BTreeMap::new();
        let mut produce = BTreeMap::new();
        {
            let mut statement = self
                .connection
                .prepare("SELECT item_kind, item_id, quantity FROM inventory")?;
            let inventory = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u32>(2)?,
                ))
            })?;
            for item in inventory {
                let (kind, id, quantity) = item?;
                match kind.as_str() {
                    "seed" => {
                        seeds.insert(id, quantity);
                    }
                    "produce" => {
                        produce.insert(id, quantity);
                    }
                    _ => return Err(StorageError::InvalidTileState(kind)),
                }
            }
        }

        let mut upgrades = BTreeMap::new();
        {
            let mut statement = self
                .connection
                .prepare("SELECT upgrade_id, level, elapsed_seconds FROM purchased_upgrades")?;
            let saved_upgrades = statement.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, u32>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })?;
            for upgrade in saved_upgrades {
                let (id, level, elapsed_seconds) = upgrade?;
                upgrades.insert(
                    id,
                    UpgradeState {
                        level,
                        elapsed_seconds,
                    },
                );
            }
        }

        let state = GameState {
            coins,
            run_earnings,
            rebirth_tokens,
            rebirth_count,
            rows,
            cols,
            selected_crop,
            tiles,
            seeds,
            produce,
            upgrades,
            last_seen_utc,
        };
        state.validate(catalog, upgrade_catalog)?;
        Ok(state)
    }

    pub fn save(&mut self, state: &GameState, now_utc: i64) -> Result<(), StorageError> {
        let coins =
            i64::try_from(state.coins).map_err(|_| StorageError::NumericOverflow("coins"))?;
        let run_earnings = i64::try_from(state.run_earnings)
            .map_err(|_| StorageError::NumericOverflow("run_earnings"))?;
        let selected_crop = i64::try_from(state.selected_crop)
            .map_err(|_| StorageError::NumericOverflow("selected_crop"))?;
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO player (
                id, coins, run_earnings, rebirth_tokens, rebirth_count,
                farm_rows, farm_cols, selected_crop, last_seen_utc
             ) VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                coins = excluded.coins,
                run_earnings = excluded.run_earnings,
                rebirth_tokens = excluded.rebirth_tokens,
                rebirth_count = excluded.rebirth_count,
                farm_rows = excluded.farm_rows,
                farm_cols = excluded.farm_cols,
                selected_crop = excluded.selected_crop,
                last_seen_utc = excluded.last_seen_utc",
            params![
                coins,
                run_earnings,
                state.rebirth_tokens,
                state.rebirth_count,
                state.rows,
                state.cols,
                selected_crop,
                now_utc,
            ],
        )?;

        transaction.execute("DELETE FROM farm_tiles", [])?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO farm_tiles
                    (row_index, col_index, tile_state, crop_id, growth_seconds)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for row in 0..state.rows {
                for col in 0..state.cols {
                    let tile = &state.tiles[(row * state.cols + col) as usize];
                    let (tile_state, crop_id, progress) = match tile {
                        TileState::Untilled => ("untilled", None, 0.0),
                        TileState::Tilled => ("tilled", None, 0.0),
                        TileState::Planted { crop_id, progress } => {
                            ("planted", Some(crop_id.as_str()), *progress)
                        }
                    };
                    insert.execute(params![row, col, tile_state, crop_id, progress])?;
                }
            }
        }

        transaction.execute("DELETE FROM inventory", [])?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO inventory (item_kind, item_id, quantity) VALUES (?1, ?2, ?3)",
            )?;
            for (id, quantity) in &state.seeds {
                insert.execute(params!["seed", id, quantity])?;
            }
            for (id, quantity) in &state.produce {
                insert.execute(params!["produce", id, quantity])?;
            }
        }

        transaction.execute("DELETE FROM purchased_upgrades", [])?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO purchased_upgrades (upgrade_id, level, elapsed_seconds)
                 VALUES (?1, ?2, ?3)",
            )?;
            for (id, upgrade) in &state.upgrades {
                insert.execute(params![id, upgrade.level, upgrade.elapsed_seconds])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }
}

fn migrate(connection: &mut Connection, current: i32, now_utc: i64) -> Result<(), StorageError> {
    if current < 1 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_1)?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, applied_at_utc) VALUES (1, ?1)",
            [now_utc],
        )?;
        transaction.pragma_update(None, "user_version", 1)?;
        transaction.commit()?;
    }
    if current < 2 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_2)?;
        transaction.execute(
            "INSERT INTO schema_migrations (version, applied_at_utc) VALUES (2, ?1)",
            [now_utc],
        )?;
        transaction.pragma_update(None, "user_version", 2)?;
        transaction.commit()?;
    }
    Ok(())
}

fn backup_path(database: &Path, now_utc: i64) -> PathBuf {
    let stem = database
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("terminalfarms");
    database.with_file_name(format!("{stem}.backup-{now_utc}.db"))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let id = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("terminalfarms-test-{}-{id}", std::process::id()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn state_round_trips_through_sqlite() {
        let temp = TempDir::new();
        let path = temp.0.join("farm.db");
        let catalog = CropCatalog::embedded().unwrap();
        let upgrades = UpgradeCatalog::embedded().unwrap();
        {
            let mut database = Database::open(&path, 100).unwrap();
            let mut state = database.load_or_create(&catalog, &upgrades, 100).unwrap();
            state.coins = 777;
            state.run_earnings = 1_000;
            state.buy_upgrade(0, &upgrades);
            state.use_tile(0, 0, &catalog);
            state.use_tile(0, 0, &catalog);
            state.apply_elapsed(12.0, 1.0);
            database.save(&state, 112).unwrap();
        }

        let mut database = Database::open(&path, 200).unwrap();
        let loaded = database.load_or_create(&catalog, &upgrades, 200).unwrap();
        assert_eq!(loaded.coins, 677);
        assert_eq!(loaded.upgrade_level("cultivator"), 1);
        assert_eq!(loaded.last_seen_utc, 112);
        assert!(matches!(
            loaded.tile(0, 0),
            Some(TileState::Planted { progress, .. }) if (*progress - 12.0).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn migration_creates_backup_before_schema_change() {
        let temp = TempDir::new();
        let path = temp.0.join("farm.db");
        {
            let connection = Connection::open(&path).unwrap();
            connection.execute_batch(MIGRATION_1).unwrap();
            connection.pragma_update(None, "user_version", 1).unwrap();
        }

        Database::open(&path, 321).unwrap();
        assert!(backup_path(&path, 321).exists());
        let connection = Connection::open(path).unwrap();
        let version: i32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 2);
    }
}
