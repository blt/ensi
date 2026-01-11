//! Crossover operators for genetic programming.
//!
//! Crossover combines genetic material from two parent genomes to create
//! offspring. We support both rule-level and expression-level crossover.

// Crossover uses intentional casts for random number operations
#![allow(clippy::cast_possible_truncation)]

use crate::gp::genome::{Expr, Genome, Rule, MAX_RULES};
use rand::Rng;
use serde::{Deserialize, Serialize};

/// Configuration for crossover operations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CrossoverConfig {
    /// Probability of rule-level crossover (vs. no crossover).
    pub rule_crossover_rate: f64,
    /// Probability of also doing expression crossover after rule crossover.
    pub expr_crossover_rate: f64,
}

impl Default for CrossoverConfig {
    fn default() -> Self {
        Self {
            rule_crossover_rate: 0.9,
            expr_crossover_rate: 0.2,
        }
    }
}

/// Perform crossover between two parent genomes.
///
/// Returns a new child genome combining genetic material from both parents.
#[must_use]
pub fn crossover<R: Rng>(parent1: &Genome, parent2: &Genome, config: &CrossoverConfig, rng: &mut R) -> Genome {
    if !rng.gen_bool(config.rule_crossover_rate) {
        // No crossover - return a clone of a random parent
        return if rng.gen_bool(0.5) {
            parent1.clone()
        } else {
            parent2.clone()
        };
    }

    // Rule-level crossover
    let mut child = crossover_rules(parent1, parent2, rng);

    // Optionally do expression-level crossover
    if rng.gen_bool(config.expr_crossover_rate) && !child.rules.is_empty() {
        let rule_idx = rng.gen_range(0..child.rules.len());
        if let Some(p2_rule) = parent2.rules.get(rng.gen_range(0..parent2.rules.len().max(1))) {
            child.rules[rule_idx].condition = crossover_expr(
                &child.rules[rule_idx].condition,
                &p2_rule.condition,
                rng,
            );
        }
    }

    // Crossover constants
    for i in 0..child.constants.len() {
        if rng.gen_bool(0.5) {
            child.constants[i] = parent2.constants[i];
        }
    }

    child
}

/// One-point crossover on rules.
fn crossover_rules<R: Rng>(parent1: &Genome, parent2: &Genome, rng: &mut R) -> Genome {
    if parent1.rules.is_empty() && parent2.rules.is_empty() {
        return Genome {
            rules: Vec::new(),
            constants: parent1.constants,
        };
    }

    let len1 = parent1.rules.len();
    let len2 = parent2.rules.len();

    // Choose crossover point
    let point1 = if len1 > 0 { rng.gen_range(0..=len1) } else { 0 };
    let point2 = if len2 > 0 { rng.gen_range(0..=len2) } else { 0 };

    // Create child by combining rules from both parents
    let mut rules: Vec<Rule> = parent1.rules[..point1].to_vec();
    rules.extend(parent2.rules[point2..].iter().cloned());

    // Enforce max rules limit
    rules.truncate(MAX_RULES);

    // Re-assign priorities
    for (i, rule) in rules.iter_mut().enumerate() {
        rule.priority = i as u8;
    }

    Genome {
        rules,
        constants: parent1.constants,
    }
}

/// Subtree crossover on expressions.
///
/// Replaces a random subtree in expr1 with a random subtree from expr2.
fn crossover_expr<R: Rng>(expr1: &Expr, expr2: &Expr, rng: &mut R) -> Expr {
    let donor = select_random_subtree(expr2, rng);
    replace_random_subtree(expr1, &donor, rng)
}

/// Select a random subtree from an expression.
fn select_random_subtree<R: Rng>(expr: &Expr, rng: &mut R) -> Expr {
    let count = expr.node_count();
    let target = rng.gen_range(0..count);
    select_subtree_at_index(expr, target).unwrap_or_else(|| expr.clone())
}

/// Select the subtree at the given index (preorder traversal).
fn select_subtree_at_index(expr: &Expr, mut index: usize) -> Option<Expr> {
    if index == 0 {
        return Some(expr.clone());
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
        | Expr::TileType(_)
        | Expr::TileOwner(_)
        | Expr::TileArmy(_) => None,

        // Binary operators
        Expr::Add(a, b)
        | Expr::Sub(a, b)
        | Expr::Mul(a, b)
        | Expr::Div(a, b)
        | Expr::Mod(a, b)
        | Expr::Gt(a, b)
        | Expr::Lt(a, b)
        | Expr::Eq(a, b)
        | Expr::And(a, b)
        | Expr::Or(a, b)
        | Expr::Min(a, b)
        | Expr::Max(a, b) => {
            let a_count = a.node_count();
            if index < a_count {
                select_subtree_at_index(a, index)
            } else {
                select_subtree_at_index(b, index - a_count)
            }
        }

        // Unary operators
        Expr::Not(a) | Expr::Neg(a) | Expr::Abs(a) => select_subtree_at_index(a, index),
    }
}

