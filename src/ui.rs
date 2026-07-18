use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

use terminalfarms::{
    CropCatalog, GameState, TileState, UpgradeCatalog, game::ACTIVE_GROWTH_MULTIPLIER,
};

pub struct View<'a> {
    pub game: &'a GameState,
    pub catalog: &'a CropCatalog,
    pub upgrades: &'a UpgradeCatalog,
    pub cursor_row: u32,
    pub cursor_col: u32,
    pub offset_row: u32,
    pub offset_col: u32,
    pub status: &'a str,
    pub hovered_shop: Option<ShopTarget>,
    pub reset_armed: bool,
    pub compatibility: bool,
    pub no_color: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShopTarget {
    Crop(usize),
    Upgrade(usize),
    Row,
    Column,
    Rebirth,
    Reset,
}

pub fn shop_target(
    column: u16,
    row: u16,
    width: u16,
    height: u16,
    crop_count: usize,
    upgrade_count: usize,
    compatibility: bool,
) -> Option<ShopTarget> {
    if compatibility || width < 80 || height < 24 {
        return None;
    }

    let shop_left = width.saturating_mul(58) / 100;
    if column <= shop_left || column >= width.saturating_sub(1) {
        return None;
    }

    let crop_start = 5_u16;
    let upgrade_start = crop_start + crop_count as u16 + 1;
    if (crop_start..crop_start + crop_count as u16).contains(&row) {
        return Some(ShopTarget::Crop((row - crop_start) as usize));
    }
    if (upgrade_start..upgrade_start + upgrade_count as u16).contains(&row) {
        return Some(ShopTarget::Upgrade((row - upgrade_start) as usize));
    }

    let land_row = upgrade_start + upgrade_count as u16;
    match row {
        value if value == land_row => Some(ShopTarget::Row),
        value if value == land_row + 1 => Some(ShopTarget::Column),
        value if value == land_row + 2 => Some(ShopTarget::Rebirth),
        value if value == land_row + 3 => Some(ShopTarget::Reset),
        _ => None,
    }
}

pub fn viewport_capacity(width: u16, height: u16, compatibility: bool) -> (u32, u32) {
    let cell_width = if compatibility { 2 } else { 4 };
    let farm_width = if compatibility {
        width
    } else {
        width.saturating_mul(58) / 100
    };
    let cols = farm_width.saturating_sub(2) / cell_width;
    let rows = if compatibility {
        height.saturating_sub(8)
    } else {
        height.saturating_sub(9) / 2
    };
    (u32::from(rows.max(1)), u32::from(cols.max(1)))
}

pub fn main_menu_target(column: u16, row: u16, width: u16, height: u16) -> Option<usize> {
    let area = Rect::new(0, 0, width, height);
    if width < 60 || height < 20 {
        if column == 0 || column >= width.saturating_sub(1) {
            return None;
        }
        return match row {
            4 => Some(0),
            6 => Some(1),
            8 => Some(2),
            _ => None,
        };
    }

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(7),
            Constraint::Length(9),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);
    let menu = centered_width(vertical[2], 40);
    if column <= menu.x || column >= menu.right().saturating_sub(1) {
        return None;
    }
    match row {
        value if value == menu.y + 2 => Some(0),
        value if value == menu.y + 4 => Some(1),
        value if value == menu.y + 6 => Some(2),
        _ => None,
    }
}

