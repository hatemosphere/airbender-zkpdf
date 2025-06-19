# PDF zkVM Program

RISC-V program for validating PDFs and extracting text within ZKsync's Airbender zero-knowledge virtual machine.

## Building

1. Install the RISC-V toolchain:
```bash
rustup target add riscv32im-unknown-none-elf
```

2. Build the program:
```bash
./build.sh
```

This creates `app.bin` - the RISC-V binary.

**Note**: The build script uses LLVM objcopy from the Rust toolchain to extract the raw binary from the ELF.

## Files

- `digitally_signed.pdf` - Sample digitally signed PDF
- `prepare_input.py` - Script to prepare input data for Airbender
- `build.sh` - Build script

## Running with Airbender

### 1. Prepare input data

```bash
# For signed PDF with text verification
python prepare_input.py digitally_signed.pdf "Sample Signed PDF Document" > input.txt

# For just PDF processing (no text check)
python prepare_input.py digitally_signed.pdf > input.txt
```

### 2. Run the program

```bash
# Run in emulator
../airbender/target/release/cli run --bin app.bin --input-file input.txt --cycles 9999999999999

# Generate proof
../airbender/target/release/cli prove --bin app.bin --input-file input.txt --output-dir output/

# Verify proof
../airbender/target/release/cli verify --proof-file output/proof.bin --public-input-file output/public_input.bin
```

## Input Format

The program expects input as a hex string with 8 characters per 32-bit word:
1. PDF file size (4 bytes, big-endian)
2. PDF data
3. Expected text size (4 bytes, big-endian) 
4. Expected text data (UTF-8)
5. Page number (4 bytes) - 0xFFFFFFFF means check all pages

## Output Format

The program returns 8 32-bit words:
- `result[0]`: Signature validity (1 = valid, 0 = invalid) or 0xFFFFFFFF for errors
- `result[1]`: Text found (1 = found, 0 = not found) or error code
- `result[2]`: Page where text was found (0-indexed)
- `result[3]`: Total number of pages
- `result[4]`: PDF size in bytes
- `result[5]`: First page text hash
- `result[6]`: Expected text size
- `result[7]`: Reserved

## Important Notes

### CSR 3072 Error

If you see `Machine IMStandardIsaConfig is not configured to support CSR number 3072`, this means your program panicked. Airbender doesn't support the RISC-V cycle CSR (0xC00) used by the default panic handler in `riscv_common`. 

To avoid this:
- Handle all errors gracefully without panicking
- Or patch `riscv_common` to avoid using the cycle CSR

### Memory Layout

- ROM: 0x0 - 2MB (code and read-only data)
- RAM: 2MB - 1024MB (stack, heap, and data)
- Heap size: ~800MB

### Limitations

- PDF files must fit in available memory (~800MB heap)
- No support for encrypted PDFs
- Limited font/encoding support
- RSA signature verification works but requires ~500M cycles