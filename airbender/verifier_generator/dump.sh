#!/bin/sh
#cargo run -p generator -- --output-dir=../tools/generator/output
#cp ../tools/generator/output/blake_generated.rs ../verifier/src/concrete/blake_generated.rs
#cp ../tools/generator/output/blake_generated_inlined_verifier.rs ../verifier/src/concrete/blake_generated_inlined_verifier.rs
# cp ../tools/generator/output/riscv_generated.rs ../verifier/src/concrete/generated.rs
# cp ../tools/generator/output/riscv_generated_inlined_verifier.rs ../verifier/src/concrete/generated_inlined_verifier.rs

cargo test -- test::launch --exact
cargo test -- test::launch_inlining --exact
rustfmt src/generated.rs
rustfmt src/generated_inlined_verifier.rs
cp src/generated.rs ../verifier/src/generated/circuit_layout.rs
cp src/generated_inlined_verifier.rs ../verifier/src/generated/quotient.rs
