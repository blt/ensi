# Ensi Bot SDK Makefile
#
# Build WASM bots from C source files.
#
# Usage:
#   make                 - Build all bots
#   make example_bot.wasm - Build specific bot
#   make clean           - Remove build artifacts

# Compiler settings
CC = clang
WASM_TARGET = wasm32-unknown-unknown
CFLAGS = --target=$(WASM_TARGET) -O3 -nostdlib -flto
LDFLAGS = -Wl,--no-entry -Wl,--export=run_turn -Wl,--lto-O3 -Wl,--allow-undefined

# Alternative: Use wasi-sdk for more libc support
# CC = /opt/wasi-sdk/bin/clang
# WASM_TARGET = wasm32-wasi
# CFLAGS = --target=$(WASM_TARGET) -O3
# LDFLAGS =

# Source files
SOURCES = $(wildcard *.c)
WASMS = $(SOURCES:.c=.wasm)

# Default target
all: $(WASMS)

# Build WASM from C
%.wasm: %.c ensi.h
	$(CC) $(CFLAGS) $(LDFLAGS) -o $@ $<

# Clean build artifacts
clean:
	rm -f *.wasm

# Check if clang supports wasm target
check:
	@echo "Checking clang WASM support..."
	@$(CC) --target=$(WASM_TARGET) -v 2>&1 | head -1 || echo "WASM target not supported"

.PHONY: all clean check
