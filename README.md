# zkpdf freestyle implementation attempt for ZKsync Airbender

Original SP1 implementation: https://github.com/privacy-scaling-explorations/zkpdf

The code in this repo has dangerous amount of vibe-coding applied by script kiddie with no proper Rust or math knowledge, or understanding what he is doing whatsoever, hence only serves to present a demo case for zkync-airbender. It was tested to work only with example `digitally_signed.pdf` from original implementation (included in this repo), so other PDF files might not even work.

Some of the RISC-V limitations, and crates replacements due to them, led to rewriting significant chunks of PDF parsing logic, so expect lots of bugs coming out of this hell of hex numbers.

### Running with Airbender
```bash
# Build airbender CLI (if not already built)
cd airbender
cargo build --release --features include_verifiers

# Build the program
cd ../pdf-zkvm-program
./build.sh

# Prepare input (use airbender-specific formatter)
python prepare_input.py digitally_signed.pdf "Sample Signed PDF Document" > input.txt

# Run in emulator
../airbender/target/release/cli run --bin app.bin --input-file input.txt --cycles 9999999999999

# Generate base proof (fast options)
../airbender/target/release/cli prove --bin app.bin --input-file input.txt --output-dir output/ --machine reduced

# Generate full final proof (requires ~128GB RAM!)
../airbender/target/release/cli prove --bin app.bin --input-file input.txt --output-dir output/ --until final-proof --tmp-dir /fast-ssd/proving

# Generate complete SNARK (requires zkos_wrapper + 128GB+ RAM)
../airbender/target/release/cli prove --bin app.bin --input-file input.txt --output-dir output/ --until snark --tmp-dir /fast-ssd/proving

# Verify proof
../airbender/target/release/cli verify --proof-file output/proof.bin --public-input-file output/public_input.bin
```

**Note on Verifier Compilation:**
If you encounter "verifier not found" errors when running `verify`, you need to compile with verifiers included:
```bash
cd ../airbender
cargo build --release --features include_verifiers
```

### Program Output Format

The program returns 8 32-bit values via `zksync_os_finish_success(&[...])`, but Airbender may display up to 16 values (with padding). The output format is:

```
Result: result[0], result[1], result[2], result[3], result[4], result[5], result[6], result[7], [padding...]
```

**Output fields:**
- `result[0]`: **Signature validity** (0=invalid/not checked, 1=valid)
- `result[1]`: **Text found** (0=not found, 1=found)
- `result[2]`: **Page number** where text was found (0-indexed)
- `result[3]`: **Total page count** in the PDF
- `result[4]`: **PDF size** in bytes
- `result[5]`: **First page text hash** (32-bit hash for content verification)
- `result[6]`: **Expected text size** in bytes (for verification)
- `result[7]`: Reserved for future use (currently 0)

**Error codes (when result[0] = 0xFFFFFFFF):**
- `result[1] = 1`: Invalid input size
- `result[1] = 2`: PDF validation/extraction failed (result[2] contains error hash)
- `result[1] = 3`: Invalid UTF-8 in expected text
- `result[1] = 5`: Bad PDF header

**Example successful output:**
```
Result: 1, 1, 0, 1, 272318, 10328225, 26, 0, 0, 0, 0, 0, 0, 0, 0, 0
```
Means: Signature valid (1), text found (1), on page 0, 1 page total, PDF is 272KB, with text hash 10328225, expected text size 26 bytes

**Note:** Airbender may display up to 16 values (64 bytes of output data), but only the first 8 are meaningful (the extra zeros are padding?).

### Development Tips

- Use `QuasiUART` for debugging in zkVM (feature = "uart")
- Test with reference implementation first before zkVM
- Monitor cycle count - default limit is 32M cycles
- Use `--cycles 9999999999999` for development to avoid cycle limits
- Build with `--release` for better performance

### Debugging with ELF File

The build process generates both `app.bin` (raw binary) and `app.elf` (with debug symbols):

```bash
# Analyze ELF sections and entry point
llvm-objdump -h app.elf
llvm-objdump -p app.elf

# Disassemble specific functions
llvm-objdump -d app.elf | grep -A 20 "<main>:"

# View all symbols
llvm-nm app.elf

# Check binary size and sections
llvm-size app.elf

# Use with GDB for RISC-V (if available)
riscv32-unknown-elf-gdb app.elf
```

The ELF file is useful for:
- Analyzing memory layout and section sizes
- Debugging crashes by mapping addresses to symbols
- Understanding the compiled code structure
- Verifying optimization and linking

## Project structure

The project includes two separate Rust programs for RISC-V zkVM:

### pdf-utils-zkvm/ (Library)
A no_std library providing PDF utilities for RISC-V zkVM environments:
- **signature-validator**: no_std PDF signature validation using der crate instead of simple_asn1
- **extractor**: no_std PDF text extraction using BTreeMap instead of HashMap
- **core**: Combined validation and extraction API

This is a **library crate** that provides the core PDF processing functionality. It's designed to be reusable and **IN THEORY** can be imported by any RISC-V zkVM program.

