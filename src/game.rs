use std::collections::BTreeMap;

use thiserror::Error;

use crate::{CropCatalog, CropDefinition, UpgradeCatalog, UpgradeDefinition, UpgradeKind};

pub const STARTING_COINS: u64 = 20;
pub const STARTING_SIZE: u32 = 3;
pub const ACTIVE_GROWTH_MULTIPLIER: f64 = 1.25;

#[derive(Debug, Clone, PartialEq)]
pub enum TileState {
    Untilled,
    Tilled,
    Planted { crop_id: String, progress: f64 },
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UpgradeState {
    pub level: u32,
    pub elapsed_seconds: f64,
}

#[derive(Debug, Clone)]
pub struct GameState {
    pub coins: u64,
    pub run_earnings: u64,
    pub rebirth_tokens: u32,
    pub rebirth_count: u32,
    pub rows: u32,
    pub cols: u32,
    pub selected_crop: usize,
    pub tiles: Vec<TileState>,
    pub seeds: BTreeMap<String, u32>,
    pub produce: BTreeMap<String, u32>,
    pub upgrades: BTreeMap<String, UpgradeState>,
    pub last_seen_utc: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionResult {
    Changed(String),
    Unchanged(String),
}

impl ActionResult {
    pub fn changed(&self) -> bool {
        matches!(self, Self::Changed(_))
    }

    pub fn message(self) -> String {
        match self {
            Self::Changed(message) | Self::Unchanged(message) => message,
        }
    }
}

#[derive(Debug, Error)]
pub enum GameError {
    #[error("farm dimensions do not match its tiles")]
    InvalidDimensions,
    #[error("save references unknown crop `{0}`")]
    UnknownCrop(String),
    #[error("selected crop index is invalid")]
    InvalidSelection,
    #[error("save references unknown upgrade `{0}`")]
    UnknownUpgrade(String),
}

impl GameState {
    pub fn new(catalog: &CropCatalog, now_utc: i64) -> Self {
        let first_crop = &catalog.crops[0];
        let mut seeds = BTreeMap::new();
        seeds.insert(first_crop.id.clone(), 3);

        Self {
            coins: STARTING_COINS,
            run_earnings: 0,
            rebirth_tokens: 0,
            rebirth_count: 0,
            rows: STARTING_SIZE,
            cols: STARTING_SIZE,
            selected_crop: 0,
            tiles: vec![TileState::Untilled; (STARTING_SIZE * STARTING_SIZE) as usize],
            seeds,
            produce: BTreeMap::new(),
            upgrades: BTreeMap::new(),
            last_seen_utc: now_utc,
        }
    }

    pub fn validate(
        &self,
        catalog: &CropCatalog,
        upgrade_catalog: &UpgradeCatalog,
    ) -> Result<(), GameError> {
        if self.tiles.len() != (self.rows * self.cols) as usize {
            return Err(GameError::InvalidDimensions);
        }
        if self.selected_crop >= catalog.crops.len() {
            return Err(GameError::InvalidSelection);
        }
        for state in &self.tiles {
            if let TileState::Planted { crop_id, .. } = state
                && catalog.get(crop_id).is_none()
            {
                return Err(GameError::UnknownCrop(crop_id.clone()));
            }
        }
        for id in self.upgrades.keys() {
            if upgrade_catalog.get(id).is_none() {
                return Err(GameError::UnknownUpgrade(id.clone()));
            }
        }
        Ok(())
    }

    pub fn tile(&self, row: u32, col: u32) -> Option<&TileState> {
        self.index(row, col).and_then(|index| self.tiles.get(index))
    }

    pub fn selected_definition<'a>(&self, catalog: &'a CropCatalog) -> &'a CropDefinition {
        &catalog.crops[self.selected_crop.min(catalog.crops.len() - 1)]
    }

    pub fn select_previous_crop(&mut self, catalog: &CropCatalog) {
        self.selected_crop = if self.selected_crop == 0 {
            catalog.crops.len() - 1
        } else {
            self.selected_crop - 1
        };
    }

    pub fn select_next_crop(&mut self, catalog: &CropCatalog) {
        self.selected_crop = (self.selected_crop + 1) % catalog.crops.len();
    }

