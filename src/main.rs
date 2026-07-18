mod ui;

use std::{
    fs::{self, File, OpenOptions},
    io::{self, Stdout, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow};
use clap::Parser;
use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use directories::ProjectDirs;
use ratatui::{Terminal, backend::CrosstermBackend};
use terminalfarms::{
    ActionResult, CropCatalog, Database, GameState, UpgradeCatalog, game::ACTIVE_GROWTH_MULTIPLIER,
};

use crate::ui::{ShopTarget, View, main_menu_target, shop_target, viewport_capacity};

const INPUT_POLL: Duration = Duration::from_millis(250);
const GAME_TICK: Duration = Duration::from_secs(1);
const AUTOSAVE: Duration = Duration::from_secs(30);
const MAIN_MENU_ITEMS: usize = 3;

#[derive(Debug, Parser)]
#[command(name = "terminalfarms", version, about = "Terminal farming deskpet")]
struct Args {
    /// Store the database beside the executable.
    #[arg(long, conflicts_with = "data_dir")]
    portable: bool,

    /// Store application data in this directory.
    #[arg(long, value_name = "DIR", conflicts_with = "portable")]
    data_dir: Option<PathBuf>,

    /// Force the simplified terminal interface.
    #[arg(long)]
    compat: bool,

    /// Disable terminal colors.
    #[arg(long)]
    no_color: bool,

    /// Write diagnostics to terminalfarms.log in the data directory.
    #[arg(long)]
    log: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppScreen {
    MainMenu,
    HowToPlay,
    Farm,
}

struct App {
    game: GameState,
    catalog: CropCatalog,
    upgrade_catalog: UpgradeCatalog,
    cursor_row: u32,
    cursor_col: u32,
    offset_row: u32,
    offset_col: u32,
    status: String,
    hovered_shop: Option<ShopTarget>,
    reset_armed: bool,
    screen: AppScreen,
    menu_selection: usize,
    force_compatibility: bool,
    no_color: bool,
    quit: bool,
    redraw: bool,
}

impl App {
    fn compatibility(&self, width: u16, height: u16) -> bool {
        self.force_compatibility || width < 80 || height < 24
    }

    fn keep_cursor_visible(&mut self, width: u16, height: u16) {
        let compatibility = self.compatibility(width, height);
        let (visible_rows, visible_cols) = viewport_capacity(width, height, compatibility);
        if self.cursor_row < self.offset_row {
            self.offset_row = self.cursor_row;
        } else if self.cursor_row >= self.offset_row + visible_rows {
            self.offset_row = self.cursor_row + 1 - visible_rows;
        }
        if self.cursor_col < self.offset_col {
            self.offset_col = self.cursor_col;
        } else if self.cursor_col >= self.offset_col + visible_cols {
            self.offset_col = self.cursor_col + 1 - visible_cols;
        }
        self.clamp_offsets(visible_rows, visible_cols);
    }

    fn clamp_offsets(&mut self, visible_rows: u32, visible_cols: u32) {
        self.offset_row = self
            .offset_row
            .min(self.game.rows.saturating_sub(visible_rows));
        self.offset_col = self
            .offset_col
            .min(self.game.cols.saturating_sub(visible_cols));
    }

    fn move_cursor(&mut self, row_delta: i32, col_delta: i32) {
        self.cursor_row = move_axis(self.cursor_row, row_delta, self.game.rows);
        self.cursor_col = move_axis(self.cursor_col, col_delta, self.game.cols);
        self.redraw = true;
    }

    fn apply_result(&mut self, result: ActionResult) -> bool {
        let changed = result.changed();
        self.status = result.message();
        self.redraw = true;
        changed
    }

    fn cancel_reset(&mut self) {
        if self.reset_armed {
            self.reset_armed = false;
            self.status = "Reset cancelled".into();
            self.redraw = true;
        }
    }

    fn request_reset(&mut self) -> bool {
        if !self.reset_armed {
            self.reset_armed = true;
            self.status = "WARNING: reset deletes all progress. Press x again to confirm.".into();
            self.redraw = true;
            return false;
        }

        self.game = GameState::new(&self.catalog, unix_time());
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.offset_row = 0;
        self.offset_col = 0;
        self.reset_armed = false;
        self.status = "Progress deleted. New farm started.".into();
        self.redraw = true;
        true
    }

    fn select_previous_menu_item(&mut self) {
        self.menu_selection = if self.menu_selection == 0 {
            MAIN_MENU_ITEMS - 1
        } else {
            self.menu_selection - 1
        };
        self.redraw = true;
    }

    fn select_next_menu_item(&mut self) {
        self.menu_selection = (self.menu_selection + 1) % MAIN_MENU_ITEMS;
        self.redraw = true;
    }

    fn activate_menu_item(&mut self) {
        match self.menu_selection {
            0 => self.screen = AppScreen::Farm,
            1 => self.screen = AppScreen::HowToPlay,
            2 => self.quit = true,
            _ => unreachable!("menu selection is clamped"),
        }
        self.redraw = true;
    }

    fn open_main_menu(&mut self) {
        self.cancel_reset();
        self.screen = AppScreen::MainMenu;
        self.redraw = true;
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();
    let data_dir = resolve_data_dir(&args)?;
    fs::create_dir_all(&data_dir)
        .with_context(|| format!("could not create {}", data_dir.display()))?;
    let mut logger = Logger::new(args.log.then(|| data_dir.join("terminalfarms.log")))?;
    logger.write("starting");

    let catalog = CropCatalog::embedded().context("could not load embedded game data")?;
    let upgrade_catalog =
        UpgradeCatalog::embedded().context("could not load embedded upgrade data")?;
    let database_path = data_dir.join("terminalfarms.db");
    let now = unix_time();
    let mut database = Database::open(&database_path, now)
        .with_context(|| format!("could not open {}", database_path.display()))?;
    let mut game = database
        .load_or_create(&catalog, &upgrade_catalog, now)
        .context("could not load farm")?;
    let offline_seconds = now.saturating_sub(game.last_seen_utc).max(0);
    let machinery_actions = apply_offline_progress(
        &mut game,
        &catalog,
        &upgrade_catalog,
        offline_seconds as f64,
    );
    game.last_seen_utc = now;
    database.save(&game, now)?;

    let force_compatibility = args.compat || !unicode_terminal();
    let no_color = args.no_color
        || std::env::var_os("NO_COLOR").is_some()
        || std::env::var("TERM").is_ok_and(|term| term == "dumb");
    let status = if offline_seconds > 0 {
        let action_suffix = if machinery_actions == 1 { "" } else { "s" };
        format!(
            "Offline: {} · {machinery_actions} machine action{action_suffix}",
            duration_text(offline_seconds as u64)
        )
    } else {
        "Ready".to_owned()
    };
    let mut app = App {
        game,
        catalog,
        upgrade_catalog,
        cursor_row: 0,
        cursor_col: 0,
        offset_row: 0,
        offset_col: 0,
        status,
        hovered_shop: None,
        reset_armed: false,
        screen: AppScreen::MainMenu,
        menu_selection: 0,
        force_compatibility,
        no_color,
        quit: false,
        redraw: true,
    };

    let result = run_terminal(&mut app, &mut database, &mut logger);
    let save_time = unix_time();
    app.game.last_seen_utc = save_time;
    database
        .save(&app.game, save_time)
        .context("could not save farm while exiting")?;
    logger.write("stopped");
    result
}

fn run_terminal(app: &mut App, database: &mut Database, logger: &mut Logger) -> Result<()> {
    enable_raw_mode().context("could not enable terminal raw mode")?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen, EnableMouseCapture) {
        let _ = disable_raw_mode();
        return Err(error).context("could not enter terminal screen");
    }
    let restore = RestoreTerminal;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("could not initialize terminal")?;
    let result = event_loop(&mut terminal, app, database, logger);
    drop(terminal);
    drop(restore);
    result
}

fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    database: &mut Database,
    logger: &mut Logger,
) -> Result<()> {
    let mut last_tick = Instant::now();
    let mut last_save = Instant::now();

    while !app.quit {
        let size = terminal.size()?;
        if app.screen == AppScreen::Farm {
            app.keep_cursor_visible(size.width, size.height);
        }
        if app.redraw {
            let compatibility = app.compatibility(size.width, size.height);
            terminal.draw(|frame| match app.screen {
                AppScreen::MainMenu => {
                    ui::render_main_menu(frame, app.menu_selection, app.no_color)
                }
                AppScreen::HowToPlay => ui::render_how_to_play(frame, app.no_color),
                AppScreen::Farm => ui::render(
                    frame,
                    &View {
                        game: &app.game,
                        catalog: &app.catalog,
                        upgrades: &app.upgrade_catalog,
                        cursor_row: app.cursor_row,
                        cursor_col: app.cursor_col,
                        offset_row: app.offset_row,
                        offset_col: app.offset_col,
                        status: &app.status,
                        hovered_shop: app.hovered_shop,
                        reset_armed: app.reset_armed,
                        compatibility,
                        no_color: app.no_color,
                    },
                ),
            })?;
            app.redraw = false;
        }

        if event::poll(INPUT_POLL)? {
            let changed = match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    handle_key(app, key, size.width, size.height)
                }
                Event::Mouse(mouse) => handle_mouse(app, mouse, size.width, size.height),
                Event::Resize(_, _) => {
                    app.redraw = true;
                    false
                }
                _ => false,
            };
            if changed {
                save_game(app, database)?;
                last_save = Instant::now();
                logger.write(&app.status);
            }
        }

        if last_tick.elapsed() >= GAME_TICK {
            let elapsed = last_tick.elapsed();
            last_tick = Instant::now();
            let growth_multiplier =
                ACTIVE_GROWTH_MULTIPLIER * app.game.growth_upgrade_multiplier(&app.upgrade_catalog);
            app.game
                .apply_elapsed(elapsed.as_secs_f64(), growth_multiplier);
            let automation =
                app.game
                    .run_automation(elapsed.as_secs_f64(), &app.catalog, &app.upgrade_catalog);
            if let Some(message) = automation.last() {
                app.status = message.clone();
                save_game(app, database)?;
                last_save = Instant::now();
                logger.write(message);
            }
            app.redraw = true;
        }

        if last_save.elapsed() >= AUTOSAVE {
            save_game(app, database)?;
            last_save = Instant::now();
            logger.write("autosaved");
        }
    }
    Ok(())
}

