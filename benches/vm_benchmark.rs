//! Benchmarks for the RISC-V VM.

#![allow(missing_docs)] // Benchmark macros generate undocumented functions
#![allow(clippy::unreadable_literal)] // Instruction encodings are standard hex

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use ensi::{NoSyscalls, Vm};

fn bench_step(c: &mut Criterion) {
    let mut vm = Vm::new(65536, 0, NoSyscalls);

    // Fill memory with addi instructions (simple loop)
    // addi x1, x1, 1
    let addi_x1 = 0x00108093u32;
    for i in 0..(65536 / 4) {
        let _ = vm.memory.store_u32(i * 4, addi_x1);
    }

    c.bench_function("step_addi", |b| {
        b.iter(|| {
            // Reset PC
            vm.cpu.pc = 0;
            for _ in 0..1000 {
                let _ = black_box(vm.step());
            }
        });
    });
}

fn bench_run_turn(c: &mut Criterion) {
    let mut vm = Vm::new(65536, 0, NoSyscalls);

    // Fill memory with addi instructions
    let addi_x1 = 0x00108093u32;
    for i in 0..(65536 / 4) {
        let _ = vm.memory.store_u32(i * 4, addi_x1);
    }

    c.bench_function("run_turn_10k", |b| {
        b.iter(|| {
            vm.cpu.pc = 0;
            let _ = black_box(vm.run_turn(10_000));
        });
    });
}

fn bench_decode(c: &mut Criterion) {
    use ensi::isa::decode;

    // Sample instructions
    let instructions = [
        0x00108093u32, // addi x1, x1, 1
        0x002081B3u32, // add x3, x1, x2
        0x00208463u32, // beq x1, x2, 8
        0x0000006Fu32, // jal x0, 0
        0x02A00093u32, // addi x1, x0, 42
    ];

    c.bench_function("decode_1000", |b| {
        b.iter(|| {
            for _ in 0..200 {
                for inst in &instructions {
                    let _ = black_box(decode(*inst));
                }
            }
        });
    });
}

criterion_group!(benches, bench_step, bench_run_turn, bench_decode);
criterion_main!(benches);