    pub fn apply_elapsed(&mut self, seconds: f64, multiplier: f64) {
        if seconds <= 0.0 {
            return;
        }
        let growth = seconds * multiplier;
        for tile in &mut self.tiles {
            if let TileState::Planted { progress, .. } = tile {
                *progress += growth;
            }
        }
    }

    pub fn use_tile(&mut self, row: u32, col: u32, catalog: &CropCatalog) -> ActionResult {
        let Some(index) = self.index(row, col) else {
            return ActionResult::Unchanged("Outside farm".into());
        };

        match self.tiles[index].clone() {
            TileState::Untilled => {
                self.tiles[index] = TileState::Tilled;
                ActionResult::Changed("Tilled soil".into())
            }
            TileState::Tilled => {
                let crop = self.selected_definition(catalog);
                if self.run_earnings < crop.unlock_earnings {
                    return ActionResult::Unchanged(format!(
                        "{} unlocks at ${}",
                        crop.name, crop.unlock_earnings
                    ));
                }
                let seeds = self.seeds.entry(crop.id.clone()).or_default();
                if *seeds == 0 {
                    return ActionResult::Unchanged(format!("No {} seeds", crop.name));
                }
                *seeds -= 1;
                self.tiles[index] = TileState::Planted {
                    crop_id: crop.id.clone(),
                    progress: 0.0,
                };
                ActionResult::Changed(format!("Sowed {}", crop.name))
            }
            TileState::Planted { crop_id, progress } => {
                let Some(crop) = catalog.get(&crop_id) else {
                    return ActionResult::Unchanged("Unknown crop".into());
                };
                if progress < crop.grow_seconds as f64 {
                    let remaining = (crop.grow_seconds as f64 - progress).ceil() as u64;
                    return ActionResult::Unchanged(format!(
                        "{}: {}s remaining",
                        crop.name, remaining
                    ));
                }
                *self.produce.entry(crop_id).or_default() += 1;
                self.tiles[index] = TileState::Tilled;
                ActionResult::Changed(format!("Harvested {}", crop.name))
            }
        }
    }

    pub fn buy_selected_seed(&mut self, catalog: &CropCatalog) -> ActionResult {
        let crop = self.selected_definition(catalog);
        if self.run_earnings < crop.unlock_earnings {
            return ActionResult::Unchanged(format!(
                "{} unlocks at ${}",
                crop.name, crop.unlock_earnings
            ));
        }
        if self.coins < crop.seed_price {
            return ActionResult::Unchanged("Not enough money".into());
        }
        self.coins -= crop.seed_price;
        *self.seeds.entry(crop.id.clone()).or_default() += 1;
        ActionResult::Changed(format!("Bought {} seed", crop.name))
    }

    pub fn upgrade_level(&self, id: &str) -> u32 {
        self.upgrades.get(id).map_or(0, |upgrade| upgrade.level)
    }

    pub fn upgrade_cost(&self, upgrade: &UpgradeDefinition) -> u64 {
        upgrade
            .base_price
            .saturating_mul(2_u64.saturating_pow(self.upgrade_level(&upgrade.id)))
    }

    pub fn buy_upgrade(&mut self, index: usize, catalog: &UpgradeCatalog) -> ActionResult {
        let Some(upgrade) = catalog.upgrades.get(index) else {
            return ActionResult::Unchanged("Unknown shop option".into());
        };
        let level = self.upgrade_level(&upgrade.id);
        if self.run_earnings < upgrade.unlock_earnings {
            return ActionResult::Unchanged(format!(
                "{} unlocks at ${}",
                upgrade.name, upgrade.unlock_earnings
            ));
        }
        if level >= upgrade.max_level {
            return ActionResult::Unchanged(format!("{} is max level", upgrade.name));
        }
        let cost = self.upgrade_cost(upgrade);
        if self.coins < cost {
            return ActionResult::Unchanged(format!("{} costs ${cost}", upgrade.name));
        }
        self.coins -= cost;
        self.upgrades.entry(upgrade.id.clone()).or_default().level += 1;
        ActionResult::Changed(format!("{} upgraded to level {}", upgrade.name, level + 1))
    }

    pub fn growth_upgrade_multiplier(&self, catalog: &UpgradeCatalog) -> f64 {
        let levels: u32 = catalog
            .upgrades
            .iter()
            .filter(|upgrade| upgrade.kind == UpgradeKind::GrowthBoost)
            .map(|upgrade| self.upgrade_level(&upgrade.id))
            .sum();
        1.0 + f64::from(levels) * 0.15
    }

