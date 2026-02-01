use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChessPosition {
    pub fen: String,
}

impl ChessPosition {
    pub fn new(fen: impl Into<String>) -> Self {
        Self { fen: fen.into() }
    }

    pub fn starting() -> Self {
        Self::new("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1")
    }

    pub fn validate(&self) -> bool {
        let parts: Vec<&str> = self.fen.split_whitespace().collect();
        if parts.len() < 4 {
            return false;
        }

        let ranks: Vec<&str> = parts[0].split('/').collect();
        if ranks.len() != 8 {
            return false;
        }

        for rank in ranks {
            let mut count = 0;
            for c in rank.chars() {
                if c.is_ascii_digit() {
                    count += c.to_digit(10).unwrap_or(0);
                } else if "pnbrqkPNBRQK".contains(c) {
                    count += 1;
                } else {
                    return false;
                }
            }
            if count != 8 {
                return false;
            }
        }

        matches!(parts[1], "w" | "b")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Move {
    pub from: String,
    pub to: String,
    pub promotion: Option<char>,
}

impl Move {
    pub fn new(from: impl Into<String>, to: impl Into<String>) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            promotion: None,
        }
    }

    pub fn with_promotion(mut self, piece: char) -> Self {
        self.promotion = Some(piece);
        self
    }

    pub fn to_uci(&self) -> String {
        match self.promotion {
            Some(p) => format!("{}{}{}", self.from, self.to, p),
            None => format!("{}{}", self.from, self.to),
        }
    }

    pub fn from_uci(uci: &str) -> Option<Self> {
        if uci.len() < 4 {
            return None;
        }

        let from = uci[0..2].to_string();
        let to = uci[2..4].to_string();
        let promotion = uci.chars().nth(4);

        Some(Self {
            from,
            to,
            promotion,
        })
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Color {
    White,
    Black,
}

impl Color {
    pub fn from_fen(c: char) -> Option<Self> {
        match c {
            'w' => Some(Color::White),
            'b' => Some(Color::Black),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_starting_position() {
        let pos = ChessPosition::starting();
        assert!(pos.validate());
        assert!(pos.fen.contains("rnbqkbnr"));
    }

    #[test]
    fn test_valid_fen() {
        let valid_fens = [
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1",
            "8/8/8/8/8/8/8/8 w - - 0 1",
            "r3k2r/pppppppp/8/8/8/8/PPPPPPPP/R3K2R w KQkq - 0 1",
        ];

        for fen in valid_fens {
            let pos = ChessPosition::new(fen);
            assert!(pos.validate(), "FEN should be valid: {}", fen);
        }
    }

    #[test]
    fn test_invalid_fen() {
        let invalid_fens = [
            "",
            "invalid",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP w KQkq - 0 1",
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR x KQkq - 0 1",
            "rnbqkbnr/ppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        ];

        for fen in invalid_fens {
            let pos = ChessPosition::new(fen);
            assert!(!pos.validate(), "FEN should be invalid: {}", fen);
        }
    }

    #[test]
    fn test_move_creation() {
        let mv = Move::new("e2", "e4");
        assert_eq!(mv.from, "e2");
        assert_eq!(mv.to, "e4");
        assert_eq!(mv.promotion, None);
    }

    #[test]
    fn test_move_with_promotion() {
        let mv = Move::new("e7", "e8").with_promotion('q');
        assert_eq!(mv.promotion, Some('q'));
    }

    #[test]
    fn test_move_to_uci() {
        let mv = Move::new("e2", "e4");
        assert_eq!(mv.to_uci(), "e2e4");

        let mv_promo = Move::new("e7", "e8").with_promotion('q');
        assert_eq!(mv_promo.to_uci(), "e7e8q");
    }

    #[test]
    fn test_move_from_uci() {
        let mv = Move::from_uci("e2e4").unwrap();
        assert_eq!(mv.from, "e2");
        assert_eq!(mv.to, "e4");
        assert_eq!(mv.promotion, None);

        let mv_promo = Move::from_uci("e7e8q").unwrap();
        assert_eq!(mv_promo.promotion, Some('q'));

        assert!(Move::from_uci("e2").is_none());
        assert!(Move::from_uci("").is_none());
    }

    #[test]
    fn test_color_from_fen() {
        assert_eq!(Color::from_fen('w'), Some(Color::White));
        assert_eq!(Color::from_fen('b'), Some(Color::Black));
        assert_eq!(Color::from_fen('x'), None);
    }
}
