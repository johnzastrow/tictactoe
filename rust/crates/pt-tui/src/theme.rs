//! Themes (ui-spec §8): palettes of named roles, mapped to truecolor when the
//! terminal supports it and to the nearest xterm-256 colour otherwise.

use std::sync::atomic::{AtomicBool, Ordering};

use pt_core::grid::Style as CellStyle;
use pt_core::model::{Health, Priority, ProjectStatus, Role, TaskStatus};
use ratatui::style::{Color, Modifier, Style};

static TRUECOLOR: AtomicBool = AtomicBool::new(true);

/// Decide colour depth: `--colors 256|truecolor` wins, then `PT_COLOR`,
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
    if let Some(v) = get("PT_COLOR") {
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
    if matches!(prog.as_str(), "ghostty" | "iTerm.app" | "WezTerm" | "vscode" | "Hyper" | "rio") {
        return true;
    }
    let term = get("TERM").unwrap_or_default();
    term.contains("direct") || term.contains("kitty") || term.contains("ghostty") || term.contains("alacritty")
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
    if is_truecolor() { Color::Rgb(r, g, b) } else { Color::Indexed(to_xterm256(r, g, b)) }
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
    pub users: [u32; 8],
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
        users: [0xff8a65, 0x4dd0e1, 0xba68c8, 0xaed581, 0xffd54f, 0xf06292, 0x7986cb, 0x4db6ac],
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
        users: [0xd9480f, 0x0b7285, 0x862e9c, 0x2b8a3e, 0xe67700, 0xc2255c, 0x364fc7, 0x087f5b],
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
        users: [0xcb4b16, 0x2aa198, 0xd33682, 0x859900, 0xb58900, 0x6c71c4, 0x268bd2, 0xeee8d5],
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
        users: [0xfe8019, 0x8ec07c, 0xd3869b, 0xb8bb26, 0xfabd2f, 0xfb4934, 0x83a598, 0xebdbb2],
    },
];

