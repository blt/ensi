//! Persistence for populations and evolution state.
//!
//! Uses bincode for efficient binary serialization and LZ4 for compression.
//! This provides compact storage with fast load/save times.

// Persistence uses intentional casts for timestamp/date calculations
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::manual_let_else
)]

use crate::gp::genome::Genome;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Read, Write};
use std::path::Path;

/// Magic bytes for file format identification.
const MAGIC: &[u8; 4] = b"ENSI";

/// Current format version.
const VERSION: u8 = 1;

/// Header for population files.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct FileHeader {
    /// Format version.
    version: u8,
    /// Generation number.
    generation: u32,
    /// Population size.
    population_size: u32,
}

/// Evolution checkpoint containing population and metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Current generation number.
    pub generation: u32,
    /// Population of genomes.
    pub population: Vec<Genome>,
    /// Fitness values for each genome.
    pub fitness: Vec<f64>,
    /// Best fitness seen so far.
    pub best_fitness: f64,
    /// RNG state for reproducibility (seed that was used).
    pub rng_seed: u64,
}

/// Save a population to a file with compression.
///
/// # Errors
///
/// Returns an error if serialization or file I/O fails.
pub fn save_population(population: &[Genome], generation: u32, path: &Path) -> io::Result<()> {
    let checkpoint = Checkpoint {
        generation,
        population: population.to_vec(),
        fitness: Vec::new(),
        best_fitness: 0.0,
        rng_seed: 0,
    };

    save_checkpoint(&checkpoint, path)
}

/// Save a full checkpoint to a file with compression.
///
/// # Errors
///
/// Returns an error if serialization or file I/O fails.
pub fn save_checkpoint(checkpoint: &Checkpoint, path: &Path) -> io::Result<()> {
    // Serialize with bincode
    let encoded = bincode::serialize(checkpoint)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Compress with LZ4
    let compressed = lz4_flex::compress_prepend_size(&encoded);

    // Write to file with header
    let mut file = fs::File::create(path)?;
    file.write_all(MAGIC)?;
    file.write_all(&[VERSION])?;
    file.write_all(&compressed)?;

    Ok(())
}

/// Load a population from a file.
///
/// # Errors
///
/// Returns an error if the file format is invalid or decompression fails.
pub fn load_population(path: &Path) -> io::Result<Vec<Genome>> {
    let checkpoint = load_checkpoint(path)?;
    Ok(checkpoint.population)
}

/// Load a full checkpoint from a file.
///
/// # Errors
///
/// Returns an error if the file format is invalid or decompression fails.
pub fn load_checkpoint(path: &Path) -> io::Result<Checkpoint> {
    let mut file = fs::File::open(path)?;

    // Read and verify magic
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid file magic",
        ));
    }

    // Read version
    let mut version = [0u8; 1];
    file.read_exact(&mut version)?;
    if version[0] != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported version: {}", version[0]),
        ));
    }

    // Read compressed data
    let mut compressed = Vec::new();
    file.read_to_end(&mut compressed)?;

    // Decompress
    let decompressed = lz4_flex::decompress_size_prepended(&compressed)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    // Deserialize
    let checkpoint: Checkpoint = bincode::deserialize(&decompressed)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    Ok(checkpoint)
}

/// Get the path for a generation checkpoint file.
#[must_use]
pub fn checkpoint_path(output_dir: &Path, generation: u32) -> std::path::PathBuf {
    output_dir.join(format!("gen_{generation:05}.bin"))
}

/// Get the path for the best genome WASM file.
#[must_use]
pub fn best_wasm_path(output_dir: &Path) -> std::path::PathBuf {
    output_dir.join("best.wasm")
}

/// Save the best genome as a WASM file.
///
/// # Errors
///
/// Returns an error if compilation or file I/O fails.
pub fn save_best_wasm(genome: &Genome, path: &Path) -> io::Result<()> {
    use crate::gp::compiler::compile;

    let wasm = compile(genome)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;

    fs::write(path, wasm)
}

/// Subdirectory within ~/.ensi for evolved bots.
const EVOLVED_SUBDIR: &str = "evolved";

