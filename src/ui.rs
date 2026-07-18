use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
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

    let shop_left = width.saturating_mul(68) / 100;
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
        width.saturating_mul(68) / 100
    };
    let cols = farm_width.saturating_sub(2) / cell_width;
    let rows = if compatibility {
        height.saturating_sub(8)
    } else {
        height.saturating_sub(9) / 2
    };
    (u32::from(rows.max(1)), u32::from(cols.max(1)))
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
        .constraints([Constraint::Percentage(68), Constraint::Percentage(32)])
        .split(sections[1]);
    render_farm(frame, body[0], view, 4);
    render_shop(frame, body[1], view);

    let selected = view.game.selected_definition(view.catalog);
    let seeds = view.game.seeds.get(&selected.id).copied().unwrap_or(0);
    let produce = view.game.produce.get(&selected.id).copied().unwrap_or(0);
    let footer = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(" MOVE ", label(view)),
            Span::raw("↑↓←→ · WASD   "),
            Span::styled(" ACT ", label(view)),
            Span::raw("Enter   "),
            Span::styled(" SEED ", label(view)),
            Span::raw("[ ] · b   "),
            Span::styled(" SHOP ", label(view)),
            Span::raw("1–5   "),
            Span::styled(" SELL ", label(view)),
            Span::raw("v   "),
            Span::styled(" EXIT ", label(view)),
            Span::raw("q"),
        ]),
        Line::from(vec![
            Span::styled(" ◆ ", styled(view, Color::LightGreen)),
            Span::styled(view.status, styled(view, Color::LightYellow)),
            Span::raw(format!(
                "  ·  {} s:{} p:{}  ·  {}s",
                selected.name, seeds, produce, selected.grow_seconds
            )),
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
    frame.render_widget(
        Paragraph::new(vec![
            Line::raw(format!(
                "{} seed:{} buy:${} sell:${} grow:{}s",
                crop.name, seeds, crop.seed_price, crop.sell_price, crop.grow_seconds
            )),
            Line::raw("arrows/WASD/hjkl move | Enter act | [ ] seed | b buy | 1-5 machines"),
            Line::raw("n row | m column | r rebirth | x reset | q quit"),
            Line::raw(view.status.to_owned()),
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
        Span::styled(" SEEDS ", label(view)),
        Span::raw(" [ ] select · b"),
    ])];
    for (index, crop) in view.catalog.crops.iter().enumerate() {
        let selected = index == view.game.selected_crop;
        let unlocked = view.game.run_earnings >= crop.unlock_earnings;
        let seeds = view.game.seeds.get(&crop.id).copied().unwrap_or(0);
        let produce = view.game.produce.get(&crop.id).copied().unwrap_or(0);
        let marker = if selected { ">" } else { " " };
        let text = if unlocked {
            format!(
                "{marker} {:<8} s{seeds:<2} ${:<4} p{produce}",
                crop.name, crop.seed_price
            )
        } else {
            format!("{marker} {:<8} lock ${}", crop.name, crop.unlock_earnings)
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
        Span::styled(" MACHINERY ", label(view)),
        Span::raw(" 1–5 buy"),
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
        format!(" LAND  n row     ${}", view.game.row_cost()),
        shop_row_style(view, Color::White, ShopTarget::Row),
    ));
    lines.push(Line::styled(
        format!("       m column  ${}", view.game.column_cost()),
        shop_row_style(view, Color::White, ShopTarget::Column),
    ));
    lines.push(Line::styled(
        format!(" REBIRTH  r · need ${}", view.game.rebirth_requirement()),
        shop_row_style(view, Color::White, ShopTarget::Rebirth),
    ));
    lines.push(Line::styled(
        if view.reset_armed {
            " RESET  x again · DELETE ALL PROGRESS".to_owned()
        } else {
            " RESET  x · delete progress".to_owned()
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