pub fn render_main_menu(frame: &mut Frame<'_>, selected: usize, no_color: bool) {
    let area = frame.area();
    if area.width < 60 || area.height < 20 {
        render_compact_main_menu(frame, selected, no_color);
        return;
    }

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(7),
            Constraint::Length(9),
            Constraint::Length(3),
            Constraint::Min(1),
        ])
        .split(area);

    let title_area = centered_width(vertical[1], 58);
    let title = Paragraph::new(vec![
        Line::raw("          ╭───────╮"),
        Line::styled(
            "          │  ♧ ♧  │",
            menu_style(no_color, Color::LightGreen).add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            "       TERMINAL FARMS",
            menu_style(no_color, Color::LightYellow).add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            "    GROW · AUTOMATE · REBIRTH",
            menu_style(no_color, Color::Rgb(126, 177, 112)),
        ),
    ])
    .alignment(Alignment::Center)
    .block(menu_panel(no_color, " WELCOME "));
    frame.render_widget(title, title_area);

    let menu_area = centered_width(vertical[2], 40);
    let options = ["ENTER FARM", "HOW TO PLAY", "QUIT"];
    let mut lines = vec![Line::raw("")];
    for (index, option) in options.iter().enumerate() {
        let marker = if index == selected { "  ▸ " } else { "    " };
        let style = if index == selected {
            selected_menu_style(no_color)
        } else {
            menu_style(no_color, Color::White)
        };
        lines.push(Line::styled(format!("{marker}{option:<18}"), style));
        lines.push(Line::raw(""));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(menu_panel(no_color, " MAIN MENU ")),
        menu_area,
    );

    frame.render_widget(
        Paragraph::new("[↑/↓] Move selection   ·   [ENTER] Choose   ·   [Q] Quit")
            .alignment(Alignment::Center)
            .style(menu_style(no_color, Color::DarkGray)),
        vertical[3],
    );
}

fn render_compact_main_menu(frame: &mut Frame<'_>, selected: usize, no_color: bool) {
    let area = frame.area();
    let options = ["ENTER FARM", "HOW TO PLAY", "QUIT"];
    let mut lines = vec![
        Line::styled(
            "TERMINAL FARMS",
            menu_style(no_color, Color::LightGreen).add_modifier(Modifier::BOLD),
        ),
        Line::styled(
            "grow · automate · rebirth",
            menu_style(no_color, Color::LightYellow),
        ),
        Line::raw(""),
    ];
    for (index, option) in options.iter().enumerate() {
        let marker = if index == selected { "> " } else { "  " };
        let style = if index == selected {
            selected_menu_style(no_color)
        } else {
            menu_style(no_color, Color::White)
        };
        lines.push(Line::styled(format!("{marker}{option}"), style));
        lines.push(Line::raw(""));
    }
    lines.push(Line::styled(
        "[ARROWS] Move · [ENTER] Choose · [Q] Quit",
        menu_style(no_color, Color::DarkGray),
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(menu_panel(no_color, " MENU ")),
        area,
    );
}

pub fn render_how_to_play(frame: &mut Frame<'_>, no_color: bool) {
    let area = frame.area();
    if area.width < 96 || area.height < 24 {
        let guide = Paragraph::new(vec![
            guide_title(no_color, "HOW TO PLAY"),
            Line::raw(""),
            Line::raw("TILE LOOP: Enter tills > plants > harvests"),
            Line::raw("Buy seed > Use tile > Wait > Use tile > Sell"),
            Line::raw(""),
            Line::raw("[ARROWS/WASD] Move   [ENTER/SPACE] Use tile"),
            Line::raw("[LEFT/RIGHT BRACKET] Change crop"),
            Line::raw("[B] Buy seed   [V] Sell produce   [ESC] Menu"),
            Line::raw("[1-5] Machines   [N/M] Land   [R] Rebirth"),
            Line::raw("[X twice] Reset all progress"),
            Line::raw(""),
            Line::styled(
                "[ESC/B] Back · [ENTER] Start farming",
                menu_style(no_color, Color::LightYellow),
            ),
        ])
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
        .block(menu_panel(no_color, " GUIDE "));
        frame.render_widget(guide, centered_height(area, 14));
        return;
    }

    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(12),
            Constraint::Length(3),
        ])
        .split(area);
    frame.render_widget(
        Paragraph::new(vec![
            guide_title(no_color, "HOW TO PLAY"),
            Line::styled(
                "Build a tiny farm into a self-running harvest machine.",
                menu_style(no_color, Color::Rgb(126, 177, 112)),
            ),
        ])
        .alignment(Alignment::Center)
        .block(menu_panel(no_color, " FIELD GUIDE ")),
        centered_width(vertical[0], 76),
    );

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ])
        .split(vertical[1]);
    frame.render_widget(
        Paragraph::new(vec![
            guide_title(no_color, "THE FARM LOOP"),
            Line::raw(""),
            Line::raw("1  Till an empty tile"),
            Line::raw("2  Sow your selected seed"),
            Line::raw("3  Let the crop grow"),
            Line::raw("4  Harvest when it is ready"),
            Line::raw("5  Sell produce for cash"),
            Line::raw(""),
            Line::styled(
                "[ENTER] automatically does the correct tile action.",
                menu_style(no_color, Color::LightGreen),
            ),
        ])
        .wrap(Wrap { trim: true })
        .block(menu_panel(no_color, " START HERE ")),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(vec![
            guide_title(no_color, "ESSENTIAL KEYS"),
            Line::raw(""),
            Line::raw("[ARROWS/WASD] Move cursor"),
            Line::raw("[ENTER/SPACE] Use tile"),
            Line::raw("[LEFT BRACKET] Previous crop"),
            Line::raw("[RIGHT BRACKET] Next crop"),
            Line::raw("[B] Buy selected seed"),
            Line::raw("[V] Sell all produce"),
            Line::raw("[ESC] Main menu"),
        ])
        .wrap(Wrap { trim: true })
        .block(menu_panel(no_color, " CONTROLS ")),
        columns[1],
    );
    frame.render_widget(
        Paragraph::new(vec![
            guide_title(no_color, "GROW FURTHER"),
            Line::raw(""),
            Line::raw("Machines keep working while the game is closed."),
            Line::raw(""),
            Line::raw("[1-5] Buy machines"),
            Line::raw("[N/M] Buy a row or column"),
            Line::raw("[R] Rebirth for a permanent bonus"),
            Line::raw("[X twice] Reset all progress"),
            Line::styled(
                "Your farm autosaves locally.",
                menu_style(no_color, Color::LightGreen),
            ),
        ])
        .wrap(Wrap { trim: true })
        .block(menu_panel(no_color, " AUTOMATION & SAVES ")),
        columns[2],
    );

    frame.render_widget(
        Paragraph::new("[ESC/B] Back to menu     ·     [ENTER] Start farming")
            .alignment(Alignment::Center)
            .style(menu_style(no_color, Color::LightYellow)),
        vertical[2],
    );
}

