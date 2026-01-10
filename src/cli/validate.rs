//! WASM validation command implementation.

use super::CliError;
use ensi::wasm::WasmBot;
use std::fs;
use std::path::PathBuf;

/// Execute the validate command.
///
/// # Errors
///
/// Returns an error if the WASM file cannot be read or is invalid.
pub(crate) fn execute(bot: PathBuf) -> Result<(), CliError> {
    let wasm_bytes = fs::read(&bot).map_err(|e| {
        CliError::new(format!("Failed to read {}: {e}", bot.display()))
    })?;

    println!("Validating: {}", bot.display());
    println!();

    // Check WASM magic number
    let magic_ok = wasm_bytes.len() >= 4 && &wasm_bytes[0..4] == b"\0asm";
    print_check("WASM magic number", magic_ok);
    if !magic_ok {
        return Err(CliError::new("Not a valid WASM file (missing magic number)"));
    }

    // Check version
    let version_ok = wasm_bytes.len() >= 8 && wasm_bytes[4..8] == [1, 0, 0, 0];
    print_check("WASM version 1", version_ok);
    if !version_ok {
        return Err(CliError::new("Unsupported WASM version (expected version 1)"));
    }

    // Try to create engine and load module
    println!();
    print!("Module load test... ");

    let engine = WasmBot::create_engine()
        .map_err(|e| CliError::new(format!("Failed to create WASM engine: {e}")))?;

    match WasmBot::from_bytes(&engine, &wasm_bytes, 1, 64, 64) {
        Ok(_bot) => {
            println!("OK");
            println!();
            println!("Summary:");
            println!("  File size:    {} bytes", wasm_bytes.len());
            println!("  run_turn:     exported (required)");
        }
        Err(e) => {
            println!("FAILED");
            return Err(CliError::new(format!("Module load failed: {e}")));
        }
    }

    println!();
    println!("Validation successful!");

    Ok(())
}

fn print_check(name: &str, ok: bool) {
    let status = if ok { "OK" } else { "FAILED" };
    let symbol = if ok { "✓" } else { "✗" };
    println!("  {symbol} {name}: {status}");
}
