//! ELF loading for RISC-V programs.

use crate::vm::{Cpu, Memory};
use crate::TrapCause;
use goblin::elf::program_header::PT_LOAD;
use goblin::elf::Elf;

/// Standard RISC-V memory base address.
pub const TEXT_BASE: u32 = 0x8000_0000;

/// Stack top address (16 MiB from text base).
pub const STACK_TOP: u32 = 0x8100_0000;

/// Error type for ELF loading.
#[derive(Debug, Clone)]
pub struct ElfLoadError {
    /// Description of the error.
    pub reason: String,
}

impl std::fmt::Display for ElfLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ELF load error: {}", self.reason)
    }
}

impl std::error::Error for ElfLoadError {}

/// Load an ELF program into a fresh CPU and Memory.
///
/// Returns (Cpu with PC set to entry point, Memory with program loaded).
///
/// # Errors
///
/// Returns an error if the ELF is invalid, not RISC-V 32-bit, or segments
/// don't fit in memory.
pub fn load_elf(
    elf_bytes: &[u8],
    memory_size: u32,
    memory_base: u32,
) -> Result<(Cpu, Memory), ElfLoadError> {
    // Parse ELF header
    let elf = Elf::parse(elf_bytes).map_err(|e| ElfLoadError {
        reason: format!("Failed to parse ELF: {e}"),
    })?;

    // Validate it's a RISC-V 32-bit executable
    validate_elf_header(&elf)?;

    // Create fresh memory
    let mut memory = Memory::new(memory_size, memory_base);

    // Load program segments
    for phdr in &elf.program_headers {
        if phdr.p_type == PT_LOAD {
            load_segment(&mut memory, elf_bytes, phdr, memory_base, memory_size)?;
        }
    }

    // Create CPU with entry point
    let entry_point = u32::try_from(elf.entry).map_err(|_| ElfLoadError {
        reason: format!("Entry point {:#x} doesn't fit in u32", elf.entry),
    })?;

    let mut cpu = Cpu::with_pc(entry_point);

    // Set up stack pointer (x2/sp)
    cpu.write_reg(2, STACK_TOP);

    // Set up global pointer (x3/gp) if specified
    if let Some(gp) = find_global_pointer(&elf) {
        cpu.write_reg(3, gp);
    }

    Ok((cpu, memory))
}

/// Validate the ELF header for RISC-V 32-bit.
fn validate_elf_header(elf: &Elf) -> Result<(), ElfLoadError> {
    // Check machine type (RISC-V = 243)
    if elf.header.e_machine != goblin::elf::header::EM_RISCV {
        return Err(ElfLoadError {
            reason: format!(
                "Expected RISC-V ELF (machine {}), got machine type {}",
                goblin::elf::header::EM_RISCV,
                elf.header.e_machine
            ),
        });
    }

    // Check it's 32-bit
    if elf.is_64 {
        return Err(ElfLoadError {
            reason: "Expected 32-bit ELF, got 64-bit".to_string(),
        });
    }

    // Check it's little-endian
    if !elf.little_endian {
        return Err(ElfLoadError {
            reason: "Expected little-endian ELF".to_string(),
        });
    }

    Ok(())
}

/// Load a single program segment into memory.
fn load_segment(
    memory: &mut Memory,
    elf_bytes: &[u8],
    phdr: &goblin::elf::ProgramHeader,
    memory_base: u32,
    memory_size: u32,
) -> Result<(), ElfLoadError> {
    let vaddr = u32::try_from(phdr.p_vaddr).map_err(|_| ElfLoadError {
        reason: format!("Segment vaddr {:#x} doesn't fit in u32", phdr.p_vaddr),
    })?;

    let filesz = usize::try_from(phdr.p_filesz).map_err(|_| ElfLoadError {
        reason: format!("Segment filesz {} too large", phdr.p_filesz),
    })?;

    let memsz = u32::try_from(phdr.p_memsz).map_err(|_| ElfLoadError {
        reason: format!("Segment memsz {} doesn't fit in u32", phdr.p_memsz),
    })?;

    let offset = usize::try_from(phdr.p_offset).map_err(|_| ElfLoadError {
        reason: format!("Segment offset {} too large", phdr.p_offset),
    })?;

    // Validate address is within our memory region
    if vaddr < memory_base {
        return Err(ElfLoadError {
            reason: format!(
                "Segment at {vaddr:#x} below memory base {memory_base:#x}"
            ),
        });
    }

    let end_addr = vaddr.checked_add(memsz).ok_or_else(|| ElfLoadError {
        reason: format!("Segment at {vaddr:#x} size {memsz} overflows"),
    })?;

    let memory_end = memory_base.saturating_add(memory_size);
    if end_addr > memory_end {
        return Err(ElfLoadError {
            reason: format!(
                "Segment at {vaddr:#x} size {memsz} exceeds memory end {memory_end:#x}"
            ),
        });
    }

    // Copy file contents
    if filesz > 0 {
        if offset.saturating_add(filesz) > elf_bytes.len() {
            return Err(ElfLoadError {
                reason: format!(
                    "Segment file data at offset {offset} size {filesz} exceeds ELF size {}",
                    elf_bytes.len()
                ),
            });
        }

        let data = &elf_bytes[offset..offset + filesz];
        memory.store_bytes(vaddr, data).map_err(|e| {
            let reason = match e {
                TrapCause::MemoryFault { addr, access } => {
                    format!("Memory fault at {addr:#x} ({access:?})")
                }
                _ => format!("{e:?}"),
            };
            ElfLoadError { reason }
        })?;
    }

    // BSS (memsz > filesz) is already zero-initialized in Memory::new()

    Ok(())
}

/// Find the global pointer symbol value if present.
fn find_global_pointer(elf: &Elf) -> Option<u32> {
    for sym in &elf.syms {
        if let Some(name) = elf.strtab.get_at(sym.st_name) {
            if name == "__global_pointer$" {
                return u32::try_from(sym.st_value).ok();
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_elf_load_error_display() {
        let err = ElfLoadError {
            reason: "test error".to_string(),
        };
        assert_eq!(format!("{err}"), "ELF load error: test error");
    }

    #[test]
    fn test_invalid_elf_bytes() {
        let result = load_elf(&[0, 1, 2, 3], 65536, TEXT_BASE);
        assert!(result.is_err());
    }
}