fn handle_key(app: &mut App, key: KeyEvent, width: u16, height: u16) -> bool {
    match app.screen {
        AppScreen::MainMenu => handle_main_menu_key(app, key),
        AppScreen::HowToPlay => handle_how_to_play_key(app, key),
        AppScreen::Farm => handle_farm_key(app, key, width, height),
    }
}

fn handle_main_menu_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Up | KeyCode::Char('w' | 'k') => app.select_previous_menu_item(),
        KeyCode::Down | KeyCode::Char('s' | 'j') => app.select_next_menu_item(),
        KeyCode::Enter | KeyCode::Char(' ') => app.activate_menu_item(),
        KeyCode::Char('q') | KeyCode::Esc => {
            app.quit = true;
            app.redraw = true;
        }
        _ => {}
    }
    false
}

fn handle_how_to_play_key(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Enter | KeyCode::Char(' ') => {
            app.screen = AppScreen::Farm;
            app.redraw = true;
        }
        KeyCode::Esc | KeyCode::Backspace | KeyCode::Char('q' | 'b') => app.open_main_menu(),
        _ => {}
    }
    false
}

fn handle_farm_key(app: &mut App, key: KeyEvent, width: u16, height: u16) -> bool {
    if key.code != KeyCode::Char('x') {
        app.cancel_reset();
    }
    let changed = match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.open_main_menu();
            false
        }
        KeyCode::Up | KeyCode::Char('w' | 'k') => {
            app.move_cursor(-1, 0);
            false
        }
        KeyCode::Down | KeyCode::Char('s' | 'j') => {
            app.move_cursor(1, 0);
            false
        }
        KeyCode::Left | KeyCode::Char('a' | 'h') => {
            app.move_cursor(0, -1);
            false
        }
        KeyCode::Right | KeyCode::Char('d' | 'l') => {
            app.move_cursor(0, 1);
            false
        }
        KeyCode::Enter | KeyCode::Char(' ') => {
            let result = app
                .game
                .use_tile(app.cursor_row, app.cursor_col, &app.catalog);
            app.apply_result(result)
        }
        KeyCode::Char('[') => {
            app.game.select_previous_crop(&app.catalog);
            app.status = format!(
                "Selected {}",
                app.game.selected_definition(&app.catalog).name
            );
            app.redraw = true;
            true
        }
        KeyCode::Char(']') => {
            app.game.select_next_crop(&app.catalog);
            app.status = format!(
                "Selected {}",
                app.game.selected_definition(&app.catalog).name
            );
            app.redraw = true;
            true
        }
        KeyCode::Char('b') => {
            let result = app.game.buy_selected_seed(&app.catalog);
            app.apply_result(result)
        }
        KeyCode::Char('v') => {
            let result = app.game.sell_all(&app.catalog);
            app.apply_result(result)
        }
        KeyCode::Char('n') => {
            let result = app.game.buy_row();
            app.apply_result(result)
        }
        KeyCode::Char('m') => {
            let result = app.game.buy_column();
            app.apply_result(result)
        }
        KeyCode::Char('r') => {
            let result = app.game.rebirth(&app.catalog);
            let changed = app.apply_result(result);
            if changed {
                app.cursor_row = 0;
                app.cursor_col = 0;
                app.offset_row = 0;
                app.offset_col = 0;
            }
            changed
        }
        KeyCode::Char('x') => app.request_reset(),
        KeyCode::Char(key @ '1'..='5') => {
            let index = key as usize - '1' as usize;
            let result = app.game.buy_upgrade(index, &app.upgrade_catalog);
            app.apply_result(result)
        }
        _ => false,
    };
    app.keep_cursor_visible(width, height);
    changed
}