fn centered_width(area: Rect, width: u16) -> Rect {
    let width = width.min(area.width);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(width),
            Constraint::Min(0),
        ])
        .split(area)[1]
}

fn centered_height(area: Rect, height: u16) -> Rect {
    let height = height.min(area.height);
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(height),
            Constraint::Min(0),
        ])
        .split(area)[1]
}

fn menu_panel<'a>(no_color: bool, title: &'a str) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(menu_style(no_color, Color::Rgb(93, 135, 81)))
}

fn menu_style(no_color: bool, color: Color) -> Style {
    if no_color {
        Style::default()
    } else {
        Style::default().fg(color)
    }
}

fn selected_menu_style(no_color: bool) -> Style {
    if no_color {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Rgb(173, 193, 111))
            .add_modifier(Modifier::BOLD)
    }
}

fn guide_title(no_color: bool, title: &str) -> Line<'static> {
    Line::styled(
        title.to_owned(),
        menu_style(no_color, Color::LightYellow).add_modifier(Modifier::BOLD),
    )
}

pub fn render(frame: &mut Frame<'_>, view: &View<'_>) {
    if view.compatibility {
        render_compatibility(frame, view);
    } else {
        render_full(frame, view);
    }
}

fn render_full(frame: &mut Frame<'_>, view: &View<'_>) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(8),
            Constraint::Length(4),
        ])
        .split(frame.area());

    let machinery_levels: u32 = view.game.upgrades.values().map(|state| state.level).sum();
    let active_growth =
        (ACTIVE_GROWTH_MULTIPLIER * view.game.growth_upgrade_multiplier(view.upgrades) * 100.0
            - 100.0)
            .round() as u32;
    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            "  TERMINAL FARMS  ",
            styled(view, Color::LightGreen).add_modifier(Modifier::BOLD),
        ),
        stat(view, " CASH ", format!("${}", view.game.coins)),
        Span::raw("  "),
        stat(view, " EARNED ", format!("${}", view.game.run_earnings)),
        Span::raw("  "),
        stat(view, " REBIRTH ", view.game.rebirth_count.to_string()),
        Span::raw(format!(
            "  +{}%  ⚙{}  GROW +{}%",
            view.game.rebirth_tokens * 10,
            machinery_levels,
            active_growth
        )),
    ]))
    .block(panel(view, ""));
    frame.render_widget(header, sections[0]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(sections[1]);
    render_farm(frame, body[0], view, 4);
    render_shop(frame, body[1], view);

    let next_action = tile_action_hint(view);
    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("[ARROWS]", label(view)),
            Span::raw(" MOVE  "),
            Span::styled("[ENTER]", label(view)),
            Span::raw(" TILE  "),
            Span::styled("[ / ]", label(view)),
            Span::raw(" CROP  "),
            Span::styled("[B]", label(view)),
            Span::raw(" BUY  "),
            Span::styled("[V]", label(view)),
            Span::raw(" SELL  "),
            Span::styled("[ESC]", label(view)),
            Span::raw(" MENU"),
        ]),
        Line::from(vec![
            Span::styled(" NEXT ", label(view)),
            Span::styled(next_action, styled(view, Color::LightGreen)),
            Span::raw("  ·  "),
            Span::styled(view.status, styled(view, Color::LightYellow)),
        ]),
    ])
    .block(panel(view, ""));
    frame.render_widget(footer, sections[2]);
}

