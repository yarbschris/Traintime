// COBS encodes content of src into out
pub fn cobs_encode(src: &[u8], out: &mut [u8]) -> Option<usize> {
    // Current index into out array
    let mut curr_index: usize = 0;
    // Out index holding the last encoding byte
    let mut last_encoding_byte: usize;

    // Start with an encoding byte
    (last_encoding_byte, curr_index) = add_encoding_byte(out, curr_index)?;

    // Then, copy src into out, putting encoding byte at the beginning and after
    // 254 consecutive non-encoding bytes
    for byte in src {
        // 254 consecutive non-encoding bytes, need to add a zero
        if curr_index - last_encoding_byte == 255 {
            (last_encoding_byte, curr_index) =
                update_last_encoding_and_add_encoding_byte(out, curr_index, last_encoding_byte)?;
        }

        if *byte == ENCODING_BYTE {
            (last_encoding_byte, curr_index) =
                update_last_encoding_and_add_encoding_byte(out, curr_index, last_encoding_byte)?;
        } else {
            curr_index = copy_byte(out, *byte, curr_index)?;
        }
    }
    // Finally, append a zero and set the last encoding byte to point to end of packet
    (_, curr_index) =
        update_last_encoding_and_add_encoding_byte(out, curr_index, last_encoding_byte)?;

    Some(curr_index)
}

// Sets out[last_encoding_byte] to point to the current index, then appends an encoding byte
// returns (index of new encoding byte, next index in array) or None if either index is out of bounds
fn update_last_encoding_and_add_encoding_byte(
    out: &mut [u8],
    curr_index: usize,
    last_encoding_byte: usize,
) -> Option<(usize, usize)> {
    *out.get_mut(last_encoding_byte)? = (curr_index - last_encoding_byte) as u8;
    add_encoding_byte(out, curr_index)
}

// Add encoding byte to out array at index,
// returns tuple of (encoding_index, incremented index), or None on failure
fn add_encoding_byte(out: &mut [u8], index: usize) -> Option<(usize, usize)> {
    Some((index, copy_byte(out, ENCODING_BYTE, index)?))
}

