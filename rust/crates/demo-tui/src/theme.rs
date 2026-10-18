//! Themes (pt ui-spec §8, reused by demo ui-spec): palettes of named roles, mapped to truecolor when the
//! terminal supports it and to the nearest xterm-256 colour otherwise.

use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::style::{Color, Modifier, Style};

static TRUECOLOR: AtomicBool = AtomicBool::new(true);

/// Decide colour depth: `--colors 256|truecolor` wins, then `DEMO_COLOR`,
/// then what the terminal advertises.
pub fn detect_color_depth(forced: Option<&str>) {
    let truecolor = match forced {
        Some(v) => !v.starts_with("256"),
        None => truecolor_from_env(|k| std::env::var(k).ok()),
    };
    TRUECOLOR.store(truecolor, Ordering::Relaxed);
}

pub fn is_truecolor() -> bool {
    TRUECOLOR.load(Ordering::Relaxed)
}

pub fn truecolor_from_env(get: impl Fn(&str) -> Option<String>) -> bool {
    if let Some(v) = get("DEMO_COLOR") {
        return !v.starts_with("256");
    }
    let colorterm = get("COLORTERM").unwrap_or_default().to_lowercase();
    if colorterm == "truecolor" || colorterm == "24bit" {
        return true;
    }
    if get("WT_SESSION").is_some() {
        return true; // Windows Terminal
    }
    let prog = get("TERM_PROGRAM").unwrap_or_default();
    if matches!(
        prog.as_str(),
        "ghostty" | "iTerm.app" | "WezTerm" | "vscode" | "Hyper" | "rio"
    ) {
        return true;
    }
    let term = get("TERM").unwrap_or_default();
    term.contains("direct")
        || term.contains("kitty")
        || term.contains("ghostty")
        || term.contains("alacritty")
}

/// Nearest xterm-256 index (6×6×6 cube or the 24-step grey ramp).
pub fn to_xterm256(r: u8, g: u8, b: u8) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let idx = |v: u8| -> usize {
        LEVELS
            .iter()
            .enumerate()
            .min_by_key(|(_, l)| (**l as i32 - v as i32).abs())
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    let (ri, gi, bi) = (idx(r), idx(g), idx(b));
    let cube = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    let dist = |a: (u8, u8, u8)| {
        let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
        d(a.0, r) + d(a.1, g) + d(a.2, b)
    };
    let avg = (r as u32 + g as u32 + b as u32) / 3;
    let gi2 = if avg < 8 { 0 } else { ((avg - 8) / 10).min(23) };
    let grey = (8 + gi2 * 10) as u8;
    if dist((grey, grey, grey)) < dist(cube) {
        232 + gi2 as u8
    } else {
        16 + (36 * ri + 6 * gi + bi) as u8
    }
}