fn stat<'a>(view: &View<'_>, label: &'a str, value: String) -> Span<'a> {
    Span::styled(
        format!("{label}{value} "),
        styled(view, Color::LightYellow).add_modifier(Modifier::BOLD),
    )
}

fn panel<'a>(view: &View<'_>, title: impl Into<Line<'a>>) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(styled(view, Color::Rgb(93, 135, 81)))
}

fn label(view: &View<'_>) -> Style {
    styled(view, Color::Black)
        .bg(if view.no_color {
            Color::Reset
        } else {
            Color::Rgb(173, 148, 86)
        })
        .add_modifier(Modifier::BOLD)
}

fn tile_action_hint(view: &View<'_>) -> String {
    match view.game.tile(view.cursor_row, view.cursor_col) {
        Some(TileState::Untilled) => "[ENTER] Till this tile".into(),
        Some(TileState::Tilled) => {
            let crop = view.game.selected_definition(view.catalog);
            let seeds = view.game.seeds.get(&crop.id).copied().unwrap_or(0);
            if view.game.run_earnings < crop.unlock_earnings {
                format!("Earn ${} to unlock {}", crop.unlock_earnings, crop.name)
            } else if seeds == 0 {
                format!("[B] Buy a {} seed", crop.name)
            } else {
                format!("[ENTER] Plant {}", crop.name)
            }
        }
        Some(TileState::Planted { crop_id, progress }) => {
            let Some(crop) = view.catalog.get(crop_id) else {
                return "This crop is unavailable".into();
            };
            if *progress >= crop.grow_seconds as f64 {
                format!("[ENTER] Harvest {}", crop.name)
            } else {
                let remaining = (crop.grow_seconds as f64 - progress).ceil() as u64;
                format!("{} is growing · {remaining}s left", crop.name)
            }
        }
        None => "Move onto a farm tile".into(),
    }
}

fn render_compatibility(frame: &mut Frame<'_>, view: &View<'_>) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(4),
            Constraint::Length(4),
        ])
        .split(frame.area());
    frame.render_widget(
        Paragraph::new(format!(
            "TERMINAL FARMS  ${}  earned ${}  rebirth {}",
            view.game.coins, view.game.run_earnings, view.game.rebirth_count
        )),
        sections[0],
    );
    render_farm(frame, sections[1], view, 2);

    let crop = view.game.selected_definition(view.catalog);
    let seeds = view.game.seeds.get(&crop.id).copied().unwrap_or(0);
    let produce = view.game.produce.get(&crop.id).copied().unwrap_or(0);
    frame.render_widget(
        Paragraph::new(vec![
            Line::raw(format!(
                "{} seeds:{} produce:{} | buy:${} sell:${} grow:{}s",
                crop.name, seeds, produce, crop.seed_price, crop.sell_price, crop.grow_seconds
            )),
            Line::raw("[ARROWS] Move | [ENTER] Use tile"),
            Line::raw("[ / ] Crop | [B] Buy | [V] Sell | [ESC] Menu"),
            Line::raw(format!("NEXT: {}", tile_action_hint(view))),
        ]),
        sections[2],
    );
}