    pub fn run_automation(
        &mut self,
        seconds: f64,
        crops: &CropCatalog,
        upgrades: &UpgradeCatalog,
    ) -> Vec<String> {
        let mut messages = Vec::new();
        for upgrade in &upgrades.upgrades {
            let level = self.upgrade_level(&upgrade.id);
            if level == 0 || upgrade.kind == UpgradeKind::GrowthBoost {
                continue;
            }
            let interval = upgrade.interval_seconds as f64 / f64::from(level);
            let cycles = {
                let state = self.upgrades.entry(upgrade.id.clone()).or_default();
                state.elapsed_seconds += seconds;
                let cycles = (state.elapsed_seconds / interval).floor() as usize;
                state.elapsed_seconds -= cycles as f64 * interval;
                cycles.min(1_000)
            };
            for _ in 0..cycles {
                let Some(message) = self.run_automation_action(upgrade.kind, crops) else {
                    break;
                };
                messages.push(message);
            }
        }
        messages
    }

    fn run_automation_action(
        &mut self,
        kind: UpgradeKind,
        catalog: &CropCatalog,
    ) -> Option<String> {
        match kind {
            UpgradeKind::AutoTill => {
                let tile = self
                    .tiles
                    .iter_mut()
                    .find(|tile| matches!(tile, TileState::Untilled))?;
                *tile = TileState::Tilled;
                Some("Cultivator tilled soil".into())
            }
            UpgradeKind::AutoSow => {
                let crop = self.selected_definition(catalog);
                if self.run_earnings < crop.unlock_earnings
                    || self.seeds.get(&crop.id).copied().unwrap_or(0) == 0
                {
                    return None;
                }
                let tile = self
                    .tiles
                    .iter_mut()
                    .find(|tile| matches!(tile, TileState::Tilled))?;
                *self.seeds.get_mut(&crop.id).expect("seed count checked") -= 1;
                *tile = TileState::Planted {
                    crop_id: crop.id.clone(),
                    progress: 0.0,
                };
                Some(format!("Seed drill sowed {}", crop.name))
            }
            UpgradeKind::AutoHarvest => {
                let index = self.tiles.iter().position(|tile| {
                    let TileState::Planted { crop_id, progress } = tile else {
                        return false;
                    };
                    catalog
                        .get(crop_id)
                        .is_some_and(|crop| *progress >= crop.grow_seconds as f64)
                })?;
                let TileState::Planted { crop_id, .. } = self.tiles[index].clone() else {
                    unreachable!();
                };
                let crop = catalog.get(&crop_id).expect("validated crop id");
                *self.produce.entry(crop_id).or_default() += 1;
                self.tiles[index] = TileState::Tilled;
                Some(format!("Harvester collected {}", crop.name))
            }
            UpgradeKind::AutoSell => match self.sell_all(catalog) {
                ActionResult::Changed(message) => Some(format!("Market stall: {message}")),
                ActionResult::Unchanged(_) => None,
            },
            UpgradeKind::GrowthBoost => None,
        }
    }

    pub fn sell_all(&mut self, catalog: &CropCatalog) -> ActionResult {
        let multiplier = 1.0 + f64::from(self.rebirth_tokens) * 0.1;
        let mut total = 0_u64;
        for crop in &catalog.crops {
            let quantity = self.produce.remove(&crop.id).unwrap_or(0);
            let base = crop.sell_price.saturating_mul(u64::from(quantity));
            total = total.saturating_add((base as f64 * multiplier).round() as u64);
        }
        if total == 0 {
            return ActionResult::Unchanged("No produce to sell".into());
        }
        self.coins = self.coins.saturating_add(total);
        self.run_earnings = self.run_earnings.saturating_add(total);
        ActionResult::Changed(format!("Sold produce for ${total}"))
    }

    pub fn row_cost(&self) -> u64 {
        25 * u64::from(self.rows - 1).pow(2)
    }

    pub fn column_cost(&self) -> u64 {
        25 * u64::from(self.cols - 1).pow(2)
    }

