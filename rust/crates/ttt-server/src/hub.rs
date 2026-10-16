//! The hub is the single owner of game state.
//!
//! Every front end (TUI, each WebSocket) sends [`Command`]s over one mpsc
//! channel; the hub applies them in order and publishes a fresh
//! [`StateView`] on a `watch` channel. Because only this task mutates the
//! game, there are no locks and no races between the TUI and the browser.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, watch};
use ttt_core::{Board, Game, MoveError, Player, Status, best_move};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Score {
    pub x: u32,
    pub o: u32,
    pub draws: u32,
}

/// Everything a front end needs to render. This is also the wire format.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateView {
    pub board: Board,
    pub turn: Player,
    pub status: Status,
    pub history: Vec<usize>,
    pub score: Score,
    pub ai: Option<Player>,
    pub web_clients: u32,
    pub game_no: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HubError {
    #[error(transparent)]
    Move(#[from] MoveError),
    #[error("it's the AI's turn")]
    AiTurn,
    #[error("game server has stopped")]
    Closed,
}

enum Command {
    Move {
        cell: usize,
        reply: oneshot::Sender<Result<(), HubError>>,
    },
    NewGame,
    SetAi(Option<Player>),
    ClientJoined,
    ClientLeft,
    /// Scheduled by the hub itself; ignored if the game moved on meanwhile.
    AiTurn {
        game_no: u32,
        ply: usize,
    },
}

/// Cheap, cloneable handle used by every front end.
#[derive(Clone)]
pub struct HubHandle {
    tx: mpsc::Sender<Command>,
    state: watch::Receiver<StateView>,
}

impl HubHandle {
    pub async fn play(&self, cell: usize) -> Result<(), HubError> {
        let (reply, rx) = oneshot::channel();
        self.send(Command::Move { cell, reply }).await?;
        rx.await.map_err(|_| HubError::Closed)?
    }

    pub async fn new_game(&self) -> Result<(), HubError> {
        self.send(Command::NewGame).await
    }

    pub async fn set_ai(&self, player: Option<Player>) -> Result<(), HubError> {
        self.send(Command::SetAi(player)).await
    }

    /// A receiver that wakes whenever the state changes.
    pub fn subscribe(&self) -> watch::Receiver<StateView> {
        self.state.clone()
    }

    pub fn snapshot(&self) -> StateView {
        self.state.borrow().clone()
    }

    /// Register a web client; the returned guard unregisters it on drop, so
    /// the count stays right even if the connection task panics or errors out.
    pub(crate) async fn join(&self) -> ClientGuard {
        let _ = self.tx.send(Command::ClientJoined).await;
        ClientGuard {
            tx: self.tx.clone(),
        }
    }

    async fn send(&self, cmd: Command) -> Result<(), HubError> {
        self.tx.send(cmd).await.map_err(|_| HubError::Closed)
    }
}

pub(crate) struct ClientGuard {
    tx: mpsc::Sender<Command>,
}

impl Drop for ClientGuard {
    fn drop(&mut self) {
        // try_send: Drop can't await. The queue is large, so this only fails
        // if the hub is already gone, in which case nobody cares.
        let _ = self.tx.try_send(Command::ClientLeft);
    }
}

struct Hub {
    game: Game,
    score: Score,
    ai: Option<Player>,
    web_clients: u32,
    game_no: u32,
    ai_delay: Duration,
    tx: mpsc::Sender<Command>,
    state: watch::Sender<StateView>,
}

/// Start the hub task. `ai_delay` is a cosmetic pause before the AI moves.
pub fn spawn_hub(ai: Option<Player>, ai_delay: Duration) -> HubHandle {
    let (tx, mut rx) = mpsc::channel(256);
    let game = Game::new();
    let initial = StateView {
        board: *game.board(),
        turn: game.turn(),
        status: game.status(),
        history: Vec::new(),
        score: Score::default(),
        ai,
        web_clients: 0,
        game_no: 1,
    };
    let (state_tx, state_rx) = watch::channel(initial);
    let mut hub = Hub {
        game,
        score: Score::default(),
        ai,
        web_clients: 0,
        game_no: 1,
        ai_delay,
        tx: tx.clone(),
        state: state_tx,
    };
    tokio::spawn(async move {
        hub.schedule_ai_if_needed();
        while let Some(cmd) = rx.recv().await {
            hub.handle(cmd);
        }
    });
    HubHandle {
        tx,
        state: state_rx,
    }
}

impl Hub {
    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Move { cell, reply } => {
                let result = self.human_move(cell);
                let _ = reply.send(result);
            }
            Command::NewGame => {
                self.game = Game::new();
                self.game_no += 1;
                self.schedule_ai_if_needed();
            }
            Command::SetAi(player) => {
                self.ai = player;
                self.schedule_ai_if_needed();
            }
            Command::ClientJoined => self.web_clients += 1,
            Command::ClientLeft => self.web_clients = self.web_clients.saturating_sub(1),
            Command::AiTurn { game_no, ply } => {
                // Stale if a new game started or someone changed settings.
                let current = game_no == self.game_no
                    && ply == self.game.history().len()
                    && self.ai == Some(self.game.turn());
                if current && let Some(cell) = best_move(&self.game) {
                    self.apply(cell).expect("AI picks a legal move");
                }
            }
        }
        self.publish();
    }

    fn human_move(&mut self, cell: usize) -> Result<(), HubError> {
        if !self.game.status().is_over() && self.ai == Some(self.game.turn()) {
            return Err(HubError::AiTurn);
        }
        self.apply(cell)?;
        self.schedule_ai_if_needed();
        Ok(())
    }

    fn apply(&mut self, cell: usize) -> Result<(), MoveError> {
        match self.game.play(cell)? {
            Status::Won {
                winner: Player::X, ..
            } => self.score.x += 1,
            Status::Won {
                winner: Player::O, ..
            } => self.score.o += 1,
            Status::Draw => self.score.draws += 1,
            Status::InProgress => {}
        }
        Ok(())
    }

    fn schedule_ai_if_needed(&self) {
        if self.game.status().is_over() || self.ai != Some(self.game.turn()) {
            return;
        }
        let tx = self.tx.clone();
        let delay = self.ai_delay;
        let cmd = Command::AiTurn {
            game_no: self.game_no,
            ply: self.game.history().len(),
        };
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let _ = tx.send(cmd).await;
        });
    }

    fn publish(&self) {
        let view = StateView {
            board: *self.game.board(),
            turn: self.game.turn(),
            status: self.game.status(),
            history: self.game.history().to_vec(),
            score: self.score,
            ai: self.ai,
            web_clients: self.web_clients,
            game_no: self.game_no,
        };
        // send_if_modified avoids waking every client for no-op commands.
        self.state.send_if_modified(|cur| {
            if *cur == view {
                false
            } else {
                *cur = view;
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn moves_update_state_and_score() {
        let hub = spawn_hub(None, Duration::ZERO);
        for cell in [0, 3, 1, 4, 2] {
            hub.play(cell).await.unwrap();
        }
        let s = hub.snapshot();
        assert!(matches!(
            s.status,
            Status::Won {
                winner: Player::X,
                ..
            }
        ));
        assert_eq!(s.score.x, 1);
        assert_eq!(hub.play(8).await, Err(HubError::Move(MoveError::GameOver)));
        hub.new_game().await.unwrap();
        hub.play(4).await.unwrap(); // acts as a barrier: new_game was processed
        let s = hub.snapshot();
        assert_eq!(s.game_no, 2);
        assert_eq!(s.score.x, 1, "score survives a new game");
    }

    #[tokio::test]
    async fn ai_replies_and_blocks_human_on_its_turn() {
        let hub = spawn_hub(Some(Player::O), Duration::from_millis(50));
        let mut rx = hub.subscribe();
        hub.play(0).await.unwrap();
        assert_eq!(hub.play(1).await, Err(HubError::AiTurn));
        // Wait for the AI's reply to show up.
        let s = rx.wait_for(|s| s.history.len() == 2).await.unwrap().clone();
        assert_eq!(s.turn, Player::X);
        assert_eq!(
            s.board[4],
            Some(Player::O),
            "perfect O answers a corner with the centre"
        );
    }
}
