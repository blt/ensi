# Ensi Bot Build System
#
# This Makefile builds RISC-V RV32IM ELF binaries for the Ensi game.
#
# Requirements:
#   - RISC-V toolchain (riscv64-linux-gnu-gcc or riscv32-unknown-elf-gcc)
#
# Install on Ubuntu/Debian:
#   sudo apt install gcc-riscv64-linux-gnu
#
# Install on Arch:
#   sudo pacman -S riscv64-linux-gnu-gcc
#
# Usage:
#   make          # Build all bots
#   make clean    # Remove build artifacts
#   make test     # Verify ELF files with readelf

# Toolchain - default to riscv64-elf, override with CROSS_COMPILE=
CROSS_COMPILE ?= riscv64-elf-

CC = $(CROSS_COMPILE)gcc
OBJCOPY = $(CROSS_COMPILE)objcopy
READELF = $(CROSS_COMPILE)readelf

# Target: RV32IM with ILP32 ABI (32-bit integers, longs, and pointers)
ARCH = rv32im
ABI = ilp32

# Compiler flags
CFLAGS = -march=$(ARCH) -mabi=$(ABI) \
         -O2 -Wall -Wextra \
         -nostdlib -nostartfiles -ffreestanding \
         -fno-builtin -fno-stack-protector

# Linker flags
LDFLAGS = -Wl,-T,linker.ld -Wl,--gc-sections -static

# Memory layout (must match tournament config)
TEXT_BASE = 0x80000000
STACK_TOP = 0x80100000

# Source files
SRCS = aggressive.c economic.c balanced.c explorer.c
ELFS = $(SRCS:.c=.elf)

.PHONY: all clean test

all: linker.ld start.S $(ELFS)
	@echo "Built: $(ELFS)"

# Generate linker script
linker.ld:
	@echo "Generating linker script..."
	@echo "ENTRY(_start)" > $@
	@echo "MEMORY {" >> $@
	@echo "  RAM (rwx) : ORIGIN = $(TEXT_BASE), LENGTH = 1M" >> $@
	@echo "}" >> $@
	@echo "SECTIONS {" >> $@
	@echo "  .text : { *(.text.start) *(.text*) } > RAM" >> $@
	@echo "  .rodata : { *(.rodata*) } > RAM" >> $@
	@echo "  .data : { *(.data*) } > RAM" >> $@
	@echo "  .bss : { *(.bss*) *(COMMON) } > RAM" >> $@
	@echo "  /DISCARD/ : { *(.eh_frame*) *(.note*) *(.comment*) }" >> $@
	@echo "}" >> $@

# Generate startup assembly
start.S:
	@echo "Generating startup code..."
	@echo ".section .text.start" > $@
	@echo ".globl _start" >> $@
	@echo "_start:" >> $@
	@echo "    la sp, __stack_top" >> $@
	@echo "    call main" >> $@
	@echo "    # If main returns, loop forever" >> $@
	@echo "1:  j 1b" >> $@
	@echo "" >> $@
	@echo ".section .data" >> $@
	@echo ".align 4" >> $@
	@echo "__stack_top = $(STACK_TOP)" >> $@

# Compile bot to ELF
%.elf: %.c start.S linker.ld ensi.h
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ start.S $<
	@$(READELF) -h $@ | grep -q "RISC-V" && echo "  OK: $@" || echo "  FAIL: $@"

clean:
	rm -f $(ELFS) linker.ld start.S *.o

test: $(ELFS)
	@echo "Verifying ELF files..."
	@for elf in $(ELFS); do \
		echo "=== $$elf ==="; \
		$(READELF) -h $$elf | grep -E "(Class|Machine|Entry)"; \
	done
