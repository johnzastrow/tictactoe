//! Rendering. Pure function of `App` (apart from recording cell rects for
//! mouse hit-testing).

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use ttt_core::{Player, Status};

use crate::{app::App, theme::Theme};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const CELL_NAMES: [&str; 9] = [
    "top-left",
    "top",
    "top-right",
    "left",
    "centre",
    "right",
    "bottom-left",
    "bottom",
    "bottom-right",
];
const MIN_W: u16 = 34;
const MIN_H: u16 = 20;

/// Glyph art per board size. Each row must be the same width.
const X_BIG: [&str; 5] = ["██   ██", " ██ ██ ", "  ███  ", " ██ ██ ", "██   ██"];
const O_BIG: [&str; 5] = [" █████ ", "██   ██", "██   ██", "██   ██", " █████ "];
const X_MID: [&str; 3] = ["╲ ╱", " ╳ ", "╱ ╲"];
const O_MID: [&str; 3] = ["╭─╮", "│ │", "╰─╯"];

/// (cell width, cell height) options, largest first.
const SIZES: [(u16, u16); 3] = [(11, 5), (7, 3), (3, 1)];

pub fn draw(f: &mut Frame, app: &mut App) {
    let t = *app.theme();
    let area = f.area();
    f.render_widget(Block::new().style(Style::new().bg(t.bg).fg(t.fg)), area);

    if area.width < MIN_W || area.height < MIN_H {
        let msg = Paragraph::new(format!(
            "Terminal too small\n{}×{} (need {MIN_W}×{MIN_H})",
            area.width, area.height
        ))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true });
        f.render_widget(
            msg,
            center(area, Constraint::Percentage(100), Constraint::Length(2)),
        );
        app.cell_rects = Default::default();
        return;
    }

    let [header, body, footer] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(f, app, &t, header);

    // Drop the sidebar on narrow terminals rather than squashing the board.
    if body.width >= 76 {
        let [board, side] =
            Layout::horizontal([Constraint::Min(42), Constraint::Length(34)]).areas(body);
        draw_board_panel(f, app, &t, board);
        draw_sidebar(f, app, &t, side);
    } else {
        draw_board_panel(f, app, &t, body);
    }
    draw_footer(f, &t, footer);

    if app.show_help {
        draw_help(f, &t, area);
    }
}

fn panel<'a>(t: &Theme, title: &'a str) -> Block<'a> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(t.grid))
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(t.muted).bold(),
        ))
        .style(Style::new().bg(t.panel).fg(t.fg))
}

fn player_color(t: &Theme, p: Player) -> ratatui::style::Color {
    match p {
        Player::X => t.x,
        Player::O => t.o,
    }
}

fn draw_header(f: &mut Frame, app: &App, t: &Theme, area: Rect) {
    let title = Line::from(vec![
        Span::raw(" "),
        Span::styled("✕", Style::new().fg(t.x).bold()),
        Span::raw(" "),
        Span::styled("◯", Style::new().fg(t.o).bold()),
        Span::styled("  TIC-TAC-TOE ", Style::new().fg(t.fg).bold()),
    ]);
    let game = Line::from(Span::styled(
        format!(" game #{} ", app.state.game_no),
        Style::new().fg(t.muted),
    ))
    .right_aligned();
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(t.grid))
        .title(title)
        .title(game)
        .style(Style::new().bg(t.panel));

    let web = match &app.web_url {
        Some(url) => {
            let n = app.state.web_clients;
            Line::from(vec![
                Span::styled(" ● ", Style::new().fg(t.accent)),
                Span::styled("web ", Style::new().fg(t.muted)),
                Span::styled(url.as_str(), Style::new().fg(t.fg).underlined()),
                Span::styled(
                    format!(
                        "  ·  {n} browser{} connected",
                        if n == 1 { "" } else { "s" }
                    ),
                    Style::new().fg(t.muted),
                ),
            ])
        }
        None => Line::from(Span::styled(
            " ○ web interface disabled",
            Style::new().fg(t.muted),
        )),
    };
    f.render_widget(Paragraph::new(web).block(block), area);
}

