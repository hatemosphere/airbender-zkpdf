#!/bin/bash
set -e

# Build the program
echo "Building RISC-V program..."
# Ensure we're using the correct linker scripts
export RUSTFLAGS="-C target-feature=+m,-unaligned-scalar-mem,+relax -C link-arg=-Tlds/memory.x -C link-arg=-Tlds/link.x -C link-arg=--save-temps -C force-frame-pointers"
cargo build --release --target riscv32im-unknown-none-elf

# Find LLVM tools - try rustup first, then system
OBJCOPY=$(find ~/.rustup -name "llvm-objcopy" 2>/dev/null | head -1)
OBJDUMP=$(find ~/.rustup -name "llvm-objdump" 2>/dev/null | head -1)

# If not found in rustup, try system paths
if [ -z "$OBJCOPY" ]; then
    OBJCOPY=$(which llvm-objcopy 2>/dev/null || which rust-objcopy 2>/dev/null)
fi

if [ -z "$OBJDUMP" ]; then
    OBJDUMP=$(which llvm-objdump 2>/dev/null || which rust-objdump 2>/dev/null)
fi

if [ -z "$OBJCOPY" ]; then
    echo "Error: llvm-objcopy not found in rustup toolchain or system PATH"
    echo "Trying to install rust-src component..."
    rustup component add rust-src
    # Try again after installing rust-src
    OBJCOPY=$(find ~/.rustup -name "llvm-objcopy" 2>/dev/null | head -1)
    if [ -z "$OBJCOPY" ]; then
        echo "Error: Still can't find llvm-objcopy"
        exit 1
    fi
fi

if [ -z "$OBJDUMP" ]; then
    echo "Warning: llvm-objdump not found, skipping ELF info"
    OBJDUMP=""
fi

# Extract binary from ELF
echo "Using LLVM objcopy: $OBJCOPY"
$OBJCOPY -O binary target/riscv32im-unknown-none-elf/release/pdf-zkvm-program app.bin

# Also copy the ELF for debugging
cp target/riscv32im-unknown-none-elf/release/pdf-zkvm-program app.elf

# Display ELF information if objdump is available
if [ -n "$OBJDUMP" ]; then
    echo ""
    echo "ELF Information:"
    echo "================"

    # Show section headers
    echo "Section headers:"
    $OBJDUMP -h app.elf | grep -E "Idx|\.text|\.rodata|\.data|\.bss|ALLOC"

    echo ""
    echo "Program headers:"
    $OBJDUMP -p app.elf | grep -E "LOAD|Entry"

    # Show binary size
    echo ""
    echo "Binary sizes:"
    ls -lh app.bin app.elf | awk '{print $9 ": " $5}'
fi

echo ""
echo "Build complete!"
echo "  Binary: app.bin"
echo "  ELF:    app.elf"
