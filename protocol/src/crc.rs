//! CRC-32 for checking that a frame arrived intact.
//!
//! The variant is CRC-32/ISO-HDLC, the one used by zip and Ethernet:
//!
//! | Parameter     | Value                                  |
//! |---------------|----------------------------------------|
//! | polynomial    | `0x04C11DB7` (`0xEDB88320` reflected)  |
//! | init          | `0xFFFFFFFF`                           |
//! | reflected     | yes: bits are processed LSB first      |
//! | xorout        | `0xFFFFFFFF`                           |
//! | check value   | `0xCBF43926` for `b"123456789"`        |
//!
//! It gives the same result as Python's `zlib.crc32`, so frames captured off
//! the wire can be checked by hand.
//!
//! Computed bit by bit, with no lookup table: frames are small and arrive at
//! 115200 baud, so speed doesn't matter, and this skips the 1 KB table.

/// Length of the CRC in bytes. How it's placed in a frame is described in
/// [`crate::frame`].
pub const CRC_LEN: usize = 4;

const CRC_32_INITIAL_VALUE: u32 = 0xFFFFFFFF;
const CRC_32_REVERSED_POLYNOMIAL: u32 = 0xEDB88320;
const CRC_32_XOR_OUT: u32 = 0xFFFFFFFF;

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = CRC_32_INITIAL_VALUE;

    for byte in bytes {
        crc ^= *byte as u32;
        for _ in 0..8 {
            let bit = crc & 1;
            crc >>= 1;
            if bit == 1 {
                crc ^= CRC_32_REVERSED_POLYNOMIAL;
            }
        }
    }
    crc ^ CRC_32_XOR_OUT
}

#[cfg(test)]
mod tests {
    use super::*;

    // The published check value for CRC-32/ISO-HDLC. If this fails, one of
    // the parameters is wrong: shift direction, polynomial, init or xorout.
    #[test]
    fn check_value_matches_the_published_one() {
        let bytes = b"123456789";
        let expected: u32 = 0xCBF43926;
        assert_eq!(crc32(bytes), expected);
    }

    #[test]
    fn empty_input_is_zero() {
        // Init and xorout are the same, so with no bytes they cancel out.
        let bytes = b"";
        let expected: u32 = 0x00000000;
        assert_eq!(crc32(bytes), expected);
    }

    #[test]
    fn single_byte_matches_zlib() {
        // Reference value from Python: hex(zlib.crc32(b"a"))
        let bytes = b"a";
        let expected: u32 = 0xE8B7BE43;
        assert_eq!(crc32(bytes), expected);
    }

    #[test]
    fn flipping_any_single_bit_changes_the_crc() {
        // A CRC-32 catches every single-bit error, at any position. Checks
        // every bit of the input, so a bug that ignores some input bits
        // (e.g. skipping the last byte, or masking off bit 7) fails here.
        let original = *b"123456789";
        let original_crc = crc32(&original);

        for byte_index in 0..original.len() {
            for bit in 0..8 {
                let mut corrupted = original;
                corrupted[byte_index] ^= 1 << bit;

                assert_ne!(
                    crc32(&corrupted),
                    original_crc,
                    "flipping bit {bit} of byte {byte_index} went undetected"
                );
            }
        }
    }
}