/// A colour from a hex triple, honouring the colour depth.
pub fn rgb(hex: u32) -> Color {
    let (r, g, b) = ((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
    if is_truecolor() {
        Color::Rgb(r, g, b)
    } else {
        Color::Indexed(to_xterm256(r, g, b))
    }
}

/// `#4f9dde` → Color.
pub fn parse_hex(s: &str) -> Option<Color> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    u32::from_str_radix(h, 16).ok().map(rgb)
}

pub struct Palette {
    pub name: &'static str,
    pub bg: u32,
    pub panel: u32,
    pub fg: u32,
    pub muted: u32,
    pub accent: u32,
    pub accent2: u32,
    pub border: u32,
    pub sel_bg: u32,
    pub cur_bg: u32,
    pub zebra: u32,
    pub header_bg: u32,
    pub ok: u32,
    pub warn: u32,
    pub err: u32,
    pub info: u32,
    pub flash: u32,
}

pub const PALETTES: [Palette; 4] = [
    Palette {
        name: "Midnight",
        bg: 0x11151c,
        panel: 0x1a2030,
        fg: 0xd8dee9,
        muted: 0x6b7489,
        accent: 0x5fb3f9,
        accent2: 0xc792ea,
        border: 0x2e3850,
        sel_bg: 0x24334f,
        cur_bg: 0x2f5e9e,
        zebra: 0x151a24,
        header_bg: 0x222a3d,
        ok: 0x7fd18b,
        warn: 0xf2c55c,
        err: 0xf26d6d,
        info: 0x6cc4f0,
        flash: 0x4a3f12,
    },
    Palette {
        name: "Daylight",
        bg: 0xfbfbf8,
        panel: 0xeef0f3,
        fg: 0x24292f,
        muted: 0x8a929c,
        accent: 0x0969da,
        accent2: 0x8250df,
        border: 0xc8ced6,
        sel_bg: 0xdbe9fb,
        cur_bg: 0x9cc4f5,
        zebra: 0xf2f4f6,
        header_bg: 0xe2e6ea,
        ok: 0x1a7f37,
        warn: 0xb36b00,
        err: 0xcf222e,
        info: 0x0a6ecf,
        flash: 0xfff1b8,
    },
    Palette {
        name: "Solarized Dark",
        bg: 0x002b36,
        panel: 0x073642,
        fg: 0x93a1a1,
        muted: 0x586e75,
        accent: 0x268bd2,
        accent2: 0x6c71c4,
        border: 0x0e4a58,
        sel_bg: 0x0b4553,
        cur_bg: 0x1d6a7d,
        zebra: 0x01313d,
        header_bg: 0x094050,
        ok: 0x859900,
        warn: 0xb58900,
        err: 0xdc322f,
        info: 0x2aa198,
        flash: 0x3d3a00,
    },
    Palette {
        name: "Gruvbox",
        bg: 0x282828,
        panel: 0x32302f,
        fg: 0xebdbb2,
        muted: 0x928374,
        accent: 0x83a598,
        accent2: 0xd3869b,
        border: 0x504945,
        sel_bg: 0x3c3836,
        cur_bg: 0x665c54,
        zebra: 0x2c2b2a,
        header_bg: 0x3c3836,
        ok: 0xb8bb26,
        warn: 0xfabd2f,
        err: 0xfb4934,
        info: 0x83a598,
        flash: 0x4a3f12,
    },
];

pub fn theme_index(name: &str) -> Option<usize> {
    let n = name.to_lowercase().replace(['-', '_'], " ");
    PALETTES
        .iter()
        .position(|p| p.name.to_lowercase() == n || p.name.to_lowercase().starts_with(&n))
}

/// Resolved colours for the current palette and colour depth.
#[derive(Clone, Debug)]
pub struct Theme {
    pub name: &'static str,
    pub bg: Color,
    pub panel: Color,
    pub fg: Color,
    pub muted: Color,
    pub accent: Color,
    pub accent2: Color,
    pub border: Color,
    pub sel_bg: Color,
    pub cur_bg: Color,
    pub zebra: Color,
    pub header_bg: Color,
    pub ok: Color,
    pub warn: Color,
    pub err: Color,
    pub info: Color,
    pub flash: Color,
}

impl Theme {
    pub fn new(idx: usize) -> Theme {
        let p = &PALETTES[idx % PALETTES.len()];
        Theme {
            name: p.name,
            bg: rgb(p.bg),
            panel: rgb(p.panel),
            fg: rgb(p.fg),
            muted: rgb(p.muted),
            accent: rgb(p.accent),
            accent2: rgb(p.accent2),
            border: rgb(p.border),
            sel_bg: rgb(p.sel_bg),
            cur_bg: rgb(p.cur_bg),
            zebra: rgb(p.zebra),
            header_bg: rgb(p.header_bg),
            ok: rgb(p.ok),
            warn: rgb(p.warn),
            err: rgb(p.err),
            info: rgb(p.info),
            flash: rgb(p.flash),
        }
    }

    pub fn base(&self) -> Style {
        Style::new().fg(self.fg).bg(self.bg)
    }
    pub fn panel(&self) -> Style {
        Style::new().fg(self.fg).bg(self.panel)
    }
    pub fn key(&self) -> Style {
        Style::new()
            .fg(self.bg)
            .bg(self.accent)
            .add_modifier(Modifier::BOLD)
    }
    /// A `#rrggbb` colour from the dataset or a peer, or `fallback`.
    pub fn hex(&self, s: &str, fallback: Color) -> Color {
        parse_hex(s).unwrap_or(fallback)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xterm_mapping() {
        assert_eq!(to_xterm256(0, 0, 0), 16);
        assert_eq!(to_xterm256(255, 255, 255), 231);
        assert_eq!(to_xterm256(255, 0, 0), 196);
        assert_eq!(to_xterm256(128, 128, 128), 244);
        assert!(
            (232..=255).contains(&to_xterm256(0x11, 0x15, 0x1c))
                || to_xterm256(0x11, 0x15, 0x1c) == 16
        );
    }

    #[test]
    fn env_detection() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| {
                pairs
                    .iter()
                    .find(|(n, _)| *n == k)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert!(truecolor_from_env(env(&[("COLORTERM", "truecolor")])));
        assert!(truecolor_from_env(env(&[("WT_SESSION", "x")])));
        assert!(!truecolor_from_env(env(&[
            ("TERM_PROGRAM", "Apple_Terminal"),
            ("TERM", "xterm-256color")
        ])));
        assert!(!truecolor_from_env(env(&[
            ("COLORTERM", "truecolor"),
            ("DEMO_COLOR", "256")
        ])));
        assert_eq!(theme_index("gruvbox"), Some(3));
        assert_eq!(theme_index("solarized"), Some(2));
    }
}