fn handle_mouse(app: &mut App, mouse: MouseEvent, width: u16, height: u16) -> bool {
    if app.screen == AppScreen::MainMenu {
        if let Some(target) = main_menu_target(mouse.column, mouse.row, width, height) {
            if target != app.menu_selection {
                app.menu_selection = target;
                app.redraw = true;
            }
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                app.activate_menu_item();
            }
        }
        return false;
    }
    if app.screen == AppScreen::HowToPlay {
        return false;
    }
    let compatibility = app.compatibility(width, height);
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left | MouseButton::Right) => {
            if let Some(target) = shop_target(
                mouse.column,
                mouse.row,
                width,
                height,
                app.catalog.crops.len(),
                app.upgrade_catalog.upgrades.len(),
                compatibility,
            ) {
                app.hovered_shop = Some(target);
                app.redraw = true;
                if target != ShopTarget::Reset {
                    app.cancel_reset();
                }
                let buy_selected = mouse.kind == MouseEventKind::Down(MouseButton::Right);
                return handle_shop_mouse(app, target, buy_selected);
            }
            app.cancel_reset();
            let Some((row, col)) = mouse_tile(app, mouse, width, height, compatibility) else {
                return false;
            };
            let already_selected = row == app.cursor_row && col == app.cursor_col;
            app.cursor_row = row;
            app.cursor_col = col;
            app.redraw = true;
            if mouse.kind == MouseEventKind::Down(MouseButton::Right) || already_selected {
                let result = app.game.use_tile(row, col, &app.catalog);
                app.apply_result(result)
            } else {
                false
            }
        }
        MouseEventKind::ScrollUp => {
            app.offset_row = app.offset_row.saturating_sub(3);
            app.redraw = true;
            false
        }
        MouseEventKind::ScrollDown => {
            app.offset_row = (app.offset_row + 3).min(app.game.rows.saturating_sub(1));
            app.redraw = true;
            false
        }
        MouseEventKind::Moved => {
            let hovered = shop_target(
                mouse.column,
                mouse.row,
                width,
                height,
                app.catalog.crops.len(),
                app.upgrade_catalog.upgrades.len(),
                compatibility,
            );
            if hovered != app.hovered_shop {
                app.hovered_shop = hovered;
                app.redraw = true;
            }
            false
        }
        _ => false,
    }
}

