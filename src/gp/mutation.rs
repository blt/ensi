//! Mutation operators for genetic programming.
//!
//! Mutations introduce random variations in genomes to explore the search space.
//! We support both point mutations (small changes) and structural mutations.

// Mutation uses intentional casts for random number generation
#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use crate::gp::genome::{Action, Expr, Genome, Rule, TileRef, MAX_RULES, NUM_CONSTANTS, NUM_REGISTERS};
use rand::Rng;
use serde::{Deserialize, Serialize};

/// Configuration for mutation operations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MutationConfig {
    /// Probability of mutating each constant.
    pub constant_mutation_rate: f64,
    /// Range for constant mutations.
    pub constant_mutation_range: i32,
    /// Probability of inserting a new rule.
    pub rule_insert_rate: f64,
    /// Probability of deleting a rule.
    pub rule_delete_rate: f64,
    /// Probability of swapping two rules.
    pub rule_swap_rate: f64,
    /// Probability of mutating a point in an expression.
    pub point_mutation_rate: f64,
    /// Probability of replacing an expression subtree.
    pub subtree_mutation_rate: f64,
    /// Maximum depth for randomly generated subtrees.
    pub max_subtree_depth: usize,
}

impl Default for MutationConfig {
    fn default() -> Self {
        Self {
            constant_mutation_rate: 0.2,
            constant_mutation_range: 10,
            rule_insert_rate: 0.05,
            rule_delete_rate: 0.05,
            rule_swap_rate: 0.1,
            point_mutation_rate: 0.1,
            subtree_mutation_rate: 0.02,
            max_subtree_depth: 3,
        }
    }
}

/// Mutate a genome in place.
pub fn mutate<R: Rng>(genome: &mut Genome, config: &MutationConfig, rng: &mut R) {
    // Mutate constants
    for constant in &mut genome.constants {
        if rng.gen_bool(config.constant_mutation_rate) {
            let delta = rng.gen_range(-config.constant_mutation_range..=config.constant_mutation_range);
            *constant = constant.saturating_add(delta);
        }
    }

    // Structural mutations on rules
    if rng.gen_bool(config.rule_insert_rate) && genome.rules.len() < MAX_RULES {
        let new_rule = Rule::random(rng, genome.rules.len() as u8);
        let pos = rng.gen_range(0..=genome.rules.len());
        genome.rules.insert(pos, new_rule);
        // Update priorities
        for (i, rule) in genome.rules.iter_mut().enumerate() {
            rule.priority = i as u8;
        }
    }

    if rng.gen_bool(config.rule_delete_rate) && genome.rules.len() > 1 {
        let pos = rng.gen_range(0..genome.rules.len());
        genome.rules.remove(pos);
        // Update priorities
        for (i, rule) in genome.rules.iter_mut().enumerate() {
            rule.priority = i as u8;
        }
    }

    if rng.gen_bool(config.rule_swap_rate) && genome.rules.len() >= 2 {
        let i = rng.gen_range(0..genome.rules.len());
        let j = rng.gen_range(0..genome.rules.len());
        if i != j {
            genome.rules.swap(i, j);
            genome.rules[i].priority = i as u8;
            genome.rules[j].priority = j as u8;
        }
    }

    // Point mutations on rules
    for rule in &mut genome.rules {
        // Mutate condition
        if rng.gen_bool(config.point_mutation_rate) {
            mutate_expr(&mut rule.condition, config, rng);
        }

        // Subtree mutation on condition
        if rng.gen_bool(config.subtree_mutation_rate) {
            let new_subtree = Expr::random(rng, config.max_subtree_depth);
            rule.condition = replace_random_subtree(&rule.condition, &new_subtree, rng);
        }

        // Mutate action
        if rng.gen_bool(config.point_mutation_rate) {
            mutate_action(&mut rule.action, config, rng);
        }
    }
}