pub fn theme_index(name: &str) -> Option<usize> {
    let n = name.to_lowercase().replace(['-', '_'], " ");
    PALETTES.iter().position(|p| p.name.to_lowercase() == n || p.name.to_lowercase().starts_with(&n))
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
    pub users: [Color; 8],
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
            users: p.users.map(rgb),
        }
    }

    pub fn base(&self) -> Style {
        Style::new().fg(self.fg).bg(self.bg)
    }
    pub fn panel(&self) -> Style {
        Style::new().fg(self.fg).bg(self.panel)
    }
    pub fn muted(&self) -> Style {
        Style::new().fg(self.muted)
    }
    pub fn key(&self) -> Style {
        Style::new().fg(self.bg).bg(self.accent).add_modifier(Modifier::BOLD)
    }
    pub fn user(&self, id: i64) -> Color {
        self.users[(id.unsigned_abs() as usize) % self.users.len()]
    }

    pub fn project_status(&self, s: ProjectStatus) -> Style {
        match s {
            ProjectStatus::Planning => Style::new().fg(self.info),
            ProjectStatus::Active => Style::new().fg(self.ok),
            ProjectStatus::OnHold => Style::new().fg(self.warn),
            ProjectStatus::Done => Style::new().fg(self.muted),
            ProjectStatus::Cancelled => Style::new().fg(self.muted).add_modifier(Modifier::CROSSED_OUT),
        }
    }
    pub fn task_status_color(&self, s: TaskStatus) -> Color {
        match s {
            TaskStatus::Todo => self.muted,
            TaskStatus::InProgress => self.info,
            TaskStatus::Blocked => self.err,
            TaskStatus::Review => self.accent2,
            TaskStatus::Done => self.ok,
        }
    }
    pub fn priority(&self, p: Priority) -> Style {
        match p {
            Priority::Low => Style::new().fg(self.muted),
            Priority::Medium => Style::new().fg(self.fg),
            Priority::High => Style::new().fg(self.warn),
            Priority::Critical => Style::new().fg(self.err).add_modifier(Modifier::BOLD),
        }
    }
    pub fn health_color(&self, h: Health) -> Color {
        match h {
            Health::Green => self.ok,
            Health::Amber => self.warn,
            Health::Red => self.err,
        }
    }
    pub fn role(&self, r: Role) -> Style {
        match r {
            Role::Admin => Style::new().fg(self.accent2).add_modifier(Modifier::BOLD),
            Role::Member => Style::new().fg(self.fg),
            Role::Viewer => Style::new().fg(self.muted),
        }
    }

    /// Text style for a grid cell hint (bars are drawn separately).
    pub fn cell(&self, s: &CellStyle) -> Style {
        match s {
            CellStyle::Plain | CellStyle::Bar { .. } => Style::new(),
            CellStyle::Muted => Style::new().fg(self.muted),
            CellStyle::Strong => Style::new().add_modifier(Modifier::BOLD),
            CellStyle::ProjectStatus(p) => self.project_status(*p),
            CellStyle::TaskStatus(t) => Style::new().fg(self.task_status_color(*t)),
            CellStyle::Priority(p) => self.priority(*p),
            CellStyle::Health(h) => Style::new().fg(self.health_color(*h)),
            CellStyle::Role(r) => self.role(*r),
            CellStyle::Overdue | CellStyle::Bad => Style::new().fg(self.err),
            CellStyle::DueSoon => Style::new().fg(self.warn),
            CellStyle::Good => Style::new().fg(self.ok),
            CellStyle::Tags => Style::new().fg(self.accent2),
            CellStyle::Color(hex) => parse_hex(hex).map_or(Style::new(), |c| Style::new().fg(c)),
            CellStyle::User(id) => Style::new().fg(self.user(*id)),
            CellStyle::Struck => Style::new().fg(self.muted).add_modifier(Modifier::CROSSED_OUT),
        }
    }

    /// Prefix glyph for decorated values (`● green`, `◆ critical`).
    pub fn glyph(s: &CellStyle) -> Option<&'static str> {
        match s {
            CellStyle::Health(_) => Some("● "),
            CellStyle::Priority(Priority::Critical) => Some("◆ "),
            CellStyle::Priority(Priority::High) => Some("▲ "),
            CellStyle::Priority(Priority::Low) => Some("▽ "),
            CellStyle::Priority(Priority::Medium) => Some("· "),
            CellStyle::TaskStatus(TaskStatus::Done) | CellStyle::ProjectStatus(ProjectStatus::Done) => Some("✓ "),
            CellStyle::TaskStatus(TaskStatus::Blocked) => Some("■ "),
            CellStyle::TaskStatus(TaskStatus::InProgress) | CellStyle::ProjectStatus(ProjectStatus::Active) => Some("▶ "),
            CellStyle::TaskStatus(TaskStatus::Review) => Some("◎ "),
            CellStyle::TaskStatus(TaskStatus::Todo) | CellStyle::ProjectStatus(ProjectStatus::Planning) => Some("○ "),
            CellStyle::ProjectStatus(ProjectStatus::OnHold) => Some("‖ "),
            CellStyle::ProjectStatus(ProjectStatus::Cancelled) => Some("✗ "),
            _ => None,
        }
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
        assert!((232..=255).contains(&to_xterm256(0x11, 0x15, 0x1c)) || to_xterm256(0x11, 0x15, 0x1c) == 16);
    }

    #[test]
    fn env_detection() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |k: &str| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
        };
        assert!(truecolor_from_env(env(&[("COLORTERM", "truecolor")])));
        assert!(truecolor_from_env(env(&[("WT_SESSION", "x")])));
        assert!(!truecolor_from_env(env(&[("TERM_PROGRAM", "Apple_Terminal"), ("TERM", "xterm-256color")])));
        assert!(!truecolor_from_env(env(&[("COLORTERM", "truecolor"), ("PT_COLOR", "256")])));
        assert_eq!(theme_index("gruvbox"), Some(3));
        assert_eq!(theme_index("solarized"), Some(2));
    }
}