// Copy byte to out, return incremented index
fn copy_byte(out: &mut [u8], byte: u8, index: usize) -> Option<usize> {
    *out.get_mut(index)? = byte;
    Some(index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::iter::once;

    /// Encodes into a buffer sized for the worst case and returns just the
    /// bytes that were written.
    fn encode(src: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; src.len() + src.len() / 254 + 2];
        let len = cobs_encode(src, &mut out).expect("buffer is sized for the worst case");
        out.truncate(len);
        out
    }

    // Examples 1-10 below are the worked examples from the Wikipedia COBS
    // article, with the trailing 00 delimiter included.

    #[test]
    fn empty_input() {
        assert_eq!(encode(&[]), [0x01, 0x00]);
    }

    #[test]
    fn single_zero() {
        assert_eq!(encode(&[0x00]), [0x01, 0x01, 0x00]);
    }

    #[test]
    fn two_zeros() {
        assert_eq!(encode(&[0x00, 0x00]), [0x01, 0x01, 0x01, 0x00]);
    }

    #[test]
    fn zero_on_both_ends() {
        assert_eq!(encode(&[0x00, 0x11, 0x00]), [0x01, 0x02, 0x11, 0x01, 0x00]);
    }

    #[test]
    fn zero_in_the_middle() {
        assert_eq!(
            encode(&[0x11, 0x22, 0x00, 0x33]),
            [0x03, 0x11, 0x22, 0x02, 0x33, 0x00]
        );
    }

    #[test]
    fn no_zeros() {
        assert_eq!(
            encode(&[0x11, 0x22, 0x33, 0x44]),
            [0x05, 0x11, 0x22, 0x33, 0x44, 0x00]
        );
    }

    #[test]
    fn trailing_zeros() {
        assert_eq!(
            encode(&[0x11, 0x00, 0x00, 0x00]),
            [0x02, 0x11, 0x01, 0x01, 0x01, 0x00]
        );
    }

    #[test]
    fn exactly_254_non_zero_bytes_fill_one_block() {
        // A full block gets code 0xFF and no extra code byte after it.
        let src: Vec<u8> = (0x01..=0xFE).collect();
        let expected: Vec<u8> = once(0xFF).chain(0x01..=0xFE).chain(once(0x00)).collect();
        assert_eq!(encode(&src), expected);
    }

    #[test]
    fn leading_zero_then_254_non_zero_bytes() {
        let src: Vec<u8> = (0x00..=0xFE).collect();
        let expected: Vec<u8> = [0x01, 0xFF]
            .into_iter()
            .chain(0x01..=0xFE)
            .chain(once(0x00))
            .collect();
        assert_eq!(encode(&src), expected);
    }

    #[test]
    fn non_zero_run_of_255_spills_into_a_second_block() {
        let src: Vec<u8> = (0x01..=0xFF).collect();
        let expected: Vec<u8> = once(0xFF)
            .chain(0x01..=0xFE)
            .chain([0x02, 0xFF, 0x00])
            .collect();
        assert_eq!(encode(&src), expected);
    }

    #[test]
    fn zero_right_after_a_full_block() {
        // The zero after a 0xFF block is not implied, so it needs its own
        // code byte.
        let src: Vec<u8> = (0x02..=0xFF).chain(once(0x00)).collect();
        let expected: Vec<u8> = once(0xFF)
            .chain(0x02..=0xFF)
            .chain([0x01, 0x01, 0x00])
            .collect();
        assert_eq!(encode(&src), expected);
    }

    #[test]
    fn zero_one_byte_before_a_full_block() {
        let src: Vec<u8> = (0x03..=0xFF).chain([0x00, 0x01]).collect();
        let expected: Vec<u8> = once(0xFE)
            .chain(0x03..=0xFF)
            .chain([0x02, 0x01, 0x00])
            .collect();
        assert_eq!(encode(&src), expected);
    }

    #[test]
    fn only_zero_in_output_is_the_trailing_delimiter() {
        // Every byte value, several times over, so runs cross block
        // boundaries at different offsets.
        let src: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
        let encoded = encode(&src);

        let (delimiter, body) = encoded.split_last().unwrap();
        assert_eq!(*delimiter, 0x00);
        assert!(
            !body.contains(&0x00),
            "found a zero before the delimiter at index {:?}",
            body.iter().position(|&b| b == 0x00)
        );
    }

    #[test]
    fn code_bytes_chain_lands_on_the_delimiter() {
        // Following each code byte forward by its value must walk exactly
        // onto the trailing 00, never past it.
        let src: Vec<u8> = (0..1024).map(|i| (i * 7 % 256) as u8).collect();
        let encoded = encode(&src);

        let mut i = 0;
        while encoded[i] != 0x00 {
            i += usize::from(encoded[i]);
            assert!(i < encoded.len(), "code byte chain ran off the end");
        }
        assert_eq!(i, encoded.len() - 1);
    }

    #[test]
    fn worst_case_length_matches_frame_overhead() {
        // With no zeros to replace, every 254 bytes costs one code byte,
        // plus the delimiter. frame.rs sizes MAX_FRAME_LEN on this.
        for n in 1..=1024 {
            let src = vec![0x01; n];
            assert_eq!(
                encode(&src).len(),
                n + n.div_ceil(254) + 1,
                "wrong encoded length for {n} non-zero bytes"
            );
        }
    }

    #[test]
    fn exactly_sized_buffer_is_enough() {
        let src = [0x11, 0x22, 0x00, 0x33];
        let mut out = [0u8; 6];
        assert_eq!(cobs_encode(&src, &mut out), Some(6));
        assert_eq!(out, [0x03, 0x11, 0x22, 0x02, 0x33, 0x00]);
    }

    #[test]
    fn buffer_one_byte_short_returns_none() {
        let src = [0x11, 0x22, 0x00, 0x33];
        let mut out = [0u8; 5];
        assert_eq!(cobs_encode(&src, &mut out), None);
    }

    #[test]
    fn empty_buffer_returns_none() {
        assert_eq!(cobs_encode(&[], &mut []), None);
        assert_eq!(cobs_encode(&[0x11], &mut []), None);
    }

    #[test]
    fn buffer_too_small_for_block_overhead_returns_none() {
        // Fits the data plus delimiter, but not the second code byte.
        let src = [0x01; 255];
        let mut out = [0u8; 255 + 1 + 1];
        assert_eq!(cobs_encode(&src, &mut out), None);
    }

    #[test]
    fn bytes_past_the_returned_length_are_untouched() {
        let mut out = [0xAA; 16];
        let len = cobs_encode(&[0x11, 0x00], &mut out).unwrap();
        assert_eq!(&out[..len], [0x02, 0x11, 0x01, 0x00]);
        assert!(out[len..].iter().all(|&b| b == 0xAA));
    }
}