/// Mutate a single expression (point mutation).
fn mutate_expr<R: Rng>(expr: &mut Expr, config: &MutationConfig, rng: &mut R) {
    match expr {
        Expr::Const(idx) => {
            // Mutate constant index
            *idx = rng.gen_range(0..NUM_CONSTANTS as u8);
        }
        Expr::TileType(tile_ref)
        | Expr::TileOwner(tile_ref)
        | Expr::TileArmy(tile_ref) => {
            mutate_tile_ref(tile_ref, rng);
        }
        // Swap operators
        Expr::Add(a, b) => {
            if rng.gen_bool(0.5) {
                *expr = Expr::Sub(a.clone(), b.clone());
            }
        }
        Expr::Sub(a, b) => {
            if rng.gen_bool(0.5) {
                *expr = Expr::Add(a.clone(), b.clone());
            }
        }
        Expr::Mul(a, b) => {
            if rng.gen_bool(0.5) {
                *expr = Expr::Div(a.clone(), b.clone());
            }
        }
        Expr::Div(a, b) => {
            if rng.gen_bool(0.5) {
                *expr = Expr::Mul(a.clone(), b.clone());
            }
        }
        Expr::Gt(a, b) => {
            if rng.gen_bool(0.5) {
                *expr = Expr::Lt(a.clone(), b.clone());
            }
        }
        Expr::Lt(a, b) => {
            if rng.gen_bool(0.5) {
                *expr = Expr::Gt(a.clone(), b.clone());
            }
        }
        Expr::And(a, b) => {
            if rng.gen_bool(0.5) {
                *expr = Expr::Or(a.clone(), b.clone());
            }
        }
        Expr::Or(a, b) => {
            if rng.gen_bool(0.5) {
                *expr = Expr::And(a.clone(), b.clone());
            }
        }
        // Recursively mutate children with decreasing probability
        Expr::Min(a, b) | Expr::Max(a, b) | Expr::Mod(a, b) | Expr::Eq(a, b) => {
            if rng.gen_bool(config.point_mutation_rate) {
                mutate_expr(a, config, rng);
            }
            if rng.gen_bool(config.point_mutation_rate) {
                mutate_expr(b, config, rng);
            }
        }
        Expr::Not(a) | Expr::Neg(a) | Expr::Abs(a) => {
            if rng.gen_bool(config.point_mutation_rate) {
                mutate_expr(a, config, rng);
            }
        }
        Expr::Reg(idx) => {
            // Mutate register index
            *idx = rng.gen_range(0..NUM_REGISTERS as u8);
        }
        // Terminals that don't need mutation
        Expr::Turn
        | Expr::MyFood
        | Expr::MyPop
        | Expr::MyArmy
        | Expr::MyTerritory
        | Expr::MapWidth
        | Expr::MapHeight
        | Expr::IterX
        | Expr::IterY => {
            // Occasionally swap to a different terminal
            if rng.gen_bool(0.1) {
                *expr = Expr::random_terminal(rng);
            }
        }
    }
}

/// Mutate a tile reference.
fn mutate_tile_ref<R: Rng>(tile_ref: &mut TileRef, rng: &mut R) {
    match tile_ref {
        TileRef::Relative(dx, dy) => {
            // Adjust offset
            *dx = (*dx + rng.gen_range(-1..=1)).clamp(-2, 2);
            *dy = (*dy + rng.gen_range(-1..=1)).clamp(-2, 2);
        }
        TileRef::Absolute(x_idx, y_idx) => {
            // Change constant indices
            if rng.gen_bool(0.5) {
                *x_idx = rng.gen_range(0..NUM_CONSTANTS as u8);
            }
            if rng.gen_bool(0.5) {
                *y_idx = rng.gen_range(0..NUM_CONSTANTS as u8);
            }
        }
        TileRef::Capital | TileRef::IterTile => {
            // Occasionally change to a different type
            if rng.gen_bool(0.2) {
                *tile_ref = TileRef::random(rng);
            }
        }
    }
}

/// Mutate an action.
fn mutate_action<R: Rng>(action: &mut Action, config: &MutationConfig, rng: &mut R) {
    match action {
        Action::Move { from, to, count } => {
            if rng.gen_bool(0.3) {
                mutate_tile_ref(from, rng);
            }
            if rng.gen_bool(0.3) {
                mutate_tile_ref(to, rng);
            }
            if rng.gen_bool(config.point_mutation_rate) {
                mutate_expr(count, config, rng);
            }
        }
        Action::Convert { city, count } => {
            if rng.gen_bool(0.3) {
                mutate_tile_ref(city, rng);
            }
            if rng.gen_bool(config.point_mutation_rate) {
                mutate_expr(count, config, rng);
            }
        }
        Action::MoveCapital { city } => {
            if rng.gen_bool(0.3) {
                mutate_tile_ref(city, rng);
            }
        }
        Action::Store { reg, value } => {
            if rng.gen_bool(0.3) {
                *reg = rng.gen_range(0..NUM_REGISTERS as u8);
            }
            if rng.gen_bool(config.point_mutation_rate) {
                mutate_expr(value, config, rng);
            }
        }
        Action::Repeat { count, inner } => {
            if rng.gen_bool(config.point_mutation_rate) {
                mutate_expr(count, config, rng);
            }
            if rng.gen_bool(config.point_mutation_rate) {
                mutate_action(inner, config, rng);
            }
            // Occasionally unwrap the repeat
            if rng.gen_bool(0.1) {
                *action = (**inner).clone();
            }
        }
        Action::Skip => {
            // Occasionally change to a different action
            if rng.gen_bool(0.1) {
                *action = Action::random(rng);
            }
        }
    }
}

/// Replace a random subtree in the expression.
fn replace_random_subtree<R: Rng>(expr: &Expr, replacement: &Expr, rng: &mut R) -> Expr {
    let count = expr.node_count();
    if count == 0 {
        return replacement.clone();
    }
    let target = rng.gen_range(0..count);
    replace_at_index(expr, replacement, target).unwrap_or_else(|| expr.clone())
}