/// Replace a random subtree with a donor expression.
fn replace_random_subtree<R: Rng>(expr: &Expr, donor: &Expr, rng: &mut R) -> Expr {
    let count = expr.node_count();
    let target = rng.gen_range(0..count);
    replace_subtree_at_index(expr, donor, target).unwrap_or_else(|| expr.clone())
}

/// Replace the subtree at the given index with the donor.
fn replace_subtree_at_index(expr: &Expr, donor: &Expr, mut index: usize) -> Option<Expr> {
    if index == 0 {
        return Some(donor.clone());
    }
    index -= 1;

    match expr {
        // Terminals have no children to replace
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
        | Expr::TileType(_)
        | Expr::TileOwner(_)
        | Expr::TileArmy(_) => None,

        // Binary operators
        Expr::Add(a, b) => binary_replace(a, b, donor, index, Expr::Add),
        Expr::Sub(a, b) => binary_replace(a, b, donor, index, Expr::Sub),
        Expr::Mul(a, b) => binary_replace(a, b, donor, index, Expr::Mul),
        Expr::Div(a, b) => binary_replace(a, b, donor, index, Expr::Div),
        Expr::Mod(a, b) => binary_replace(a, b, donor, index, Expr::Mod),
        Expr::Gt(a, b) => binary_replace(a, b, donor, index, Expr::Gt),
        Expr::Lt(a, b) => binary_replace(a, b, donor, index, Expr::Lt),
        Expr::Eq(a, b) => binary_replace(a, b, donor, index, Expr::Eq),
        Expr::And(a, b) => binary_replace(a, b, donor, index, Expr::And),
        Expr::Or(a, b) => binary_replace(a, b, donor, index, Expr::Or),
        Expr::Min(a, b) => binary_replace(a, b, donor, index, Expr::Min),
        Expr::Max(a, b) => binary_replace(a, b, donor, index, Expr::Max),

        // Unary operators
        Expr::Not(a) => unary_replace(a, donor, index, Expr::Not),
        Expr::Neg(a) => unary_replace(a, donor, index, Expr::Neg),
        Expr::Abs(a) => unary_replace(a, donor, index, Expr::Abs),
    }
}

/// Helper for binary operator replacement.
fn binary_replace<F>(
    a: &Expr,
    b: &Expr,
    donor: &Expr,
    index: usize,
    constructor: F,
) -> Option<Expr>
where
    F: Fn(Box<Expr>, Box<Expr>) -> Expr,
{
    let a_count = a.node_count();
    if index < a_count {
        let new_a = replace_subtree_at_index(a, donor, index)?;
        Some(constructor(Box::new(new_a), Box::new(b.clone())))
    } else {
        let new_b = replace_subtree_at_index(b, donor, index - a_count)?;
        Some(constructor(Box::new(a.clone()), Box::new(new_b)))
    }
}

/// Helper for unary operator replacement.
fn unary_replace<F>(child: &Expr, donor: &Expr, index: usize, constructor: F) -> Option<Expr>
where
    F: Fn(Box<Expr>) -> Expr,
{
    let new_child = replace_subtree_at_index(child, donor, index)?;
    Some(constructor(Box::new(new_child)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    #[test]
    fn test_crossover_rules() {
        let mut rng = SmallRng::seed_from_u64(42);

        let parent1 = Genome::random(&mut rng, 5);
        let parent2 = Genome::random(&mut rng, 5);

        let child = crossover_rules(&parent1, &parent2, &mut rng);

        assert!(!child.rules.is_empty());
        assert!(child.rules.len() <= MAX_RULES);
    }

    #[test]
    fn test_crossover_preserves_structure() {
        let mut rng = SmallRng::seed_from_u64(123);
        let config = CrossoverConfig::default();

        let parent1 = Genome::random(&mut rng, 3);
        let parent2 = Genome::random(&mut rng, 3);

        let child = crossover(&parent1, &parent2, &config, &mut rng);

        // Child should have valid structure
        assert!(child.rules.len() <= MAX_RULES);
        assert_eq!(child.constants.len(), parent1.constants.len());
    }

    #[test]
    fn test_no_crossover() {
        let mut rng = SmallRng::seed_from_u64(456);
        let config = CrossoverConfig {
            rule_crossover_rate: 0.0,
            expr_crossover_rate: 0.0,
        };

        let parent1 = Genome::random(&mut rng, 3);
        let parent2 = Genome::random(&mut rng, 3);

        let child = crossover(&parent1, &parent2, &config, &mut rng);

        // Child should be a clone of one parent
        assert!(child == parent1 || child == parent2);
    }
}