fn handle_shop_mouse(app: &mut App, target: ShopTarget, buy_selected: bool) -> bool {
    match target {
        ShopTarget::Crop(index) => {
            let already_selected = index == app.game.selected_crop;
            app.game.selected_crop = index;
            if already_selected || buy_selected {
                let result = app.game.buy_selected_seed(&app.catalog);
                app.apply_result(result)
            } else {
                app.status = format!(
                    "Selected {}",
                    app.game.selected_definition(&app.catalog).name
                );
                app.redraw = true;
                true
            }
        }
        ShopTarget::Upgrade(index) => {
            let result = app.game.buy_upgrade(index, &app.upgrade_catalog);
            app.apply_result(result)
        }
        ShopTarget::Row => {
            let result = app.game.buy_row();
            app.apply_result(result)
        }
        ShopTarget::Column => {
            let result = app.game.buy_column();
            app.apply_result(result)
        }
        ShopTarget::Rebirth => {
            let result = app.game.rebirth(&app.catalog);
            let changed = app.apply_result(result);
            if changed {
                app.cursor_row = 0;
                app.cursor_col = 0;
                app.offset_row = 0;
                app.offset_col = 0;
            }
            changed
        }
        ShopTarget::Reset => app.request_reset(),
    }
}

fn mouse_tile(
    app: &App,
    mouse: MouseEvent,
    width: u16,
    height: u16,
    compatibility: bool,
) -> Option<(u32, u32)> {
    let (farm_width, body_y, body_bottom, cell_width, tile_height) = if compatibility {
        (width, 2_u16, height.saturating_sub(4), 2_u16, 1_u16)
    } else {
        (
            width.saturating_mul(58) / 100,
            3_u16,
            height.saturating_sub(3),
            4_u16,
            2_u16,
        )
    };
    if mouse.column == 0
        || mouse.column >= farm_width.saturating_sub(1)
        || mouse.row <= body_y
        || mouse.row >= body_bottom.saturating_sub(1)
    {
        return None;
    }
    let row = app.offset_row + u32::from((mouse.row - body_y - 1) / tile_height);
    let col = app.offset_col + u32::from((mouse.column - 1) / cell_width);
    (row < app.game.rows && col < app.game.cols).then_some((row, col))
}

