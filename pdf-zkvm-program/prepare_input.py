#!/usr/bin/env python3
"""
Prepare input data for PDF validation RISC-V program in Airbender format.
Usage: python prepare_input.py <pdf_file> [expected_text] > input.txt
"""

import sys

def main():
    if len(sys.argv) < 2:
        print(
            "Usage: python prepare_input.py <pdf_file> [expected_text]", file=sys.stderr
        )
        sys.exit(1)

    pdf_file = sys.argv[1]
    expected_text = sys.argv[2] if len(sys.argv) > 2 else ""

    # Read PDF file
    with open(pdf_file, "rb") as f:
        pdf_data = f.read()

    # Convert expected text to bytes
    expected_bytes = expected_text.encode("utf-8")

    # Create input data structure:
    # 1. PDF size (4 bytes, big-endian)
    # 2. PDF data
    # 3. Expected text size (4 bytes, big-endian)
    # 4. Expected text data
    # 5. Page number (4 bytes) - 0xFFFFFFFF means check all pages

    input_data = bytearray()

    # Add PDF size (4 bytes)
    input_data.extend(len(pdf_data).to_bytes(4, byteorder="big"))

    # Add PDF data
    input_data.extend(pdf_data)

    # Pad PDF data to multiple of 4 bytes
    while len(input_data) % 4 != 0:
        input_data.append(0)

    # Add expected text size (4 bytes)
    input_data.extend(len(expected_bytes).to_bytes(4, byteorder="big"))

    # Add expected text data
    input_data.extend(expected_bytes)

    # Add page number (0xFFFFFFFF = check all pages)
    input_data.extend(b"\xff\xff\xff\xff")

    # Pad to multiple of 4 bytes (since airbender reads 32-bit words)
    while len(input_data) % 4 != 0:
        input_data.append(0)

    # Convert to hex string (8 chars per 32-bit word)
    hex_words = []
    for i in range(0, len(input_data), 4):
        # Convert bytes to word in big-endian order
        word_bytes = input_data[i : i + 4]
        word = int.from_bytes(word_bytes, byteorder="big")
        hex_words.append(f"{word:08x}")

    # Print as single hex string
    print("".join(hex_words))


if __name__ == "__main__":
    main()