fn render_farm(frame: &mut Frame<'_>, area: Rect, view: &View<'_>, cell_width: u16) {
    let tile_height = if view.compatibility { 1 } else { 2 };
    let visible_rows = u32::from(area.height.saturating_sub(2) / tile_height);
    let visible_cols = u32::from(area.width.saturating_sub(2) / cell_width).max(1);
    let row_end = (view.offset_row + visible_rows).min(view.game.rows);
    let col_end = (view.offset_col + visible_cols).min(view.game.cols);
    let mut lines = Vec::new();

    for row in view.offset_row..row_end {
        for art_row in 0..tile_height {
            let mut spans = Vec::new();
            for col in view.offset_col..col_end {
                let selected = row == view.cursor_row && col == view.cursor_col;
                spans.extend(tile_spans(view, row, col, selected, art_row as usize));
            }
            lines.push(Line::from(spans));
        }
    }

    let ready = view
        .game
        .tiles
        .iter()
        .filter(|tile| match tile {
            TileState::Planted { crop_id, progress } => view
                .catalog
                .get(crop_id)
                .is_some_and(|crop| *progress >= crop.grow_seconds as f64),
            _ => false,
        })
        .count();
    let title = format!(
        " FIELD  {}×{}  ·  {} ready  ·  view {},{} ",
        view.game.rows,
        view.game.cols,
        ready,
        view.offset_row + 1,
        view.offset_col + 1
    );
    if view.compatibility {
        lines.insert(
            0,
            Line::raw(format!(
                "FARM {}x{} | {} ready | view {},{}",
                view.game.rows,
                view.game.cols,
                ready,
                view.offset_row + 1,
                view.offset_col + 1
            )),
        );
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), area);
        return;
    }
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(view, title))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn tile_spans<'a>(
    view: &View<'a>,
    row: u32,
    col: u32,
    selected: bool,
    art_row: usize,
) -> Vec<Span<'a>> {
    let tile = view.game.tile(row, col).expect("visible farm tile");
    let (text, color, background, ready) = match tile {
        TileState::Untilled => {
            if view.compatibility {
                ("##".to_owned(), Color::DarkGray, Color::Reset, false)
            } else {
                (
                    "▓▓▓".to_owned(),
                    Color::Rgb(67, 73, 58),
                    Color::Rgb(41, 47, 39),
                    false,
                )
            }
        }
        TileState::Tilled => {
            if view.compatibility {
                ("..".to_owned(), Color::Yellow, Color::Reset, false)
            } else {
                (
                    if art_row == 0 { " ░ " } else { "░░░" }.to_owned(),
                    Color::Rgb(160, 110, 72),
                    Color::Rgb(75, 47, 33),
                    false,
                )
            }
        }
        TileState::Planted { crop_id, progress } => {
            let crop = view.catalog.get(crop_id).expect("validated crop id");
            let stage = GameState::growth_stage(*progress, crop.grow_seconds);
            let text = if view.compatibility {
                format!("{} ", ["o", "i", "Y", "*"][stage])
            } else {
                crop.art[stage][art_row].clone()
            };
            (
                text,
                crop_color(&crop.color),
                if view.compatibility {
                    Color::Reset
                } else {
                    Color::Rgb(75, 47, 33)
                },
                stage == 3,
            )
        }
    };

    let mut style = styled(view, color);
    if !view.no_color {
        style = style.bg(background);
    }
    if ready {
        style = style.add_modifier(Modifier::BOLD);
    }
    if selected {
        style = if view.no_color {
            style.add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            style
                .bg(Color::Rgb(205, 173, 97))
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD)
        };
    }
    let mut spans = vec![Span::styled(text, style)];
    if !view.compatibility {
        spans.push(Span::raw(" "));
    }
    spans
}