/// Get the path to the ensi data directory (~/.ensi).
///
/// Creates the directory if it doesn't exist.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined or directory creation fails.
pub fn ensi_data_dir() -> io::Result<std::path::PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "cannot determine home directory"))?;

    let data_dir = Path::new(&home).join(".ensi");
    fs::create_dir_all(&data_dir)?;
    Ok(data_dir)
}

/// Get the path to the evolved bots directory (~/.ensi/evolved).
///
/// Creates the directory if it doesn't exist.
///
/// # Errors
///
/// Returns an error if directory creation fails.
pub fn evolved_bots_dir() -> io::Result<std::path::PathBuf> {
    let data_dir = ensi_data_dir()?;
    let evolved_dir = data_dir.join(EVOLVED_SUBDIR);
    fs::create_dir_all(&evolved_dir)?;
    Ok(evolved_dir)
}

/// Metadata about a saved evolved bot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolvedBotMeta {
    /// Timestamp when saved (Unix epoch seconds).
    pub timestamp: u64,
    /// Generation number when saved.
    pub generation: u32,
    /// Best fitness at time of saving.
    pub fitness: f64,
    /// Number of rules in the genome.
    pub num_rules: usize,
    /// Optional description.
    pub description: Option<String>,
}

/// Save an evolved bot to permanent storage (~/.ensi/evolved/).
///
/// This saves both the WASM file and the genome checkpoint.
/// Files are named with timestamp for uniqueness: `evolved_YYYYMMDD_HHMMSS.wasm`
///
/// # Errors
///
/// Returns an error if file I/O fails.
pub fn save_evolved_bot(
    genome: &Genome,
    generation: u32,
    fitness: f64,
) -> io::Result<std::path::PathBuf> {
    use crate::gp::compiler::compile;
    use std::time::{SystemTime, UNIX_EPOCH};

    let evolved_dir = evolved_bots_dir()?;

    // Generate timestamp-based filename
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let time_str = format_timestamp(timestamp);
    let base_name = format!("evolved_{time_str}");

    // Save WASM
    let wasm_path = evolved_dir.join(format!("{base_name}.wasm"));
    let wasm = compile(genome)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
    fs::write(&wasm_path, &wasm)?;

    // Save genome checkpoint
    let genome_path = evolved_dir.join(format!("{base_name}.genome"));
    let checkpoint = Checkpoint {
        generation,
        population: vec![genome.clone()],
        fitness: vec![fitness],
        best_fitness: fitness,
        rng_seed: 0,
    };
    save_checkpoint(&checkpoint, &genome_path)?;

    // Save metadata as JSON for easy inspection
    let meta = EvolvedBotMeta {
        timestamp,
        generation,
        fitness,
        num_rules: genome.rules.len(),
        description: None,
    };
    let meta_path = evolved_dir.join(format!("{base_name}.json"));
    let meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(&meta_path, meta_json)?;

    Ok(wasm_path)
}

/// Format a Unix timestamp as `YYYYMMDD_HHMMSS` using simple arithmetic.
fn format_timestamp(secs: u64) -> String {
    const SECS_PER_DAY: u64 = 86400;
    const SECS_PER_HOUR: u64 = 3600;
    const SECS_PER_MIN: u64 = 60;

    // Extract time of day
    let time_of_day = secs % SECS_PER_DAY;
    let hour = time_of_day / SECS_PER_HOUR;
    let min = (time_of_day % SECS_PER_HOUR) / SECS_PER_MIN;
    let sec = time_of_day % SECS_PER_MIN;

    // Calculate days since epoch
    let mut days = secs / SECS_PER_DAY;

    // Approximate year (will refine)
    let mut year = 1970 + (days * 400 / 146_097) as i32;
    days -= days_since_epoch(year) as u64;

    // Refine year
    while days >= days_in_year(year) as u64 {
        days -= days_in_year(year) as u64;
        year += 1;
    }

    // Calculate month and day
    let is_leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
    let days_in_months: [u64; 12] = if is_leap {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 0;
    while month < 12 && days >= days_in_months[month] {
        days -= days_in_months[month];
        month += 1;
    }
    let day = days + 1;

    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        year,
        month + 1,
        day,
        hour,
        min,
        sec
    )
}

