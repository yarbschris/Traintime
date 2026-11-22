// 32 bit CRC
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

    // Test known value b"123456789" -> 0xCBF43926
    #[test]
    fn test_check_value() {
        let bytes = b"123456789";
        let expected: u32 = 0xCBF43926;
        assert_eq!(crc32(bytes), expected);
    }

    #[test]
    fn test_empty() {
        let bytes = b"";
        let expected: u32 = 0x00000000;
        assert_eq!(crc32(bytes), expected);
    }

    #[test]
    fn test_single_byte() {
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
