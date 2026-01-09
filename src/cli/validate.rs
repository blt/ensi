//! ELF validation command implementation.

use super::CliError;
use ensi::tournament::{load_elf, TEXT_BASE};
use goblin::elf::Elf;
use std::fs;
use std::path::PathBuf;

/// Execute the validate command.
///
/// # Errors
///
/// Returns an error if the ELF file cannot be read or is invalid.
pub(crate) fn execute(bot: PathBuf) -> Result<(), CliError> {
    let elf_bytes = fs::read(&bot).map_err(|e| {
        CliError::new(format!("Failed to read {}: {e}", bot.display()))
    })?;

    println!("Validating: {}", bot.display());
    println!();

    // Parse ELF header
    let elf = Elf::parse(&elf_bytes).map_err(|e| {
        CliError::new(format!("Failed to parse ELF: {e}"))
    })?;

    // Check machine type
    let machine_ok = elf.header.e_machine == goblin::elf::header::EM_RISCV;
    print_check("RISC-V architecture", machine_ok);
    if !machine_ok {
        return Err(CliError::new(format!(
            "Expected RISC-V (machine 243), got machine type {}",
            elf.header.e_machine
        )));
    }

    // Check 32-bit
    let bits_ok = !elf.is_64;
    print_check("32-bit ELF", bits_ok);
    if !bits_ok {
        return Err(CliError::new("Expected 32-bit ELF, got 64-bit"));
    }

    // Check little-endian
    let endian_ok = elf.little_endian;
    print_check("Little-endian", endian_ok);
    if !endian_ok {
        return Err(CliError::new("Expected little-endian ELF"));
    }

    // Check entry point
    let entry = elf.entry;
    let entry_ok = entry >= u64::from(TEXT_BASE) && entry < u64::from(TEXT_BASE) + 0x100_0000;
    print_check(&format!("Entry point ({entry:#x})"), entry_ok);
    if !entry_ok {
        return Err(CliError::new(format!(
            "Entry point {entry:#x} outside expected range ({TEXT_BASE:#x}..)"
        )));
    }

    // Try full load
    println!();
    print!("Full load test... ");
    match load_elf(&elf_bytes, 1024 * 1024, TEXT_BASE) {
        Ok((cpu, _memory)) => {
            println!("OK");
            println!();
            println!("Summary:");
            println!("  File size:    {} bytes", elf_bytes.len());
            println!("  Entry point:  {:#x}", cpu.pc);
            println!("  Stack top:    {:#x}", cpu.read_reg(2));
            if cpu.read_reg(3) != 0 {
                println!("  Global ptr:   {:#x}", cpu.read_reg(3));
            }
        }
        Err(e) => {
            println!("FAILED");
            return Err(e.into());
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
