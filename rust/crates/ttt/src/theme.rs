//! Colour themes. Truecolor themes look best; "Classic" uses the 16 ANSI
//! colours so it renders correctly in terminals without 24-bit support
//! (e.g. macOS Terminal.app, older Windows conhost).

use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub bg: Color,
    pub panel: Color,
    pub fg: Color,
    pub muted: Color,
    pub grid: Color,
    pub x: Color,
    pub o: Color,
    pub cursor: Color,
    pub win: Color,
    pub win_fg: Color,
    pub accent: Color,
    pub error: Color,
}

pub const THEMES: [Theme; 3] = [
    Theme {
        name: "Midnight",
        bg: Color::Rgb(17, 20, 27),
        panel: Color::Rgb(26, 31, 42),
        fg: Color::Rgb(231, 234, 240),
        muted: Color::Rgb(110, 118, 135),
        grid: Color::Rgb(70, 80, 100),
        x: Color::Rgb(255, 138, 76),
        o: Color::Rgb(77, 171, 247),
        cursor: Color::Rgb(45, 53, 70),
        win: Color::Rgb(255, 212, 59),
        win_fg: Color::Rgb(17, 20, 27),
        accent: Color::Rgb(105, 219, 124),
        error: Color::Rgb(255, 107, 107),
    },
    Theme {
        name: "Paper",
        bg: Color::Rgb(246, 247, 251),
        panel: Color::Rgb(255, 255, 255),
        fg: Color::Rgb(29, 35, 48),
        muted: Color::Rgb(120, 128, 145),
        grid: Color::Rgb(190, 196, 210),
        x: Color::Rgb(217, 72, 15),
        o: Color::Rgb(25, 113, 194),
        cursor: Color::Rgb(226, 231, 241),
        win: Color::Rgb(250, 176, 5),
        win_fg: Color::Rgb(29, 35, 48),
        accent: Color::Rgb(43, 138, 62),
        error: Color::Rgb(201, 42, 42),
    },
    Theme {
        name: "Classic",
        bg: Color::Reset,
        panel: Color::Reset,
        fg: Color::Reset,
        muted: Color::DarkGray,
        grid: Color::Gray,
        x: Color::LightRed,
        o: Color::LightCyan,
        cursor: Color::DarkGray,
        win: Color::Yellow,
        win_fg: Color::Black,
        accent: Color::LightGreen,
        error: Color::Red,
    },
];

/// Start on a truecolor theme only if the terminal advertises support.
pub fn default_index() -> usize {
    let truecolor = std::env::var("COLORTERM")
        .is_ok_and(|v| v.eq_ignore_ascii_case("truecolor") || v.eq_ignore_ascii_case("24bit"));
    if truecolor { 0 } else { 2 }
}