fn render_shop(frame: &mut Frame<'_>, area: Rect, view: &View<'_>) {
    let mut lines = vec![Line::from(vec![
        Span::styled(" CROPS ", label(view)),
        Span::raw(" [ / ] · [B] buy"),
    ])];
    for (index, crop) in view.catalog.crops.iter().enumerate() {
        let selected = index == view.game.selected_crop;
        let unlocked = view.game.run_earnings >= crop.unlock_earnings;
        let seeds = view.game.seeds.get(&crop.id).copied().unwrap_or(0);
        let produce = view.game.produce.get(&crop.id).copied().unwrap_or(0);
        let marker = if selected { ">" } else { " " };
        let text = if unlocked {
            crop_inventory_line(marker, &crop.name, crop.seed_price, seeds, produce)
        } else {
            locked_crop_inventory_line(marker, &crop.name, seeds, produce)
        };
        let color = if selected {
            Color::LightYellow
        } else if unlocked {
            Color::White
        } else {
            Color::DarkGray
        };
        lines.push(Line::styled(
            text,
            shop_row_style(view, color, ShopTarget::Crop(index)),
        ));
    }

    lines.push(Line::from(vec![
        Span::styled(" MACHINES ", label(view)),
        Span::raw(" [1-5] buy"),
    ]));
    for (index, upgrade) in view.upgrades.upgrades.iter().enumerate() {
        let level = view.game.upgrade_level(&upgrade.id);
        let text = if view.game.run_earnings < upgrade.unlock_earnings {
            format!(
                "{} {:<12} ≥${}",
                index + 1,
                upgrade.name,
                upgrade.unlock_earnings
            )
        } else if level >= upgrade.max_level {
            format!("{} {:<12} L{level} MAX", index + 1, upgrade.name)
        } else {
            format!(
                "{} {:<12} L{level} ${}",
                index + 1,
                upgrade.name,
                view.game.upgrade_cost(upgrade)
            )
        };
        lines.push(Line::styled(
            text,
            shop_row_style(
                view,
                if level > 0 {
                    Color::LightGreen
                } else {
                    Color::White
                },
                ShopTarget::Upgrade(index),
            ),
        ));
    }
    lines.push(Line::styled(
        format!(" [N] ROW      ${}", view.game.row_cost()),
        shop_row_style(view, Color::White, ShopTarget::Row),
    ));
    lines.push(Line::styled(
        format!(" [M] COLUMN   ${}", view.game.column_cost()),
        shop_row_style(view, Color::White, ShopTarget::Column),
    ));
    lines.push(Line::styled(
        format!(" [R] REBIRTH  ${}", view.game.rebirth_requirement()),
        shop_row_style(view, Color::White, ShopTarget::Rebirth),
    ));
    lines.push(Line::styled(
        if view.reset_armed {
            " [X] CONFIRM DELETE".to_owned()
        } else {
            " [X][X] RESET FARM".to_owned()
        },
        shop_row_style(view, Color::LightRed, ShopTarget::Reset),
    ));

    frame.render_widget(Paragraph::new(lines).block(panel(view, " SHOP ")), area);
}

fn shop_row_style(view: &View<'_>, color: Color, target: ShopTarget) -> Style {
    let style = styled(view, color);
    if view.hovered_shop != Some(target) {
        return style;
    }
    if view.no_color {
        style.add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        style
            .bg(Color::Rgb(52, 76, 49))
            .add_modifier(Modifier::BOLD)
    }
}

fn crop_inventory_line(
    marker: &str,
    name: &str,
    seed_price: u64,
    seeds: u32,
    produce: u32,
) -> String {
    format!("{marker}{name:<6} ${seed_price} seeds:{seeds} produce:{produce}")
}

fn locked_crop_inventory_line(marker: &str, name: &str, seeds: u32, produce: u32) -> String {
    format!("{marker}{name:<6} locked seeds:{seeds} produce:{produce}")
}

fn styled(view: &View<'_>, color: Color) -> Style {
    if view.no_color {
        Style::default()
    } else {
        Style::default().fg(color)
    }
}

