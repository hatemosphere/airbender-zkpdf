#![no_std]
#![allow(incomplete_features)]
#![feature(allocator_api)]
#![feature(generic_const_exprs)]
#![no_main]
#![no_builtins]

extern crate alloc;

mod uart_logger;

use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::fmt::Write;
use core::panic::PanicInfo;
use linked_list_allocator::Heap;
use riscv_common::{csr_read_word, zksync_os_finish_success, QuasiUART};

// Allocator
struct SimpleAllocator;
static mut HEAP: Heap = Heap::empty();

unsafe impl GlobalAlloc for SimpleAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: We're in a single-threaded environment and this is the only place accessing HEAP during allocation
        let heap_ref = unsafe { &mut *core::ptr::addr_of_mut!(HEAP) };
        heap_ref
            .allocate_first_fit(layout)
            .ok()
            .map_or(core::ptr::null_mut(), |allocation| allocation.as_ptr())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: We're in a single-threaded environment and this is the only place accessing HEAP during deallocation
        let heap_ref = unsafe { &mut *core::ptr::addr_of_mut!(HEAP) };
        heap_ref.deallocate(core::ptr::NonNull::new_unchecked(ptr), layout)
    }
}

#[global_allocator]
static ALLOCATOR: SimpleAllocator = SimpleAllocator;

// Standard panic handler using rust_abort
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    riscv_common::rust_abort()
}

extern "C" {
    static _sheap: u8;
    static _eheap: u8;
    static _sstack: u8;
    static _estack: u8;
}

core::arch::global_asm!(include_str!(
    "../../airbender/examples/scripts/asm/asm_reduced.S"
));

#[no_mangle]
extern "C" fn eh_personality() {}

#[link_section = ".init.rust"]
#[export_name = "_start_rust"]
unsafe extern "C" fn start_rust() -> ! {
    main()
}