fn draw_board_panel(f: &mut Frame, app: &mut App, t: &Theme, area: Rect) {
    let block = panel(t, "Board");
    let inner = block.inner(area);
    f.render_widget(block, area);

    let [grid_area, status_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(inner);

    // Pick the largest cell size that fits.
    let (cw, ch) = SIZES
        .into_iter()
        .find(|&(w, h)| 3 * w + 4 <= grid_area.width && 3 * h + 4 <= grid_area.height)
        .unwrap_or(SIZES[2]);
    let grid = center(
        grid_area,
        Constraint::Length(3 * cw + 4),
        Constraint::Length(3 * ch + 4),
    );
    draw_grid(f.buffer_mut(), app, t, grid, cw, ch);
    draw_status(f, app, t, status_area);
}

fn draw_grid(buf: &mut Buffer, app: &mut App, t: &Theme, g: Rect, cw: u16, ch: u16) {
    let line_style = Style::new().fg(t.grid);
    // Grid lines: walk every position, draw where a row or column line falls.
    for dy in 0..g.height {
        for dx in 0..g.width {
            let on_v = dx % (cw + 1) == 0;
            let on_h = dy % (ch + 1) == 0;
            let (top, bottom) = (dy == 0, dy == g.height - 1);
            let (left, right) = (dx == 0, dx == g.width - 1);
            let sym = match (on_h, on_v) {
                (true, true) => match (top, bottom, left, right) {
                    (true, _, true, _) => "╭",
                    (true, _, _, true) => "╮",
                    (_, true, true, _) => "╰",
                    (_, true, _, true) => "╯",
                    (true, ..) => "┬",
                    (_, true, ..) => "┴",
                    (_, _, true, _) => "├",
                    (_, _, _, true) => "┤",
                    _ => "┼",
                },
                (true, false) => "─",
                (false, true) => "│",
                (false, false) => continue,
            };
            buf.set_string(g.x + dx, g.y + dy, sym, line_style);
        }
    }

    let s = &app.state;
    let win_line = match s.status {
        Status::Won { line, .. } => Some(line),
        _ => None,
    };
    let interactive = !s.status.is_over() && s.ai != Some(s.turn);
    let flash_on = (app.ticks / 4) % 2 == 0;
    let last = s.history.last().copied();

    for i in 0..9u16 {
        let idx = i as usize;
        let rect = Rect::new(
            g.x + 1 + (i % 3) * (cw + 1),
            g.y + 1 + (i / 3) * (ch + 1),
            cw,
            ch,
        );
        app.cell_rects[idx] = rect;

        let winning = win_line.is_some_and(|l| l.contains(&idx));
        let lit = winning && flash_on;
        let bg = if lit {
            t.win
        } else if idx == app.cursor && interactive {
            t.cursor
        } else {
            t.panel
        };
        buf.set_style(rect, Style::new().bg(bg));

        match s.board[idx] {
            Some(p) => {
                let fg = if lit { t.win_fg } else { player_color(t, p) };
                draw_glyph(buf, rect, p, Style::new().fg(fg).bold());
            }
            None if idx == app.cursor && interactive => {
                // Ghost preview of the mark you're about to place.
                draw_glyph(
                    buf,
                    rect,
                    s.turn,
                    Style::new().fg(t.muted).add_modifier(Modifier::DIM),
                );
            }
            None => {
                let label = (idx + 1).to_string();
                let x = rect.x + rect.width / 2;
                let y = rect.y + rect.height / 2;
                buf.set_string(
                    x,
                    y,
                    label,
                    Style::new().fg(t.muted).add_modifier(Modifier::DIM),
                );
            }
        }

        if last == Some(idx) && cw > 3 {
            buf.set_string(
                rect.right() - 1,
                rect.y,
                "•",
                Style::new().fg(t.accent).bg(bg),
            );
        }
    }
}

fn draw_glyph(buf: &mut Buffer, rect: Rect, p: Player, style: Style) {
    let art: &[&str] = match (rect.height, p) {
        (h, Player::X) if h >= 5 => &X_BIG,
        (h, Player::O) if h >= 5 => &O_BIG,
        (h, Player::X) if h >= 3 => &X_MID,
        (h, Player::O) if h >= 3 => &O_MID,
        (_, Player::X) => &["X"],
        (_, Player::O) => &["O"],
    };
    let w = art[0].chars().count() as u16;
    let x = rect.x + rect.width.saturating_sub(w) / 2;
    let y = rect.y + rect.height.saturating_sub(art.len() as u16) / 2;
    for (row, text) in art.iter().enumerate() {
        buf.set_string(x, y + row as u16, text, style);
    }
}

fn draw_status(f: &mut Frame, app: &App, t: &Theme, area: Rect) {
    let s = &app.state;
    let headline = match s.status {
        Status::Won { winner, .. } => {
            let who = if s.ai == Some(winner) { " (AI)" } else { "" };
            Line::from(vec![
                Span::styled(
                    format!("★ {winner} WINS{who} ★"),
                    Style::new().fg(t.win).bold(),
                ),
                Span::styled("   n = new game", Style::new().fg(t.muted)),
            ])
        }
        Status::Draw => Line::from(vec![
            Span::styled("Draw", Style::new().fg(t.fg).bold()),
            Span::styled("   n = new game", Style::new().fg(t.muted)),
        ]),
        Status::InProgress if s.ai == Some(s.turn) => {
            let spin = SPINNER[(app.ticks as usize) % SPINNER.len()];
            Line::from(vec![
                Span::styled(format!("{spin} "), Style::new().fg(t.accent)),
                Span::styled(
                    format!("AI ({})", s.turn),
                    Style::new().fg(player_color(t, s.turn)).bold(),
                ),
                Span::styled(" is thinking…", Style::new().fg(t.muted)),
            ])
        }
        Status::InProgress => Line::from(vec![
            Span::styled(
                s.turn.to_string(),
                Style::new().fg(player_color(t, s.turn)).bold(),
            ),
            Span::styled(" to move", Style::new().fg(t.fg)),
        ]),
    };
    let sub = match app.active_toast() {
        Some(toast) => Line::from(Span::styled(
            toast.text.as_str(),
            Style::new().fg(if toast.is_error { t.error } else { t.accent }),
        )),
        None => Line::from(Span::styled(
            "arrows/hjkl + enter · 1-9 · or click",
            Style::new().fg(t.muted),
        )),
    };
    f.render_widget(
        Paragraph::new(vec![Line::default(), headline, sub]).alignment(Alignment::Center),
        area,
    );
}

fn draw_sidebar(f: &mut Frame, app: &App, t: &Theme, area: Rect) {
    let [score, mode, history] = Layout::vertical([
        Constraint::Length(6),
        Constraint::Length(5),
        Constraint::Min(3),
    ])
    .areas(area);
    draw_score(f, app, t, score);
    draw_mode(f, app, t, mode);
    draw_history(f, app, t, history);
}

fn draw_score(f: &mut Frame, app: &App, t: &Theme, area: Rect) {
    let block = panel(t, "Score");
    let inner = block.inner(area);
    f.render_widget(block, area);
    let sc = app.state.score;

    let [labels, numbers, _, bar] = Layout::vertical([Constraint::Length(1); 4]).areas(inner);
    let cols = |r: Rect| Layout::horizontal([Constraint::Ratio(1, 3); 3]).areas::<3>(r);
    for ((label, value, color), (l, n)) in [
        ("X", sc.x, t.x),
        ("draws", sc.draws, t.muted),
        ("O", sc.o, t.o),
    ]
    .into_iter()
    .zip(cols(labels).into_iter().zip(cols(numbers)))
    {
        f.render_widget(
            Paragraph::new(Span::styled(label, Style::new().fg(t.muted)))
                .alignment(Alignment::Center),
            l,
        );
        f.render_widget(
            Paragraph::new(Span::styled(
                value.to_string(),
                Style::new().fg(color).bold(),
            ))
            .alignment(Alignment::Center),
            n,
        );
    }

    // Proportional bar: X | draws | O.
    let total = sc.x + sc.o + sc.draws;
    let w = u32::from(bar.width);
    let line = match (sc.x * w).checked_div(total) {
        None => Line::from(Span::styled(
            "░".repeat(bar.width as usize),
            Style::new().fg(t.grid),
        )),
        Some(xw) => {
            let xw = xw as usize;
            let ow = (sc.o * w / total) as usize;
            let dw = (bar.width as usize).saturating_sub(xw + ow);
            Line::from(vec![
                Span::styled("█".repeat(xw), Style::new().fg(t.x)),
                Span::styled("█".repeat(dw), Style::new().fg(t.grid)),
                Span::styled("█".repeat(ow), Style::new().fg(t.o)),
            ])
        }
    };
    f.render_widget(Paragraph::new(line), bar);
}

fn draw_mode(f: &mut Frame, app: &App, t: &Theme, area: Rect) {
    let block = panel(t, "Opponent").title_bottom(
        Line::from(Span::styled(" a to change ", Style::new().fg(t.muted))).right_aligned(),
    );
    let options = [
        (None, "Two humans"),
        (Some(Player::O), "AI plays O"),
        (Some(Player::X), "AI plays X"),
    ];
    let lines: Vec<Line> = options
        .into_iter()
        .map(|(v, label)| {
            if app.state.ai == v {
                Line::from(Span::styled(
                    format!(" ◉ {label}"),
                    Style::new().fg(t.accent).bold(),
                ))
            } else {
                Line::from(Span::styled(
                    format!(" ○ {label}"),
                    Style::new().fg(t.muted),
                ))
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_history(f: &mut Frame, app: &App, t: &Theme, area: Rect) {
    let block = panel(t, "Moves");
    let s = &app.state;
    if s.history.is_empty() {
        let p =
            Paragraph::new(Span::styled(" no moves yet", Style::new().fg(t.muted))).block(block);
        f.render_widget(p, area);
        return;
    }
    // X always starts, so even plies are X.
    let items: Vec<ListItem> = s
        .history
        .iter()
        .enumerate()
        .map(|(n, &cell)| {
            let p = if n % 2 == 0 { Player::X } else { Player::O };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{:>2}. ", n + 1), Style::new().fg(t.muted)),
                Span::styled(p.to_string(), Style::new().fg(player_color(t, p)).bold()),
                Span::styled(format!("  {}", CELL_NAMES[cell]), Style::new().fg(t.fg)),
            ]))
        })
        .collect();
    // Selecting the last item keeps the newest move scrolled into view.
    let mut state = ListState::default().with_selected(Some(items.len() - 1));
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::new().bg(t.cursor))
        .highlight_symbol("› ");
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_footer(f: &mut Frame, t: &Theme, area: Rect) {
    let keys = [
        ("←↑↓→", "move"),
        ("⏎", "play"),
        ("1-9", "cell"),
        ("n", "new"),
        ("a", "opponent"),
        ("t", "theme"),
        ("?", "help"),
        ("q", "quit"),
    ];
    let mut spans = vec![Span::raw(" ")];
    for (k, d) in keys {
        spans.push(Span::styled(
            format!(" {k} "),
            Style::new().fg(t.bg).bg(t.muted).bold(),
        ));
        spans.push(Span::styled(format!(" {d}  "), Style::new().fg(t.muted)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_help(f: &mut Frame, t: &Theme, area: Rect) {
    let popup = center(area, Constraint::Length(58), Constraint::Length(17));
    let key = |k: &str, d: &str| {
        Line::from(vec![
            Span::styled(format!("  {k:<14}"), Style::new().fg(t.accent).bold()),
            Span::styled(d.to_string(), Style::new().fg(t.fg)),
        ])
    };
    let lines = vec![
        Line::default(),
        key("arrows / hjkl", "move the cursor"),
        key("enter / space", "place your mark"),
        key("1 - 9", "place mark in that cell (1 = top-left)"),
        key("mouse", "hover to aim, click to play"),
        key("n", "new game (score is kept)"),
        key("a", "cycle opponent: humans / AI O / AI X"),
        key("t", "cycle colour theme"),
        key("q / esc", "quit"),
        Line::default(),
        Line::from(Span::styled(
            "  The browser and this terminal share one live game:",
            Style::new().fg(t.muted),
        )),
        Line::from(Span::styled(
            "  moves made in either show up instantly in both.",
            Style::new().fg(t.muted),
        )),
        Line::default(),
        Line::from(Span::styled(
            "press any key to close",
            Style::new().fg(t.muted),
        ))
        .centered(),
    ];
    f.render_widget(Clear, popup);
    f.render_widget(Paragraph::new(lines).block(panel(t, "Help")), popup);
}

fn center(area: Rect, w: Constraint, h: Constraint) -> Rect {
    let [area] = Layout::horizontal([w]).flex(Flex::Center).areas(area);
    let [area] = Layout::vertical([h]).flex(Flex::Center).areas(area);
    area
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::{Terminal, backend::TestBackend};
    use ttt_server::spawn_hub;

    use super::*;

    fn render(app: &mut App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer();
        (0..h)
            .map(|y| (0..w).map(|x| buf[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[tokio::test]
    async fn renders_full_layout_and_maps_cells() {
        let hub = spawn_hub(None, Duration::ZERO);
        for cell in [4, 0, 8] {
            hub.play(cell).await.unwrap();
        }
        let mut app = App::new(hub.clone(), Some("http://127.0.0.1:8787".into()));
        app.state = hub.snapshot();
        let screen = render(&mut app, 100, 32);
        println!("{screen}");
        assert!(screen.contains("TIC-TAC-TOE"));
        assert!(screen.contains("O to move"));
        assert!(screen.contains("bottom-right"), "history panel shows moves");
        // Every cell got a distinct, non-empty hit box for the mouse.
        assert!(app.cell_rects.iter().all(|r| r.area() > 0));
        assert!(!app.cell_rects[0].intersects(app.cell_rects[1]));
    }

    #[tokio::test]
    async fn small_terminal_degrades_gracefully() {
        let hub = spawn_hub(None, Duration::ZERO);
        let mut app = App::new(hub, None);
        let screen = render(&mut app, 50, 22);
        println!("{screen}");
        assert!(screen.contains("to move"), "board still usable without sidebar");
        assert!(render(&mut app, 20, 10).contains("too small"));
    }
}