fn save_game(app: &mut App, database: &mut Database) -> Result<()> {
    let now = unix_time();
    app.game.last_seen_utc = now;
    database.save(&app.game, now)?;
    Ok(())
}

fn resolve_data_dir(args: &Args) -> Result<PathBuf> {
    if let Some(path) = &args.data_dir {
        return Ok(path.clone());
    }
    if args.portable {
        return std::env::current_exe()?
            .parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| anyhow!("could not determine executable directory"));
    }
    ProjectDirs::from("io", "M0KA907", "TerminalFarms")
        .map(|dirs| dirs.data_local_dir().to_path_buf())
        .ok_or_else(|| anyhow!("could not determine user data directory"))
}

fn unicode_terminal() -> bool {
    if cfg!(windows) {
        return true;
    }
    ["LC_ALL", "LC_CTYPE", "LANG"].iter().any(|name| {
        std::env::var(name).is_ok_and(|value| {
            let value = value.to_ascii_uppercase();
            value.contains("UTF-8") || value.contains("UTF8")
        })
    })
}

fn apply_offline_progress(
    game: &mut GameState,
    crops: &CropCatalog,
    upgrades: &UpgradeCatalog,
    seconds: f64,
) -> usize {
    let growth_multiplier = game.growth_upgrade_multiplier(upgrades);
    game.apply_elapsed(seconds, growth_multiplier);
    game.run_automation(seconds, crops, upgrades).len()
}

fn unix_time() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as i64
}

fn duration_text(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{seconds}s")
    }
}

fn move_axis(value: u32, delta: i32, bound: u32) -> u32 {
    if delta < 0 {
        value.saturating_sub(delta.unsigned_abs())
    } else {
        value
            .saturating_add(delta as u32)
            .min(bound.saturating_sub(1))
    }
}

struct RestoreTerminal;

impl Drop for RestoreTerminal {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture);
    }
}

struct Logger {
    file: Option<File>,
}

impl Logger {
    fn new(path: Option<PathBuf>) -> Result<Self> {
        let file = path
            .map(|path| {
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .with_context(|| format!("could not open {}", path.display()))
            })
            .transpose()?;
        Ok(Self { file })
    }