fn days_in_year(year: i32) -> i32 {
    if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
        366
    } else {
        365
    }
}

fn days_since_epoch(year: i32) -> i32 {
    let mut days = 0;
    for y in 1970..year {
        days += days_in_year(y);
    }
    days
}

/// List all evolved bots in permanent storage (~/.ensi/evolved/).
///
/// Returns a list of (path, metadata) tuples sorted by timestamp (newest first).
///
/// # Errors
///
/// Returns an error if reading the directory fails.
pub fn list_evolved_bots() -> io::Result<Vec<(std::path::PathBuf, EvolvedBotMeta)>> {
    let evolved_dir = match evolved_bots_dir() {
        Ok(d) => d,
        Err(_) => return Ok(Vec::new()),
    };

    if !evolved_dir.exists() {
        return Ok(Vec::new());
    }

    let mut bots = Vec::new();

    for entry in fs::read_dir(&evolved_dir)? {
        let entry = entry?;
        let path = entry.path();

        // Only look at .json metadata files
        if path.extension().is_some_and(|e| e == "json")
            && let Ok(meta_str) = fs::read_to_string(&path)
                && let Ok(meta) = serde_json::from_str::<EvolvedBotMeta>(&meta_str) {
                    // Get corresponding WASM path
                    let wasm_path = path.with_extension("wasm");
                    if wasm_path.exists() {
                        bots.push((wasm_path, meta));
                    }
                }
    }

    // Sort by timestamp, newest first
    bots.sort_by(|a, b| b.1.timestamp.cmp(&a.1.timestamp));

    Ok(bots)
}

/// Load an evolved bot's genome from permanent storage.
///
/// # Errors
///
/// Returns an error if the genome file doesn't exist or is invalid.
pub fn load_evolved_genome(wasm_path: &Path) -> io::Result<Genome> {
    let genome_path = wasm_path.with_extension("genome");
    let checkpoint = load_checkpoint(&genome_path)?;
    checkpoint
        .population
        .into_iter()
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty genome file"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::SmallRng;
    use rand::SeedableRng;
    use tempfile::tempdir;

    #[test]
    fn test_save_load_roundtrip() {
        let mut rng = SmallRng::seed_from_u64(42);
        let population: Vec<Genome> = (0..10).map(|_| Genome::random(&mut rng, 5)).collect();

        let dir = tempdir().unwrap();
        let path = dir.path().join("test.bin");

        save_population(&population, 0, &path).unwrap();
        let loaded = load_population(&path).unwrap();

        assert_eq!(population.len(), loaded.len());
        assert_eq!(population, loaded);
    }

    #[test]
    fn test_checkpoint_roundtrip() {
        let mut rng = SmallRng::seed_from_u64(123);
        let population: Vec<Genome> = (0..5).map(|_| Genome::random(&mut rng, 3)).collect();
        let fitness: Vec<f64> = (0..5).map(|i| i as f64 * 0.2).collect();

        let checkpoint = Checkpoint {
            generation: 42,
            population: population.clone(),
            fitness: fitness.clone(),
            best_fitness: 0.8,
            rng_seed: 12345,
        };

        let dir = tempdir().unwrap();
        let path = dir.path().join("checkpoint.bin");

        save_checkpoint(&checkpoint, &path).unwrap();
        let loaded = load_checkpoint(&path).unwrap();

        assert_eq!(loaded.generation, 42);
        assert_eq!(loaded.population, population);
        assert_eq!(loaded.fitness, fitness);
        assert!((loaded.best_fitness - 0.8).abs() < 0.001);
        assert_eq!(loaded.rng_seed, 12345);
    }

    #[test]
    fn test_invalid_magic() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("bad.bin");

        fs::write(&path, b"BAAD").unwrap();

        let result = load_checkpoint(&path);
        assert!(result.is_err());
    }
}