### pdf-zkvm-program/ (Binary)
The actual RISC-V program binary for PDF validation within ZKsync Airbender zkVM:
- Uses `pdf-utils-zkvm` as a dependency for PDF processing
- Implements Airbender-specific I/O (CSR registers, QuasiUART)
- Handles input parsing and output formatting for the zkVM
- Input: PDF bytes + optional expected text
- Output: Signature validity, text presence, page count
- Build: `./build.sh` (requires riscv32im-unknown-none-elf target)

This is a **binary crate** that produces the actual RISC-V executable that runs in Airbender:
- `app.bin`: Raw binary for zkVM execution
- `app.elf`: ELF file with debug symbols for analysis

## Key Implementation Details for RISC-V zkVM Compatibility

#### 1. **Custom Allocator Required**
Airbender's `riscv_common` only provides a `NullAllocator` that panics on any allocation. To use heap allocation I had to:
- Enable the `custom_allocator` feature in riscv_common
- Use `linked_list_allocator` crate (v0.10) which provides a no_std heap without atomics
- Wrap `linked_list_allocator::Heap` (not `LockedHeap` which uses atomics) in a `GlobalAlloc` impl
- Initialize the heap with `HEAP.init()` at program start

#### 2. **Custom Panic Handler and CSR 3072 Error**
Airbender uses CSR 3072 (cycle counter) for panic handling, which causes "Machine IMStandardIsaConfig is not configured to support CSR number 3072" errors:
- CSR 3072 is the "cycle" CSR in RISC-V, read-only by design
- The canonical panic opcode in RISC-V attempts to write to this read-only CSR
- `rust_abort()` → `zksync_os_finish_error()` → uses cycle CSR (3072)
- This error is **expected** when the program panics - it's how Airbender implements panics
- Use `zksync_os_finish_success(&[...])` for controlled program termination with results
- Override panic handler with `#[panic_handler]` to use `rust_abort()` for consistent behavior

#### 3. **Input Format**
Airbender expects input as a hex string with specific formatting:
- Each 32-bit word is represented as 8 hex characters
- Words are read using `csr_read_word()` from CSR 0x7C0
- Data is packed in **big-endian order within the hex string** (MSB first)
- The `prepare_input.py` script handles this formatting:
  ```python
  # Convert bytes to 32-bit words (big-endian for hex string)
  for i in range(0, len(padded_data), 4):
      word = struct.unpack('>I', padded_data[i:i+4])[0]  # Big-endian
      hex_output += f"{word:08x}"  # 8 hex chars per word
  ```
- PDF data must be padded to 4-byte boundary before appending other data
- Input structure: [PDF size][PDF data][expected text size][expected text][page number]

#### 4. **no_std Replacements**
Required changes for RISC-V zkVM compatibility:
- `simple_asn1` → ported to no_std: simple_asn1 uses HashMap internally which requires RandomState/atomics, so it had to be forked stripping `std` and replacing `num-bigint` with our forked `crypto-bigint`
- `HashMap/HashSet` → `BTreeMap/BTreeSet`: HashMap's default hasher (RandomState) requires atomic operations
- `std::error::Error` → `alloc::string::String`: Error trait is not available in no_std environments
- Even though `RustCrypto/RSA` replaced `num-bigint` with `no_std` and no heap `crypto-bigint`, the latter still uses `Arc<[Word]>` for heap allocation, and `Arc` requires atomics, so I patched it to replace `Arc` with `Rc` (inspired by rustls's atomic-free approach)
- RSA v0.10.0-rc.0's `Pkcs1v15Sign::new::<D>()` method requires the hash algorithm type D to implement the `AssociatedOid` trait. This trait provides the OID (Object Identifier) for the hash algorithm, which is used to construct the DigestInfo structure. In the reference implementation (which uses std library), the SHA hash types from the sha1 and sha2 crates implement `AssociatedOid` when certain features are enabled. However, in our no_std environment, these implementations are not available because:

  1. The AssociatedOid trait comes from the pkcs8 crate
  2. The SHA implementations only provide AssociatedOid when the oid feature is enabled
  3. In our no_std configuration, these features aren't properly connected

  That's why we got the error:
  ```
  error[E0277]: the trait bound `Sha1: rsa::pkcs8::AssociatedOid` is not satisfied
  ```

  To work around this, I manually constructed the DigestInfo structure by:
  1. Defining the ASN.1 DER prefixes for each hash algorithm (from RFC 3447)
  2. Concatenating the prefix with the raw hash
  3. Using `Pkcs1v15Sign::new_unprefixed()` which expects the complete DigestInfo

  This in theory achieves the same result as `new::<Sha1>()` but without requiring the `AssociatedOid` trait.

**Feature flags for RISC-V compatibility:**

Key changes:
- Disabled `std` features across all crates
- Enabled only `alloc` where needed
- Used `hashbrown` instead of std HashMap (but with BTreeMap for determinism)
- Avoided features that pull in atomic dependencies

### Current Limitations

1. **RSA Verification**:
   - RSA signature verification requires hundreds of millions of cycles
   - Modular exponentiation is computationally intensive
   - May require optimizations or alternative approaches

2. **Limited PDF Support**:
   - Basic PDF structure parsing only
   - No support for encrypted PDFs
   - Limited font encoding support compared to full PDF libraries

3. **Certificate Storage**:
   - Only handles certificates embedded in PKCS#7 structure
   - No support for external certificate references
   - Some PDFs may store certificates separately
