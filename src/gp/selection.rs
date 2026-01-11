//! Selection operators for genetic programming.
//!
//! Selection determines which individuals survive and reproduce based on fitness.
//! We use tournament selection with elitism to maintain population quality.

// Selection uses intentional casts for statistics
#![allow(clippy::cast_precision_loss)]

use rand::Rng;
use serde::{Deserialize, Serialize};

/// Configuration for selection operations.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SelectionConfig {
    /// Number of individuals competing in each tournament.
    pub tournament_size: usize,
    /// Number of elite individuals preserved unchanged.
    pub elite_count: usize,
}

impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            tournament_size: 5,
            elite_count: 2,
        }
    }
}

/// Result of selection: indices of parents for the next generation.
#[derive(Debug)]
pub struct SelectionResult {
    /// Indices of elite individuals (preserved unchanged).
    pub elite_indices: Vec<usize>,
    /// Pairs of parent indices for crossover.
    pub parent_pairs: Vec<(usize, usize)>,
}

/// Select parents for the next generation using tournament selection.
///
/// Returns elite individuals (preserved unchanged) and pairs of parents
/// for crossover to fill the remaining population.
#[must_use]
pub fn select_parents<R: Rng>(
    fitness: &[f64],
    config: &SelectionConfig,
    target_size: usize,
    rng: &mut R,
) -> SelectionResult {
    let pop_size = fitness.len();

    // Select elite individuals
    let elite_count = config.elite_count.min(pop_size).min(target_size);
    let elite_indices = select_elite(fitness, elite_count);

    // Calculate number of offspring needed
    let offspring_needed = target_size.saturating_sub(elite_count);
    let pairs_needed = offspring_needed.div_ceil(2); // Each pair produces 2 offspring

    // Select parent pairs using tournament selection
    let mut parent_pairs = Vec::with_capacity(pairs_needed);
    for _ in 0..pairs_needed {
        let p1 = tournament_select(fitness, config.tournament_size, rng);
        let p2 = tournament_select(fitness, config.tournament_size, rng);
        parent_pairs.push((p1, p2));
    }

    SelectionResult {
        elite_indices,
        parent_pairs,
    }
}

/// Select the top N individuals by fitness.
fn select_elite(fitness: &[f64], count: usize) -> Vec<usize> {
    let mut indexed: Vec<(usize, f64)> = fitness.iter().copied().enumerate().collect();
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    indexed.into_iter().take(count).map(|(i, _)| i).collect()
}

/// Tournament selection: randomly select k individuals and return the best.
fn tournament_select<R: Rng>(fitness: &[f64], k: usize, rng: &mut R) -> usize {
    let pop_size = fitness.len();
    if pop_size == 0 {
        return 0;
    }

    let k = k.min(pop_size).max(1);
    let mut best_idx = rng.gen_range(0..pop_size);
    let mut best_fitness = fitness[best_idx];

    for _ in 1..k {
        let idx = rng.gen_range(0..pop_size);
        if fitness[idx] > best_fitness {
            best_idx = idx;
            best_fitness = fitness[idx];
        }
    }

    best_idx
}

/// Calculate selection pressure statistics.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy)]
pub struct SelectionStats {
    /// Mean fitness of the population.
    pub mean_fitness: f64,
    /// Best fitness in the population.
    pub best_fitness: f64,
    /// Worst fitness in the population.
    pub worst_fitness: f64,
    /// Standard deviation of fitness.
    pub fitness_std: f64,
}

impl SelectionStats {
    /// Calculate statistics from fitness values.
    #[must_use]
    pub fn from_fitness(fitness: &[f64]) -> Self {
        if fitness.is_empty() {
            return Self {
                mean_fitness: 0.0,
                best_fitness: 0.0,
                worst_fitness: 0.0,
                fitness_std: 0.0,
            };
        }

        let sum: f64 = fitness.iter().sum();
        let mean = sum / fitness.len() as f64;

        let best = fitness
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max);
        let worst = fitness
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);

        let variance: f64 = fitness
            .iter()
            .map(|f| (f - mean).powi(2))
            .sum::<f64>()
            / fitness.len() as f64;
        let std = variance.sqrt();

        Self {
            mean_fitness: mean,
            best_fitness: best,
            worst_fitness: worst,
            fitness_std: std,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;

    #[test]
    fn test_tournament_selection_prefers_fitter() {
        let mut rng = SmallRng::seed_from_u64(42);
        let fitness = vec![0.1, 0.5, 0.9, 0.2, 0.8];

        // Run many selections and count which indices are chosen
        let mut counts = [0usize; 5];
        for _ in 0..1000 {
            let idx = tournament_select(&fitness, 3, &mut rng);
            counts[idx] += 1;
        }

        // Index 2 (fitness 0.9) should be selected most often
        let max_idx = counts.iter().enumerate().max_by_key(|(_, c)| *c).unwrap().0;
        assert_eq!(max_idx, 2);
    }

    #[test]
    fn test_elite_selection() {
        let fitness = vec![0.3, 0.9, 0.1, 0.8, 0.5];
        let elite = select_elite(&fitness, 2);

        assert_eq!(elite.len(), 2);
        assert!(elite.contains(&1)); // 0.9
        assert!(elite.contains(&3)); // 0.8
    }

    #[test]
    fn test_select_parents() {
        let mut rng = SmallRng::seed_from_u64(123);
        let fitness = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0];
        let config = SelectionConfig {
            tournament_size: 3,
            elite_count: 2,
        };

        let result = select_parents(&fitness, &config, 10, &mut rng);

        assert_eq!(result.elite_indices.len(), 2);
        assert!(!result.parent_pairs.is_empty());
    }

    #[test]
    fn test_selection_stats() {
        let fitness = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let stats = SelectionStats::from_fitness(&fitness);

        assert!((stats.mean_fitness - 3.0).abs() < 0.001);
        assert!((stats.best_fitness - 5.0).abs() < 0.001);
        assert!((stats.worst_fitness - 1.0).abs() < 0.001);
    }
}
