//! Genome representation for genetic programming.
//!
//! A genome consists of rules (condition-action pairs) and a constant pool.
//! Rules are evaluated in priority order during each turn, with the first
//! matching condition triggering its associated action.

// Genome uses intentional casts for random generation
#![allow(clippy::cast_possible_truncation, clippy::match_same_arms)]

use rand::Rng;
use serde::{Deserialize, Serialize};

/// Maximum number of rules per genome.
pub const MAX_RULES: usize = 64;

/// Number of constants in the constant pool.
pub const NUM_CONSTANTS: usize = 16;

/// Number of registers for in-turn state.
pub(crate) const NUM_REGISTERS: usize = 8;

/// Maximum iterations for Repeat action.
pub(crate) const MAX_REPEAT: u32 = 16;

/// A complete genome representing a bot's strategy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Genome {
    /// Rules evaluated in priority order.
    pub rules: Vec<Rule>,
    /// Constant pool for parameterizing expressions.
    pub constants: [i32; NUM_CONSTANTS],
}

impl Default for Genome {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            constants: [0; NUM_CONSTANTS],
        }
    }
}

impl Genome {
    /// Create a random genome.
    #[must_use]
    pub fn random<R: Rng>(rng: &mut R, num_rules: usize) -> Self {
        let num_rules = num_rules.min(MAX_RULES);
        let mut rules = Vec::with_capacity(num_rules);

        for i in 0..num_rules {
            rules.push(Rule::random(rng, i as u8));
        }

        let mut constants = [0i32; NUM_CONSTANTS];
        for c in &mut constants {
            *c = rng.gen_range(-100..=100);
        }

        Self { rules, constants }
    }

    /// Get the number of rules in this genome.
    #[must_use]
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}

/// A single rule: IF condition THEN action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// Condition expression (evaluates to true/false).
    pub condition: Expr,
    /// Action to take when condition is true.
    pub action: Action,
    /// Priority for rule ordering (lower = higher priority).
    pub priority: u8,
}

impl Rule {
    /// Create a random rule.
    #[must_use]
    pub fn random<R: Rng>(rng: &mut R, priority: u8) -> Self {
        Self {
            condition: Expr::random_condition(rng, 3),
            action: Action::random(rng),
            priority,
        }
    }
}

/// Expression tree for conditions and value computations.
///
/// Expressions are evaluated as a stack machine: terminals push values,
/// operators pop operands and push results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Expr {
    // === Terminals (leaf nodes) ===
    /// Reference to constant pool.
    Const(u8),
    /// Current turn number.
    Turn,
    /// Player's food balance.
    MyFood,
    /// Player's total population.
    MyPop,
    /// Player's total army.
    MyArmy,
    /// Player's territory count.
    MyTerritory,
    /// Map width.
    MapWidth,
    /// Map height.
    MapHeight,
    /// Current iteration X coordinate.
    IterX,
    /// Current iteration Y coordinate.
    IterY,
    /// Read from register (0-7).
    Reg(u8),

    // === Tile queries ===
    /// Tile type at reference (0=City, 1=Desert, 2=Mountain, 255=Fog).
    TileType(TileRef),
    /// Tile owner at reference (0=neutral, 1-8=player, 255=fog).
    TileOwner(TileRef),
    /// Army count at tile reference.
    TileArmy(TileRef),

    // === Binary operators ===
    /// Addition.
    Add(Box<Expr>, Box<Expr>),
    /// Subtraction.
    Sub(Box<Expr>, Box<Expr>),
    /// Multiplication.
    Mul(Box<Expr>, Box<Expr>),
    /// Division (protected: div by 0 returns 0).
    Div(Box<Expr>, Box<Expr>),
    /// Modulo (protected: mod by 0 returns 0).
    Mod(Box<Expr>, Box<Expr>),
    /// Greater than.
    Gt(Box<Expr>, Box<Expr>),
    /// Less than.
    Lt(Box<Expr>, Box<Expr>),
    /// Equal.
    Eq(Box<Expr>, Box<Expr>),
    /// Logical AND.
    And(Box<Expr>, Box<Expr>),
    /// Logical OR.
    Or(Box<Expr>, Box<Expr>),
    /// Minimum.
    Min(Box<Expr>, Box<Expr>),
    /// Maximum.
    Max(Box<Expr>, Box<Expr>),

    // === Unary operators ===
    /// Logical NOT.
    Not(Box<Expr>),
    /// Negation.
    Neg(Box<Expr>),
    /// Absolute value.
    Abs(Box<Expr>),
}