fn crop_color(color: &str) -> Color {
    match color {
        "red" => Color::LightRed,
        "yellow" => Color::LightYellow,
        "magenta" => Color::LightMagenta,
        "green" => Color::LightGreen,
        _ => Color::White,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    fn test_view<'a>(
        game: &'a GameState,
        catalog: &'a CropCatalog,
        upgrades: &'a UpgradeCatalog,
    ) -> View<'a> {
        View {
            game,
            catalog,
            upgrades,
            cursor_row: 0,
            cursor_col: 0,
            offset_row: 0,
            offset_col: 0,
            status: "Ready",
            hovered_shop: None,
            reset_armed: false,
            compatibility: false,
            no_color: true,
        }
    }

    #[test]
    fn menus_render_at_large_and_compact_sizes() {
        for (width, height) in [(100, 30), (80, 24), (40, 14)] {
            let backend = TestBackend::new(width, height);
            let mut terminal = Terminal::new(backend).unwrap();
            terminal
                .draw(|frame| render_main_menu(frame, 1, false))
                .unwrap();
            terminal
                .draw(|frame| render_how_to_play(frame, false))
                .unwrap();
        }
    }

    #[test]
    fn main_menu_hit_test_tracks_rendered_options() {
        assert_eq!(main_menu_target(50, 15, 100, 30), Some(0));
        assert_eq!(main_menu_target(20, 6, 40, 14), Some(1));
        assert_eq!(main_menu_target(0, 6, 40, 14), None);
    }

    #[test]
    fn selected_tile_hint_explains_the_next_action() {
        let catalog = CropCatalog::embedded().unwrap();
        let upgrades = UpgradeCatalog::embedded().unwrap();
        let mut game = GameState::new(&catalog, 0);

        assert!(tile_action_hint(&test_view(&game, &catalog, &upgrades)).contains("Till"));
        game.use_tile(0, 0, &catalog);
        assert!(tile_action_hint(&test_view(&game, &catalog, &upgrades)).contains("Plant"));
        game.use_tile(0, 0, &catalog);
        assert!(tile_action_hint(&test_view(&game, &catalog, &upgrades)).contains("growing"));
        game.apply_elapsed(30.0, 1.0);
        assert!(tile_action_hint(&test_view(&game, &catalog, &upgrades)).contains("Harvest"));
    }

    #[test]
    fn every_crop_counter_uses_full_inventory_labels() {
        let catalog = CropCatalog::embedded().unwrap();
        for crop in &catalog.crops {
            for line in [
                crop_inventory_line(">", &crop.name, crop.seed_price, 12, 34),
                locked_crop_inventory_line(">", &crop.name, 12, 34),
            ] {
                assert!(line.contains(&crop.name));
                assert!(line.contains("seeds:12"));
                assert!(line.contains("produce:34"));
            }
        }
    }

    #[test]
    fn compatibility_ui_renders_in_small_terminal() {
        let catalog = CropCatalog::embedded().unwrap();
        let upgrades = UpgradeCatalog::embedded().unwrap();
        let game = GameState::new(&catalog, 0);
        let backend = TestBackend::new(40, 14);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    &View {
                        game: &game,
                        catalog: &catalog,
                        upgrades: &upgrades,
                        cursor_row: 0,
                        cursor_col: 0,
                        offset_row: 0,
                        offset_col: 0,
                        status: "Ready",
                        hovered_shop: None,
                        reset_armed: false,
                        compatibility: true,
                        no_color: true,
                    },
                );
            })
            .unwrap();
    }

    #[test]
    fn full_ui_renders_large_tiles_and_shop() {
        let catalog = CropCatalog::embedded().unwrap();
        let upgrades = UpgradeCatalog::embedded().unwrap();
        let mut game = GameState::new(&catalog, 0);
        game.use_tile(0, 0, &catalog);
        game.use_tile(0, 0, &catalog);
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                render(
                    frame,
                    &View {
                        game: &game,
                        catalog: &catalog,
                        upgrades: &upgrades,
                        cursor_row: 0,
                        cursor_col: 0,
                        offset_row: 0,
                        offset_col: 0,
                        status: "Sowed Radish",
                        hovered_shop: Some(ShopTarget::Row),
                        reset_armed: false,
                        compatibility: false,
                        no_color: false,
                    },
                );
            })
            .unwrap();
    }

    #[test]
    fn shop_hit_test_maps_land_rows() {
        assert_eq!(
            shop_target(64, 15, 80, 24, 4, 5, false),
            Some(ShopTarget::Row)
        );
        assert_eq!(
            shop_target(64, 16, 80, 24, 4, 5, false),
            Some(ShopTarget::Column)
        );
        assert_eq!(
            shop_target(64, 18, 80, 24, 4, 5, false),
            Some(ShopTarget::Reset)
        );
        assert_eq!(shop_target(20, 15, 80, 24, 4, 5, false), None);
    }
}
