//! Pure tic-tac-toe rules and an unbeatable AI.
//!
//! This crate does no I/O. Every front end (TUI, web, tests) drives the game
//! through [`Game::play`], which is the single place moves are validated.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Cells are indexed 0..9, row-major:
/// ```text
///  0 | 1 | 2
///  3 | 4 | 5
///  6 | 7 | 8
/// ```
pub const CELLS: usize = 9;

/// Every line that wins the game.
pub const LINES: [[usize; 3]; 8] = [
    [0, 1, 2],
    [3, 4, 5],
    [6, 7, 8],
    [0, 3, 6],
    [1, 4, 7],
    [2, 5, 8],
    [0, 4, 8],
    [2, 4, 6],
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Player {
    X,
    O,
}

impl Player {
    pub fn other(self) -> Self {
        match self {
            Player::X => Player::O,
            Player::O => Player::X,
        }
    }
}

impl std::fmt::Display for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Player::X => "X",
            Player::O => "O",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Status {
    InProgress,
    Won { winner: Player, line: [usize; 3] },
    Draw,
}

impl Status {
    pub fn is_over(self) -> bool {
        !matches!(self, Status::InProgress)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum MoveError {
    #[error("cell {0} is out of range (0-8)")]
    OutOfRange(usize),
    #[error("cell {0} is already taken")]
    Occupied(usize),
    #[error("the game is already over")]
    GameOver,
}

pub type Board = [Option<Player>; CELLS];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Game {
    board: Board,
    turn: Player,
    status: Status,
    /// Cells in the order they were played.
    history: Vec<usize>,
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

impl Game {
    /// A fresh game; X always moves first.
    pub fn new() -> Self {
        Self {
            board: [None; CELLS],
            turn: Player::X,
            status: Status::InProgress,
            history: Vec::with_capacity(CELLS),
        }
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn turn(&self) -> Player {
        self.turn
    }

    pub fn status(&self) -> Status {
        self.status
    }

    pub fn history(&self) -> &[usize] {
        &self.history
    }

    pub fn legal_moves(&self) -> impl Iterator<Item = usize> + '_ {
        let open = !self.status.is_over();
        (0..CELLS).filter(move |&i| open && self.board[i].is_none())
    }

    /// Place the current player's mark. This is the only mutation path, so
    /// all validation lives here rather than in any UI.
    pub fn play(&mut self, cell: usize) -> Result<Status, MoveError> {
        if self.status.is_over() {
            return Err(MoveError::GameOver);
        }
        if cell >= CELLS {
            return Err(MoveError::OutOfRange(cell));
        }
        if self.board[cell].is_some() {
            return Err(MoveError::Occupied(cell));
        }
        self.board[cell] = Some(self.turn);
        self.history.push(cell);
        self.status = evaluate(&self.board);
        if !self.status.is_over() {
            self.turn = self.turn.other();
        }
        Ok(self.status)
    }
}

/// Work out the status of an arbitrary board.
pub fn evaluate(board: &Board) -> Status {
    for line in LINES {
        if let Some(p) = board[line[0]]
            && board[line[1]] == Some(p)
            && board[line[2]] == Some(p)
        {
            return Status::Won { winner: p, line };
        }
    }
    if board.iter().all(Option::is_some) {
        Status::Draw
    } else {
        Status::InProgress
    }
}

/// Best move for the player whose turn it is, using full minimax.
///
/// Scores prefer faster wins and slower losses, so the AI finishes a won
/// game promptly instead of toying with you. Returns `None` if the game is over.
pub fn best_move(game: &Game) -> Option<usize> {
    let me = game.turn();
    let mut board = *game.board();
    let mut best: Option<(i32, usize)> = None;
    for cell in game.legal_moves() {
        board[cell] = Some(me);
        let score = -negamax(&mut board, me.other(), 1);
        board[cell] = None;
        // Strict `>` keeps the lowest-index cell on ties: deterministic and testable.
        if best.is_none_or(|(s, _)| score > s) {
            best = Some((score, cell));
        }
    }
    best.map(|(_, cell)| cell)
}

/// Score from the perspective of `to_move`. The board is only 9 cells, so
/// exhaustive search (~550k nodes from empty) is fast enough without pruning.
fn negamax(board: &mut Board, to_move: Player, depth: i32) -> i32 {
    match evaluate(board) {
        // The previous mover just won, which is bad for `to_move`.
        Status::Won { .. } => return depth - 10,
        Status::Draw => return 0,
        Status::InProgress => {}
    }
    let mut best = i32::MIN;
    for cell in 0..CELLS {
        if board[cell].is_none() {
            board[cell] = Some(to_move);
            best = best.max(-negamax(board, to_move.other(), depth + 1));
            board[cell] = None;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play_all(cells: &[usize]) -> Game {
        let mut g = Game::new();
        for &c in cells {
            g.play(c).expect("legal move");
        }
        g
    }

    #[test]
    fn x_moves_first_and_turns_alternate() {
        let mut g = Game::new();
        assert_eq!(g.turn(), Player::X);
        g.play(4).unwrap();
        assert_eq!(g.turn(), Player::O);
        assert_eq!(g.board()[4], Some(Player::X));
    }

    #[test]
    fn rejects_bad_moves() {
        let mut g = Game::new();
        assert_eq!(g.play(9), Err(MoveError::OutOfRange(9)));
        g.play(0).unwrap();
        assert_eq!(g.play(0), Err(MoveError::Occupied(0)));
    }

    #[test]
    fn detects_win_with_line_and_blocks_further_moves() {
        // X: 0,1,2  O: 3,4
        let mut g = play_all(&[0, 3, 1, 4, 2]);
        assert_eq!(
            g.status(),
            Status::Won {
                winner: Player::X,
                line: [0, 1, 2]
            }
        );
        assert_eq!(g.turn(), Player::X, "turn stays with the winner");
        assert_eq!(g.play(8), Err(MoveError::GameOver));
        assert_eq!(g.legal_moves().count(), 0);
    }

    #[test]
    fn detects_draw() {
        // X O X / X O O / O X X
        let g = play_all(&[0, 1, 2, 4, 3, 5, 7, 6, 8]);
        assert_eq!(g.status(), Status::Draw);
    }

    #[test]
    fn ai_takes_immediate_win() {
        // X: 0,1  O: 3,4 — X to move should finish the top row.
        let g = play_all(&[0, 3, 1, 4]);
        assert_eq!(best_move(&g), Some(2));
    }

    #[test]
    fn ai_blocks_immediate_loss() {
        // X: 0,1  O: 4 — O to move must block at 2.
        let g = play_all(&[0, 4, 1]);
        assert_eq!(best_move(&g), Some(2));
    }

    /// Exhaustively play every possible opponent strategy against the AI,
    /// for both sides: the AI must never lose.
    #[test]
    fn ai_never_loses() {
        fn explore(g: &Game, ai: Player, games: &mut u32) {
            if g.status().is_over() {
                *games += 1;
                if let Status::Won { winner, .. } = g.status() {
                    assert_eq!(winner, ai, "AI lost: {:?}", g.history());
                }
                return;
            }
            if g.turn() == ai {
                let mut next = g.clone();
                next.play(best_move(g).unwrap()).unwrap();
                explore(&next, ai, games);
            } else {
                for cell in g.legal_moves() {
                    let mut next = g.clone();
                    next.play(cell).unwrap();
                    explore(&next, ai, games);
                }
            }
        }
        let mut games = 0;
        explore(&Game::new(), Player::X, &mut games);
        explore(&Game::new(), Player::O, &mut games);
        assert!(games > 100);
    }

    #[test]
    fn status_serializes_as_tagged_json() {
        let s = Status::Won {
            winner: Player::O,
            line: [2, 4, 6],
        };
        assert_eq!(
            serde_json::to_string(&s).unwrap(),
            r#"{"kind":"won","winner":"O","line":[2,4,6]}"#
        );
    }
}