    fn write(&mut self, message: &str) {
        if let Some(file) = &mut self.file {
            let _ = writeln!(file, "{} {message}", unix_time());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        let catalog = CropCatalog::embedded().unwrap();
        let upgrade_catalog = UpgradeCatalog::embedded().unwrap();
        App {
            game: GameState::new(&catalog, 0),
            catalog,
            upgrade_catalog,
            cursor_row: 0,
            cursor_col: 0,
            offset_row: 0,
            offset_col: 0,
            status: "No Radish seeds".into(),
            hovered_shop: None,
            reset_armed: false,
            screen: AppScreen::Farm,
            menu_selection: 0,
            force_compatibility: false,
            no_color: false,
            quit: false,
            redraw: false,
        }
    }

    #[test]
    fn cursor_movement_clamps_to_farm() {
        assert_eq!(move_axis(0, -1, 3), 0);
        assert_eq!(move_axis(2, 1, 3), 2);
        assert_eq!(move_axis(1, -1, 3), 0);
    }

    #[test]
    fn duration_is_compact() {
        assert_eq!(duration_text(90), "1m");
        assert_eq!(duration_text(90_000), "1d 1h");
    }

    #[test]
    fn main_menu_opens_guide_and_farm() {
        let mut app = test_app();
        app.screen = AppScreen::MainMenu;

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Down, crossterm::event::KeyModifiers::NONE),
            80,
            24,
        );
        assert_eq!(app.menu_selection, 1);

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE),
            80,
            24,
        );
        assert_eq!(app.screen, AppScreen::HowToPlay);

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE),
            80,
            24,
        );
        assert_eq!(app.screen, AppScreen::Farm);

        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE),
            80,
            24,
        );
        assert_eq!(app.screen, AppScreen::MainMenu);
        assert!(!app.quit);
    }

    #[test]
    fn main_menu_accepts_mouse_selection() {
        let mut app = test_app();
        app.screen = AppScreen::MainMenu;
        let row = (0..24)
            .find(|row| main_menu_target(40, *row, 80, 24) == Some(1))
            .unwrap();
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 40,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        assert!(!handle_mouse(&mut app, click, 80, 24));
        assert_eq!(app.screen, AppScreen::HowToPlay);
    }

    #[test]
    fn offline_progress_runs_machinery_after_growth() {
        let mut app = test_app();
        app.game.coins = 10_000;
        app.game.run_earnings = 10_000;
        assert!(app.game.buy_upgrade(3, &app.upgrade_catalog).changed());
        assert!(app.game.use_tile(0, 0, &app.catalog).changed());
        assert!(app.game.use_tile(0, 0, &app.catalog).changed());

        let actions =
            apply_offline_progress(&mut app.game, &app.catalog, &app.upgrade_catalog, 30.0);

        assert_eq!(actions, 1);
        assert_eq!(app.game.tile(0, 0), Some(&terminalfarms::TileState::Tilled));
        assert_eq!(app.game.produce.get("radish"), Some(&1));
    }

    #[test]
    fn reset_requires_a_warning_and_confirmation() {
        let mut app = test_app();
        app.game.coins = 999;
        app.game.run_earnings = 500;

        assert!(!app.request_reset());
        assert!(app.reset_armed);
        assert_eq!(app.game.coins, 999);
        assert!(app.status.contains("deletes all progress"));

        assert!(app.request_reset());
        assert!(!app.reset_armed);
        assert_eq!(app.game.coins, terminalfarms::game::STARTING_COINS);
        assert_eq!(app.game.run_earnings, 0);
    }

    #[test]
    fn shop_mouse_clicks_buy_land_instead_of_using_tile() {
        let mut app = test_app();
        app.game.coins = 1_000;
        let row_click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: 64,
            row: 15,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert!(handle_mouse(&mut app, row_click, 80, 24));
        assert_eq!(app.game.rows, 4);
        assert!(app.status.starts_with("Bought row"));

        let column_click = MouseEvent {
            row: 16,
            ..row_click
        };
        assert!(handle_mouse(&mut app, column_click, 80, 24));
        assert_eq!(app.game.cols, 4);
        assert!(app.status.starts_with("Bought column"));
    }

    #[test]
    fn mouse_movement_tracks_shop_hover() {
        let mut app = test_app();
        let shop_move = MouseEvent {
            kind: MouseEventKind::Moved,
            column: 64,
            row: 15,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        assert!(!handle_mouse(&mut app, shop_move, 80, 24));
        assert_eq!(app.hovered_shop, Some(ShopTarget::Row));
        assert!(app.redraw);

        app.redraw = false;
        let farm_move = MouseEvent {
            column: 20,
            row: 10,
            ..shop_move
        };
        assert!(!handle_mouse(&mut app, farm_move, 80, 24));
        assert_eq!(app.hovered_shop, None);
        assert!(app.redraw);
    }
}