#[export_name = "_setup_interrupts"]
/// # Safety
/// This function must be called only once during system initialization.
/// It directly manipulates the trap vector table.
pub unsafe fn custom_setup_interrupts() {
    extern "C" {
        fn _machine_start_trap();
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct MachineTrapFrame {
    pub registers: [u32; 32],
}

#[link_section = ".trap.rust"]
#[export_name = "_machine_start_trap_rust"]
pub extern "C" fn machine_start_trap_rust(_trap_frame: *mut MachineTrapFrame) -> usize {
    unsafe { core::hint::unreachable_unchecked() }
}

// Input structure:
// - PDF file size (4 bytes)
// - PDF data (variable)
// - Expected text size (4 bytes)
// - Expected text (variable)
// - Page number to check (4 bytes) - optional, 0xFFFFFFFF means check all pages

unsafe fn workload() -> ! {
    // Create UART for debugging
    let mut uart = QuasiUART::new();
    let _ = write!(uart, "Starting PDF zkVM program...");

    // Initialize logger for the library
    #[cfg(feature = "debug")]
    pdf_utils_zkvm_core::set_logger(&uart_logger::UART_LOGGER);

    // Initialize heap
    let heap_start = &_sheap as *const u8 as usize;
    let heap_end = &_eheap as *const u8 as usize;
    let heap_size = heap_end - heap_start;

    let _ = write!(
        uart,
        "Heap: start={heap_start:x}, end={heap_end:x}, size={heap_size}"
    );

    // Debug: return heap info
    if heap_size == 0 || heap_start >= heap_end {
        zksync_os_finish_success(&[
            0xDEAD0001,
            heap_start as u32,
            heap_end as u32,
            heap_size as u32,
            0,
            0,
            0,
            0,
        ]);
    }

    // SAFETY: We're in a single-threaded environment and this is the only initialization of HEAP
    let heap_ref = unsafe { &mut *core::ptr::addr_of_mut!(HEAP) };
    heap_ref.init(heap_start as *mut u8, heap_size);

    // Read input size (first word)
    let input_size_word = csr_read_word();
    let input_size = input_size_word as usize;

    let _ = write!(uart, "PDF size: {input_size} bytes");

    // Debug: show raw word and converted size
    if input_size > 10_000_000 || input_size == 0 {
        // Error: invalid input size
        // Return error code 1 with raw word and converted size
        zksync_os_finish_success(&[
            0xFFFFFFFF,
            1,
            input_size_word,
            input_size as u32,
            0,
            0,
            0,
            0,
        ]);
    }

    // Read PDF data word by word
    let mut pdf_data = Vec::with_capacity(input_size);
    let words_to_read = input_size.div_ceil(4); // Round up to next word

    for _ in 0..words_to_read {
        let word = csr_read_word();
        // Extract bytes from word (big-endian order to match hex string)
        if pdf_data.len() < input_size {
            pdf_data.push(((word >> 24) & 0xFF) as u8);
        }
        if pdf_data.len() < input_size {
            pdf_data.push(((word >> 16) & 0xFF) as u8);
        }
        if pdf_data.len() < input_size {
            pdf_data.push(((word >> 8) & 0xFF) as u8);
        }
        if pdf_data.len() < input_size {
            pdf_data.push((word & 0xFF) as u8);
        }
    }
    pdf_data.truncate(input_size);

    let pdf_len = pdf_data.len();
    let _ = write!(uart, "Read {pdf_len} bytes of PDF data");

    // Validate minimum input size
    if input_size < 10 {
        let _ = write!(uart, "Error: Input too small");
        riscv_common::zksync_os_finish_error();
    }

    // Debug: Check PDF header
    if pdf_data.len() >= 8 {
        let header_valid = &pdf_data[0..4] == b"%PDF";
        let header = &pdf_data[0..8];
        let _ = write!(uart, "PDF header: {header:?}");
        if !header_valid {
            // Bad PDF header
            zksync_os_finish_success(&[
                0xFFFFFFFF,
                5,
                pdf_data[0] as u32,
                pdf_data[1] as u32,
                pdf_data[2] as u32,
                pdf_data[3] as u32,
                0,
                0,
            ]);
        }
    }

    // Read expected text size
    let expected_text_size_word = csr_read_word();
    let expected_text_size = expected_text_size_word as usize;
    let _ = write!(
        uart,
        "Expected text size word: 0x{expected_text_size_word:08x} = {expected_text_size}"
    );

    // Read expected text if provided
    let expected_text = if expected_text_size > 0 {
        let mut text = Vec::with_capacity(expected_text_size);
        let words_to_read = expected_text_size.div_ceil(4);

        for _ in 0..words_to_read {
            let word = csr_read_word();
            // Extract bytes in big-endian order to match hex string
            if text.len() < expected_text_size {
                text.push(((word >> 24) & 0xFF) as u8);
            }
            if text.len() < expected_text_size {
                text.push(((word >> 16) & 0xFF) as u8);
            }
            if text.len() < expected_text_size {
                text.push(((word >> 8) & 0xFF) as u8);
            }
            if text.len() < expected_text_size {
                text.push((word & 0xFF) as u8);
            }
        }
        text.truncate(expected_text_size);
        Some(text)
    } else {
        None
    };

    // Read page number (kept for backward compatibility, but ignored)
    let _page_number_word = csr_read_word();
    // Always check all pages like the reference implementation

    let _ = write!(uart, "Starting PDF validation...");

    // First check if this is a signed PDF
    let has_byterange = pdf_data.windows(10).any(|w| w == b"/ByteRange");
    let has_contents = pdf_data.windows(9).any(|w| w == b"/Contents");
    let _ = write!(
        uart,
        "Has ByteRange: {has_byterange}, Has Contents: {has_contents}"
    );

    // If we found /ByteRange, let's see where it is
    if has_byterange {
        if let Some(br_pos) = pdf_data.windows(10).position(|w| w == b"/ByteRange") {
            let _ = write!(uart, "/ByteRange found at position: {br_pos}");

            // Look for the array values after /ByteRange
            if let Some(bracket_start) = pdf_data[br_pos + 10..].iter().position(|&b| b == b'[') {
                let bracket_start = br_pos + 10 + bracket_start;
                if let Some(bracket_end) = pdf_data[bracket_start..].iter().position(|&b| b == b']')
                {
                    let bracket_end = bracket_start + bracket_end;
                    if let Ok(range_str) =
                        core::str::from_utf8(&pdf_data[bracket_start + 1..bracket_end])
                    {
                        let _ = write!(uart, "ByteRange values: {range_str}");

                        // Parse the values
                        let parts: Vec<&str> = range_str.split_whitespace().collect();
                        if parts.len() == 4 {
                            if let (Ok(offset1), Ok(length1), Ok(offset2), Ok(_length2)) = (
                                parts[0].parse::<usize>(),
                                parts[1].parse::<usize>(),
                                parts[2].parse::<usize>(),
                                parts[3].parse::<usize>(),
                            ) {
                                // Check where /Contents should be
                                let sig_start = offset1 + length1;
                                let sig_end = offset2;
                                let _ = write!(uart, "Signature range: {sig_start} to {sig_end}");

                                // Look for /Contents in that range
                                if sig_end > sig_start && sig_end <= pdf_data.len() {
                                    let sig_range = &pdf_data[sig_start..sig_end];
                                    let contents_in_range =
                                        sig_range.windows(9).any(|w| w == b"/Contents");
                                    let _ = write!(
                                        uart,
                                        "/Contents in signature range: {contents_in_range}"
                                    );

                                    // Print first 100 bytes of signature range
                                    let preview_len = core::cmp::min(100, sig_range.len());
                                    if let Ok(preview) =
                                        core::str::from_utf8(&sig_range[0..preview_len])
                                    {
                                        let _ = write!(uart, "Signature range preview: {preview}");
                                    }

                                    // Look for /Contents before the ByteRange
                                    if br_pos > 100 {
                                        let before_range = &pdf_data[br_pos - 100..br_pos];
                                        if let Some(contents_pos) =
                                            before_range.windows(9).position(|w| w == b"/Contents")
                                        {
                                            let _ = write!(
                                                uart,
                                                "/Contents found {} bytes before /ByteRange",
                                                100 - contents_pos
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Try to validate signature and extract text
    let _ = write!(uart, "Starting signature validation...");
    let signature_valid = match pdf_utils_zkvm_core::verify_pdf_signature(&pdf_data) {
        Ok(valid) => {
            let _ = write!(uart, "Signature validation result: {valid}");
            valid
        }
        Err(e) => {
            let _ = write!(uart, "Signature validation error: {e}");
            false
        }
    };
    let _ = write!(uart, "Signature validation complete");

    // Extract text regardless of signature validation result
    let result = match pdf_utils_zkvm_core::extract_text(pdf_data.clone()) {
        Ok(text_pages) => {
            let _ = write!(
                uart,
                "Text extraction successful! {} pages",
                text_pages.len()
            );
            pdf_utils_zkvm_core::PdfValidationResult {
                signature_valid,
                text_pages,
            }
        }
        Err(e) => {
            let _ = write!(uart, "Text extraction failed: {e}");

            // Let's examine the PDF structure near the end to debug
            if pdf_data.len() > 100 {
                let end_preview = &pdf_data[pdf_data.len() - 100..];
                if let Ok(preview_str) = core::str::from_utf8(end_preview) {
                    let _ = write!(uart, "PDF end preview: {preview_str}");
                }
            }
            // Return error code 2: PDF validation/extraction failed
            // Include a hash of the error message for debugging
            let error_hash = {
                let mut hash = 0u32;
                for (i, byte) in alloc::format!("{e}").bytes().enumerate() {
                    if i >= 16 {
                        break;
                    } // Only hash first 16 bytes
                    hash = hash.rotate_left(3) ^ (byte as u32);
                }
                hash
            };
            zksync_os_finish_success(&[
                0xFFFFFFFF,
                2,
                error_hash,
                pdf_data.len() as u32,
                0,
                0,
                0,
                0,
            ]);
        }
    };

    // Check signature validity
    let sig_valid = if result.signature_valid { 1u32 } else { 0u32 };

    // Check if extracted text contains expected text
    let (text_found, page_found) = if let Some(expected_bytes) = expected_text {
        let _ = write!(
            uart,
            "Checking for expected text of {} bytes",
            expected_bytes.len()
        );
        let expected_str = match core::str::from_utf8(&expected_bytes) {
            Ok(s) => s,
            Err(_) => {
                // Error: invalid UTF-8 in expected text
                // Return error code 3: invalid UTF-8
                zksync_os_finish_success(&[0xFFFFFFFF, 3, 0, 0, 0, 0, 0, 0])
            }
        };

        let _ = write!(uart, "Expected text: '{expected_str}'");

        // Always check all pages like the reference implementation
        let mut found = false;
        let mut found_page = 0u32;
        for (idx, page_text) in result.text_pages.iter().enumerate() {
            let _ = write!(uart, "Page {idx} text: '{page_text}'");
            let trimmed = page_text.trim();
            let _ = write!(uart, "  Trimmed: '{trimmed}'");
            let text_len = page_text.len();
            let preview_bytes = &page_text.as_bytes()[..core::cmp::min(50, page_text.len())];
            let _ = write!(uart, "  Length: {text_len}, bytes: {preview_bytes:?}");

            // Debug: Check if all characters are spaces
            let all_spaces = page_text.chars().all(|c| c == ' ');
            let _ = write!(uart, "  All spaces: {all_spaces}");

            if page_text.contains(expected_str) {
                found = true;
                found_page = idx as u32;
                break;
            }
        }
        (if found { 1u32 } else { 0u32 }, found_page)
    } else {
        // No text to check
        (1u32, 0u32)
    };

    // Calculate a hash of the first page text (for proof of content)
    let first_page_hash = if !result.text_pages.is_empty() {
        let first_page = &result.text_pages[0];
        let mut hash = 0u32;
        for byte in first_page.bytes().take(32) {
            hash = hash.rotate_left(7) ^ (byte as u32);
        }
        hash
    } else {
        0u32
    };

    // Return comprehensive results
    // result[0] = signature valid (1) or not (0)
    // result[1] = text found (1) or not (0)
    // result[2] = page where text was found (or 0)
    // result[3] = total number of pages
    // result[4] = PDF size
    // result[5] = first page text hash
    // result[6] = expected text size (for verification)
    // result[7] = reserved for future use
    let num_pages = result.text_pages.len() as u32;

    let _ = write!(
        uart,
        "Success! Sig={sig_valid}, TextFound={text_found}, Page={page_found}, NumPages={num_pages}"
    );

    zksync_os_finish_success(&[
        sig_valid,
        text_found,
        page_found,
        num_pages,
        input_size as u32,
        first_page_hash,
        expected_text_size as u32,
        0,
    ]);
}

#[inline(never)]
fn main() -> ! {
    unsafe { workload() }
}
