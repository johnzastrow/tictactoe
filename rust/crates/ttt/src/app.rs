//! TUI state and input handling. The TUI is just another hub client: it
//! renders `StateView` and sends intents, exactly like the web page.

use std::time::{Duration, Instant};

use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use futures_util::StreamExt;
use ratatui::{DefaultTerminal, layout::Rect};
use ttt_core::Player;
use ttt_server::{HubHandle, StateView};

use crate::{theme, ui};

const TICK: Duration = Duration::from_millis(100);
const TOAST_FOR: Duration = Duration::from_millis(2500);

pub struct Toast {
    pub text: String,
    pub is_error: bool,
    pub since: Instant,
}

pub struct App {
    hub: HubHandle,
    pub state: StateView,
    pub web_url: Option<String>,
    /// Keyboard/mouse-hover cursor, 0..9.
    pub cursor: usize,
    pub theme: usize,
    pub show_help: bool,
    pub toast: Option<Toast>,
    /// Advances every TICK; drives the spinner and win flash.
    pub ticks: u64,
    /// Filled in by the renderer so mouse clicks can be mapped to cells.
    pub cell_rects: [Rect; 9],
    quit: bool,
}

impl App {
    pub fn new(hub: HubHandle, web_url: Option<String>) -> Self {
        let state = hub.snapshot();
        Self {
            hub,
            state,
            web_url,
            cursor: 4,
            theme: theme::default_index(),
            show_help: false,
            toast: None,
            ticks: 0,
            cell_rects: [Rect::default(); 9],
            quit: false,
        }
    }

    pub fn theme(&self) -> &'static theme::Theme {
        &theme::THEMES[self.theme]
    }

    pub fn active_toast(&self) -> Option<&Toast> {
        self.toast
            .as_ref()
            .filter(|t| t.since.elapsed() < TOAST_FOR)
    }

    pub async fn run(mut self, terminal: &mut DefaultTerminal) -> std::io::Result<()> {
        let mut events = crossterm::event::EventStream::new();
        let mut updates = self.hub.subscribe();
        let mut tick = tokio::time::interval(TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        while !self.quit {
            terminal.draw(|f| ui::draw(f, &mut self))?;
            tokio::select! {
                ev = events.next() => match ev {
                    Some(Ok(ev)) => self.on_event(ev).await,
                    Some(Err(e)) => return Err(e),
                    None => break,
                },
                changed = updates.changed() => {
                    if changed.is_err() {
                        break; // hub gone
                    }
                    self.state = updates.borrow_and_update().clone();
                }
                _ = tick.tick() => self.ticks = self.ticks.wrapping_add(1),
            }
        }
        Ok(())
    }

    async fn on_event(&mut self, ev: Event) {
        match ev {
            // Windows reports both press and release; act on press only.
            Event::Key(k) if k.kind == KeyEventKind::Press => self.on_key(k).await,
            Event::Mouse(m) => self.on_mouse(m).await,
            _ => {}
        }
    }

    async fn on_key(&mut self, k: KeyEvent) {
        if self.show_help {
            // Any key closes help (but still allow quitting).
            self.show_help = false;
            if !matches!(k.code, KeyCode::Char('q')) {
                return;
            }
        }
        let (row, col) = (self.cursor / 3, self.cursor % 3);
        match k.code {
            KeyCode::Char('c') if k.modifiers.contains(KeyModifiers::CONTROL) => self.quit = true,
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Left | KeyCode::Char('h') => self.cursor = row * 3 + (col + 2) % 3,
            KeyCode::Right | KeyCode::Char('l') => self.cursor = row * 3 + (col + 1) % 3,
            KeyCode::Up | KeyCode::Char('k') => self.cursor = ((row + 2) % 3) * 3 + col,
            KeyCode::Down | KeyCode::Char('j') => self.cursor = ((row + 1) % 3) * 3 + col,
            KeyCode::Enter | KeyCode::Char(' ') => self.play(self.cursor).await,
            KeyCode::Char(c @ '1'..='9') => {
                let cell = c as usize - '1' as usize;
                self.cursor = cell;
                self.play(cell).await;
            }
            KeyCode::Char('n') => self.act(self.hub.new_game().await, "New game"),
            KeyCode::Char('a') => {
                // Cycle: humans -> AI plays O -> AI plays X -> humans.
                let next = match self.state.ai {
                    None => Some(Player::O),
                    Some(Player::O) => Some(Player::X),
                    Some(Player::X) => None,
                };
                let label = match next {
                    None => "Two humans".to_string(),
                    Some(p) => format!("AI plays {p}"),
                };
                self.act(self.hub.set_ai(next).await, &label);
            }
            KeyCode::Char('t') => {
                self.theme = (self.theme + 1) % theme::THEMES.len();
                self.info(format!("Theme: {}", self.theme().name));
            }
            KeyCode::Char('?') => self.show_help = true,
            _ => {}
        }
    }

    async fn on_mouse(&mut self, m: MouseEvent) {
        let hit = self
            .cell_rects
            .iter()
            .position(|r| r.contains((m.column, m.row).into()));
        match (m.kind, hit) {
            (MouseEventKind::Moved, Some(cell)) => self.cursor = cell,
            (MouseEventKind::Down(MouseButton::Left), Some(cell)) => {
                self.cursor = cell;
                self.play(cell).await;
            }
            _ => {}
        }
    }

    async fn play(&mut self, cell: usize) {
        if let Err(e) = self.hub.play(cell).await {
            self.error(e.to_string());
        }
    }

    fn act(&mut self, result: Result<(), ttt_server::HubError>, ok: &str) {
        match result {
            Ok(()) => self.info(ok.to_string()),
            Err(e) => self.error(e.to_string()),
        }
    }

    fn info(&mut self, text: String) {
        self.toast = Some(Toast {
            text,
            is_error: false,
            since: Instant::now(),
        });
    }

    fn error(&mut self, text: String) {
        self.toast = Some(Toast {
            text,
            is_error: true,
            since: Instant::now(),
        });
    }
}