    pub fn buy_row(&mut self) -> ActionResult {
        let cost = self.row_cost();
        if self.coins < cost {
            return ActionResult::Unchanged(format!("Row costs ${cost}"));
        }
        self.coins -= cost;
        self.tiles
            .extend(std::iter::repeat_n(TileState::Untilled, self.cols as usize));
        self.rows += 1;
        ActionResult::Changed(format!("Bought row for ${cost}"))
    }

    pub fn buy_column(&mut self) -> ActionResult {
        let cost = self.column_cost();
        if self.coins < cost {
            return ActionResult::Unchanged(format!("Column costs ${cost}"));
        }
        self.coins -= cost;
        let mut expanded = Vec::with_capacity((self.rows * (self.cols + 1)) as usize);
        for row in self.tiles.chunks(self.cols as usize) {
            expanded.extend_from_slice(row);
            expanded.push(TileState::Untilled);
        }
        self.tiles = expanded;
        self.cols += 1;
        ActionResult::Changed(format!("Bought column for ${cost}"))
    }

    pub fn rebirth_requirement(&self) -> u64 {
        2_500_u64.saturating_mul(2_u64.saturating_pow(self.rebirth_count))
    }

    pub fn rebirth(&mut self, catalog: &CropCatalog) -> ActionResult {
        let requirement = self.rebirth_requirement();
        if self.run_earnings < requirement {
            return ActionResult::Unchanged(format!("Rebirth requires ${requirement} earned"));
        }
        let now = self.last_seen_utc;
        let count = self.rebirth_count + 1;
        let tokens = self.rebirth_tokens + 1;
        *self = Self::new(catalog, now);
        self.rebirth_count = count;
        self.rebirth_tokens = tokens;
        ActionResult::Changed(format!("Rebirth {count}: +1 permanent token"))
    }

    pub fn growth_stage(progress: f64, grow_seconds: u64) -> usize {
        let ratio = progress / grow_seconds as f64;
        if ratio >= 1.0 {
            3
        } else if ratio >= 0.45 {
            2
        } else if ratio >= 0.12 {
            1
        } else {
            0
        }
    }

    fn index(&self, row: u32, col: u32) -> Option<usize> {
        (row < self.rows && col < self.cols).then_some((row * self.cols + col) as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> CropCatalog {
        CropCatalog::embedded().unwrap()
    }

    #[test]
    fn active_time_advances_crops_faster() {
        let catalog = catalog();
        let mut game = GameState::new(&catalog, 0);
        game.use_tile(0, 0, &catalog);
        game.use_tile(0, 0, &catalog);
        game.apply_elapsed(10.0, ACTIVE_GROWTH_MULTIPLIER);
        assert!(matches!(
            game.tile(0, 0),
            Some(TileState::Planted { progress, .. }) if (*progress - 12.5).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn expansion_preserves_existing_tiles() {
        let catalog = catalog();
        let mut game = GameState::new(&catalog, 0);
        game.coins = 10_000;
        game.use_tile(1, 2, &catalog);
        assert!(game.buy_column().changed());
        assert_eq!(game.cols, 4);
        assert_eq!(game.tile(1, 2), Some(&TileState::Tilled));
        assert!(game.buy_row().changed());
        assert_eq!(game.rows, 4);
        game.validate(&catalog, &UpgradeCatalog::embedded().unwrap())
            .unwrap();
    }

    #[test]
    fn rebirth_resets_run_and_preserves_tokens() {
        let catalog = catalog();
        let mut game = GameState::new(&catalog, 0);
        game.run_earnings = game.rebirth_requirement();
        game.coins = 99_999;
        assert!(game.rebirth(&catalog).changed());
        assert_eq!(game.coins, STARTING_COINS);
        assert_eq!(game.rebirth_tokens, 1);
        assert_eq!(game.rows, STARTING_SIZE);
    }

    #[test]
    fn machinery_automates_farm_actions() {
        let crops = catalog();
        let upgrades = UpgradeCatalog::embedded().unwrap();
        let mut game = GameState::new(&crops, 0);
        game.coins = 10_000;
        game.run_earnings = 10_000;
        assert!(game.buy_upgrade(0, &upgrades).changed());
        let messages = game.run_automation(20.0, &crops, &upgrades);
        assert_eq!(messages.len(), 1);
        assert_eq!(game.tile(0, 0), Some(&TileState::Tilled));
    }
}