impl Expr {
    /// Generate a random expression tree with given max depth.
    #[must_use]
    pub fn random<R: Rng>(rng: &mut R, max_depth: usize) -> Self {
        if max_depth == 0 || rng.gen_bool(0.3) {
            Self::random_terminal(rng)
        } else {
            Self::random_operator(rng, max_depth - 1)
        }
    }

    /// Generate a random condition expression (tends toward comparisons).
    #[must_use]
    pub fn random_condition<R: Rng>(rng: &mut R, max_depth: usize) -> Self {
        if max_depth == 0 {
            // At depth 0, create a simple comparison
            let left = Box::new(Self::random_terminal(rng));
            let right = Box::new(Self::random_terminal(rng));
            match rng.gen_range(0..3) {
                0 => Self::Gt(left, right),
                1 => Self::Lt(left, right),
                _ => Self::Eq(left, right),
            }
        } else if rng.gen_bool(0.4) {
            // Sometimes create a logical combination
            let left = Box::new(Self::random_condition(rng, max_depth - 1));
            let right = Box::new(Self::random_condition(rng, max_depth - 1));
            if rng.gen_bool(0.5) {
                Self::And(left, right)
            } else {
                Self::Or(left, right)
            }
        } else {
            // Create a comparison of value expressions
            let left = Box::new(Self::random(rng, max_depth - 1));
            let right = Box::new(Self::random(rng, max_depth - 1));
            match rng.gen_range(0..3) {
                0 => Self::Gt(left, right),
                1 => Self::Lt(left, right),
                _ => Self::Eq(left, right),
            }
        }
    }

    /// Generate a random terminal (leaf) expression.
    #[must_use]
    pub fn random_terminal<R: Rng>(rng: &mut R) -> Self {
        match rng.gen_range(0..15) {
            0 => Self::Const(rng.gen_range(0..NUM_CONSTANTS as u8)),
            1 => Self::Turn,
            2 => Self::MyFood,
            3 => Self::MyPop,
            4 => Self::MyArmy,
            5 => Self::MyTerritory,
            6 => Self::MapWidth,
            7 => Self::MapHeight,
            8 => Self::IterX,
            9 => Self::IterY,
            10 => Self::Reg(rng.gen_range(0..NUM_REGISTERS as u8)),
            11 => Self::TileType(TileRef::random(rng)),
            12 => Self::TileOwner(TileRef::random(rng)),
            13 => Self::TileArmy(TileRef::random(rng)),
            _ => Self::Const(rng.gen_range(0..NUM_CONSTANTS as u8)),
        }
    }