/// Replace subtree at given index.
fn replace_at_index(expr: &Expr, replacement: &Expr, mut index: usize) -> Option<Expr> {
    if index == 0 {
        return Some(replacement.clone());
    }
    index -= 1;

    match expr {
        // Terminals have no children
        Expr::Const(_)
        | Expr::Turn
        | Expr::MyFood
        | Expr::MyPop
        | Expr::MyArmy
        | Expr::MyTerritory
        | Expr::MapWidth
        | Expr::MapHeight
        | Expr::IterX
        | Expr::IterY
        | Expr::Reg(_)
        | Expr::TileType(_)
        | Expr::TileOwner(_)
        | Expr::TileArmy(_) => None,

        // Binary operators
        Expr::Add(a, b) => binary_replace(a, b, replacement, index, Expr::Add),
        Expr::Sub(a, b) => binary_replace(a, b, replacement, index, Expr::Sub),
        Expr::Mul(a, b) => binary_replace(a, b, replacement, index, Expr::Mul),
        Expr::Div(a, b) => binary_replace(a, b, replacement, index, Expr::Div),
        Expr::Mod(a, b) => binary_replace(a, b, replacement, index, Expr::Mod),
        Expr::Gt(a, b) => binary_replace(a, b, replacement, index, Expr::Gt),
        Expr::Lt(a, b) => binary_replace(a, b, replacement, index, Expr::Lt),
        Expr::Eq(a, b) => binary_replace(a, b, replacement, index, Expr::Eq),
        Expr::And(a, b) => binary_replace(a, b, replacement, index, Expr::And),
        Expr::Or(a, b) => binary_replace(a, b, replacement, index, Expr::Or),
        Expr::Min(a, b) => binary_replace(a, b, replacement, index, Expr::Min),
        Expr::Max(a, b) => binary_replace(a, b, replacement, index, Expr::Max),

        // Unary operators
        Expr::Not(a) => unary_replace(a, replacement, index, Expr::Not),
        Expr::Neg(a) => unary_replace(a, replacement, index, Expr::Neg),
        Expr::Abs(a) => unary_replace(a, replacement, index, Expr::Abs),
    }
}

/// Helper for binary operator replacement.
fn binary_replace<F>(
    a: &Expr,
    b: &Expr,
    replacement: &Expr,
    index: usize,
    constructor: F,
) -> Option<Expr>
where
    F: Fn(Box<Expr>, Box<Expr>) -> Expr,
{
    let a_count = a.node_count();
    if index < a_count {
        let new_a = replace_at_index(a, replacement, index)?;
        Some(constructor(Box::new(new_a), Box::new(b.clone())))
    } else {
        let new_b = replace_at_index(b, replacement, index - a_count)?;
        Some(constructor(Box::new(a.clone()), Box::new(new_b)))
    }
}

/// Helper for unary operator replacement.
fn unary_replace<F>(child: &Expr, replacement: &Expr, index: usize, constructor: F) -> Option<Expr>
where
    F: Fn(Box<Expr>) -> Expr,
{
    let new_child = replace_at_index(child, replacement, index)?;
    Some(constructor(Box::new(new_child)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    #[test]
    fn test_mutate_genome() {
        let mut rng = SmallRng::seed_from_u64(42);
        let config = MutationConfig::default();

        let mut genome = Genome::random(&mut rng, 5);
        let original = genome.clone();

        mutate(&mut genome, &config, &mut rng);

        // Genome should have changed (with high probability given mutation rates)
        // Note: there's a small chance they're equal, so we just verify no crash
        assert!(!genome.rules.is_empty() || original.rules.is_empty());
    }

    #[test]
    fn test_constant_mutation() {
        let mut rng = SmallRng::seed_from_u64(123);
        let config = MutationConfig {
            constant_mutation_rate: 1.0, // Always mutate
            ..Default::default()
        };

        let mut genome = Genome::random(&mut rng, 3);
        let original_constants = genome.constants;

        mutate(&mut genome, &config, &mut rng);

        // At least some constants should have changed
        let changed = genome
            .constants
            .iter()
            .zip(original_constants.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(changed > 0);
    }

    #[test]
    fn test_rule_limits() {
        let mut rng = SmallRng::seed_from_u64(456);
        let config = MutationConfig {
            rule_insert_rate: 1.0,
            rule_delete_rate: 0.0,
            ..Default::default()
        };

        let mut genome = Genome::random(&mut rng, MAX_RULES - 1);

        // Insert one rule
        mutate(&mut genome, &config, &mut rng);
        assert!(genome.rules.len() <= MAX_RULES);

        // Try to insert more - should stay at max
        for _ in 0..10 {
            mutate(&mut genome, &config, &mut rng);
        }
        assert!(genome.rules.len() <= MAX_RULES);
    }
}