    /// Generate a random operator expression.
    #[must_use]
    fn random_operator<R: Rng>(rng: &mut R, child_depth: usize) -> Self {
        match rng.gen_range(0..15) {
            0 => Self::Add(
                Box::new(Self::random(rng, child_depth)),
                Box::new(Self::random(rng, child_depth)),
            ),
            1 => Self::Sub(
                Box::new(Self::random(rng, child_depth)),
                Box::new(Self::random(rng, child_depth)),
            ),
            2 => Self::Mul(
                Box::new(Self::random(rng, child_depth)),
                Box::new(Self::random(rng, child_depth)),
            ),
            3 => Self::Div(
                Box::new(Self::random(rng, child_depth)),
                Box::new(Self::random(rng, child_depth)),
            ),
            4 => Self::Mod(
                Box::new(Self::random(rng, child_depth)),
                Box::new(Self::random(rng, child_depth)),
            ),
            5 => Self::Gt(
                Box::new(Self::random(rng, child_depth)),
                Box::new(Self::random(rng, child_depth)),
            ),
            6 => Self::Lt(
                Box::new(Self::random(rng, child_depth)),
                Box::new(Self::random(rng, child_depth)),
            ),
            7 => Self::Eq(
                Box::new(Self::random(rng, child_depth)),
                Box::new(Self::random(rng, child_depth)),
            ),
            8 => Self::And(
                Box::new(Self::random(rng, child_depth)),
                Box::new(Self::random(rng, child_depth)),
            ),
            9 => Self::Or(
                Box::new(Self::random(rng, child_depth)),
                Box::new(Self::random(rng, child_depth)),
            ),
            10 => Self::Min(
                Box::new(Self::random(rng, child_depth)),
                Box::new(Self::random(rng, child_depth)),
            ),
            11 => Self::Max(
                Box::new(Self::random(rng, child_depth)),
                Box::new(Self::random(rng, child_depth)),
            ),
            12 => Self::Not(Box::new(Self::random(rng, child_depth))),
            13 => Self::Neg(Box::new(Self::random(rng, child_depth))),
            _ => Self::Abs(Box::new(Self::random(rng, child_depth))),
        }
    }

    /// Count the number of nodes in this expression tree.
    #[must_use]
    pub fn node_count(&self) -> usize {
        match self {
            // Terminals
            Self::Const(_)
            | Self::Turn
            | Self::MyFood
            | Self::MyPop
            | Self::MyArmy
            | Self::MyTerritory
            | Self::MapWidth
            | Self::MapHeight
            | Self::IterX
            | Self::IterY
            | Self::Reg(_)
            | Self::TileType(_)
            | Self::TileOwner(_)
            | Self::TileArmy(_) => 1,

            // Binary operators
            Self::Add(a, b)
            | Self::Sub(a, b)
            | Self::Mul(a, b)
            | Self::Div(a, b)
            | Self::Mod(a, b)
            | Self::Gt(a, b)
            | Self::Lt(a, b)
            | Self::Eq(a, b)
            | Self::And(a, b)
            | Self::Or(a, b)
            | Self::Min(a, b)
            | Self::Max(a, b) => 1 + a.node_count() + b.node_count(),

            // Unary operators
            Self::Not(a) | Self::Neg(a) | Self::Abs(a) => 1 + a.node_count(),
        }
    }
}

/// Reference to a tile location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TileRef {
    /// Absolute coordinates (from constant pool indices).
    Absolute(u8, u8),
    /// Relative offset from current iterator position.
    Relative(i8, i8),
    /// Player's capital.
    Capital,
    /// Current iteration tile.
    IterTile,
}

impl TileRef {
    /// Generate a random tile reference.
    #[must_use]
    pub fn random<R: Rng>(rng: &mut R) -> Self {
        match rng.gen_range(0..4) {
            0 => Self::Absolute(
                rng.gen_range(0..NUM_CONSTANTS as u8),
                rng.gen_range(0..NUM_CONSTANTS as u8),
            ),
            1 => Self::Relative(rng.gen_range(-2..=2), rng.gen_range(-2..=2)),
            2 => Self::Capital,
            _ => Self::IterTile,
        }
    }
}

/// Actions a bot can take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// Move army from one tile to another.
    Move {
        /// Source tile.
        from: TileRef,
        /// Destination tile (must be adjacent).
        to: TileRef,
        /// Number of units to move.
        count: Expr,
    },
    /// Convert population to army at a city.
    Convert {
        /// City to convert at.
        city: TileRef,
        /// Number to convert.
        count: Expr,
    },
    /// Move capital to a new city.
    MoveCapital {
        /// Target city for capital.
        city: TileRef,
    },
    /// Store a value in a register.
    Store {
        /// Register index (0-7).
        reg: u8,
        /// Value to store.
        value: Expr,
    },
    /// Repeat an action up to MAX_REPEAT times.
    Repeat {
        /// Number of iterations (capped at MAX_REPEAT).
        count: Expr,
        /// Action to repeat.
        inner: Box<Action>,
    },
    /// Skip this action (do nothing).
    Skip,
}

impl Action {
    /// Generate a random action.
    #[must_use]
    pub fn random<R: Rng>(rng: &mut R) -> Self {
        match rng.gen_range(0..14) {
            // Move is most common (43%)
            0..=5 => Self::Move {
                from: TileRef::IterTile,
                to: TileRef::Relative(
                    *[(-1, 0), (1, 0), (0, -1), (0, 1)]
                        .choose(rng)
                        .map_or(&0, |(dx, _)| dx),
                    *[(-1, 0), (1, 0), (0, -1), (0, 1)]
                        .choose(rng)
                        .map_or(&0, |(_, dy)| dy),
                ),
                count: Expr::random(rng, 2),
            },
            // Convert (14%)
            6 | 7 => Self::Convert {
                city: if rng.gen_bool(0.5) {
                    TileRef::Capital
                } else {
                    TileRef::IterTile
                },
                count: Expr::random(rng, 2),
            },
            // Move capital (7%)
            8 => Self::MoveCapital {
                city: TileRef::random(rng),
            },
            // Store (14%)
            9 | 10 => Self::Store {
                reg: rng.gen_range(0..NUM_REGISTERS as u8),
                value: Expr::random(rng, 2),
            },
            // Repeat (14%) - only simple actions inside to prevent deep nesting
            11 | 12 => Self::Repeat {
                count: Expr::random(rng, 1),
                inner: Box::new(Self::random_simple(rng)),
            },
            // Skip (7%)
            _ => Self::Skip,
        }
    }

    /// Generate a simple (non-nested) random action for use inside Repeat.
    #[must_use]
    pub fn random_simple<R: Rng>(rng: &mut R) -> Self {
        match rng.gen_range(0..10) {
            // Move (50%)
            0..=4 => Self::Move {
                from: TileRef::IterTile,
                to: TileRef::Relative(
                    *[(-1, 0), (1, 0), (0, -1), (0, 1)]
                        .choose(rng)
                        .map_or(&0, |(dx, _)| dx),
                    *[(-1, 0), (1, 0), (0, -1), (0, 1)]
                        .choose(rng)
                        .map_or(&0, |(_, dy)| dy),
                ),
                count: Expr::random(rng, 1),
            },
            // Convert (20%)
            5 | 6 => Self::Convert {
                city: TileRef::IterTile,
                count: Expr::random(rng, 1),
            },
            // Store (20%)
            7 | 8 => Self::Store {
                reg: rng.gen_range(0..NUM_REGISTERS as u8),
                value: Expr::random(rng, 1),
            },
            // Skip (10%)
            _ => Self::Skip,
        }
    }
}

/// Extension trait for choosing from slices.
trait SliceChoose<T> {
    /// Choose a random element.
    fn choose<R: Rng>(&self, rng: &mut R) -> Option<&T>;
}

impl<T> SliceChoose<T> for [T] {
    fn choose<R: Rng>(&self, rng: &mut R) -> Option<&T> {
        if self.is_empty() {
            None
        } else {
            Some(&self[rng.gen_range(0..self.len())])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;
    use rand::rngs::SmallRng;

    #[test]
    fn test_genome_random() {
        let mut rng = SmallRng::seed_from_u64(12345);
        let genome = Genome::random(&mut rng, 10);

        assert_eq!(genome.rules.len(), 10);
        assert_eq!(genome.constants.len(), NUM_CONSTANTS);
    }

    #[test]
    fn test_expr_node_count() {
        let expr = Expr::Add(
            Box::new(Expr::Const(0)),
            Box::new(Expr::Mul(Box::new(Expr::MyFood), Box::new(Expr::Turn))),
        );
        assert_eq!(expr.node_count(), 5);
    }

    #[test]
    fn test_genome_serialization() {
        let mut rng = SmallRng::seed_from_u64(42);
        let genome = Genome::random(&mut rng, 5);

        let encoded = bincode::serialize(&genome).unwrap();
        let decoded: Genome = bincode::deserialize(&encoded).unwrap();

        assert_eq!(genome, decoded);
    }
}
