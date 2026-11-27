const ENCODING_BYTE: u8 = 0;

// COBS decode buf in-place
pub fn cobs_decode(buf: &mut [u8]) -> Option<usize> {
    // A valid encoding has at least one byte
    if buf.is_empty() {
        return None;
    }
    let mut read: usize = 0;
    let mut write: usize = 0;

    while read < buf.len() {
        let val_code_byte = *buf.get(read)?;
        // Corrupt
        if val_code_byte == 0 {
            return None;
        }
        let next_code_byte_index = read + val_code_byte as usize;

        // Corrupt
        if (next_code_byte_index) > buf.len() {
            return None;
        }

        buf.copy_within(read + 1..next_code_byte_index, write);
        read = next_code_byte_index;
        write += val_code_byte as usize - 1;

        // We only add a `0x00` data byte if the next code byte was not overhead
        // (a.k.a the previous code byte was not 255) and when we have not read the last block
        if val_code_byte != 255 && read < buf.len() {
            write = copy_byte(buf, ENCODING_BYTE, write)?;
        }
    }

    Some(write)
}

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

    // ---- Decoding ----

    /// Decodes a copy of `frame` (no delimiter) and returns the decoded bytes.
    fn decode(frame: &[u8]) -> Option<Vec<u8>> {
        let mut buf = frame.to_vec();
        let len = cobs_decode(&mut buf)?;
        buf.truncate(len);
        Some(buf)
    }

    /// Encodes `src`, strips the delimiter as the receiver would, decodes in
    /// place, and checks the original comes back.
    fn assert_round_trips(src: &[u8]) {
        let encoded = encode(src);
        let (_delimiter, frame) = encoded.split_last().unwrap();
        assert_eq!(
            decode(frame).as_deref(),
            Some(src),
            "round trip failed for {} input bytes",
            src.len()
        );
    }

    #[test]
    fn decodes_short_known_frames() {
        // The short Wikipedia examples again, delimiter removed.
        let cases: &[(&[u8], &[u8])] = &[
            (&[0x01], &[]),
            (&[0x01, 0x01], &[0x00]),
            (&[0x01, 0x01, 0x01], &[0x00, 0x00]),
            (&[0x01, 0x02, 0x11, 0x01], &[0x00, 0x11, 0x00]),
            (&[0x03, 0x11, 0x22, 0x02, 0x33], &[0x11, 0x22, 0x00, 0x33]),
            (&[0x05, 0x11, 0x22, 0x33, 0x44], &[0x11, 0x22, 0x33, 0x44]),
            (&[0x02, 0x11, 0x01, 0x01, 0x01], &[0x11, 0x00, 0x00, 0x00]),
        ];
        for (frame, expected) in cases {
            assert_eq!(
                decode(frame).as_deref(),
                Some(*expected),
                "frame {frame:02X?}"
            );
        }
    }

    #[test]
    fn full_last_block_has_no_trailing_zero() {
        // 0xFF block that is also the last block: neither condition implies a zero.
        let frame: Vec<u8> = once(0xFF).chain(0x01..=0xFE).collect();
        let expected: Vec<u8> = (0x01..=0xFE).collect();
        assert_eq!(decode(&frame), Some(expected));
    }

    #[test]
    fn full_block_in_the_middle_has_no_implied_zero() {
        let frame: Vec<u8> = once(0xFF).chain(0x01..=0xFE).chain([0x02, 0xFF]).collect();
        let expected: Vec<u8> = (0x01..=0xFF).collect();
        assert_eq!(decode(&frame), Some(expected));
    }

    #[test]
    fn zero_after_a_full_block_is_decoded() {
        let frame: Vec<u8> = once(0xFF).chain(0x02..=0xFF).chain([0x01, 0x01]).collect();
        let expected: Vec<u8> = (0x02..=0xFF).chain(once(0x00)).collect();
        assert_eq!(decode(&frame), Some(expected));
    }

    #[test]
    fn round_trips_every_encoder_example() {
        assert_round_trips(&[]);
        assert_round_trips(&[0x00]);
        assert_round_trips(&[0x00, 0x00]);
        assert_round_trips(&[0x00, 0x11, 0x00]);
        assert_round_trips(&[0x11, 0x22, 0x00, 0x33]);
        assert_round_trips(&[0x11, 0x22, 0x33, 0x44]);
        assert_round_trips(&[0x11, 0x00, 0x00, 0x00]);
        assert_round_trips(&(0x01..=0xFE).collect::<Vec<u8>>());
        assert_round_trips(&(0x00..=0xFE).collect::<Vec<u8>>());
        assert_round_trips(&(0x01..=0xFF).collect::<Vec<u8>>());
        assert_round_trips(&(0x02..=0xFF).chain(once(0x00)).collect::<Vec<u8>>());
        assert_round_trips(&(0x03..=0xFF).chain([0x00, 0x01]).collect::<Vec<u8>>());
    }

    #[test]
    fn round_trips_non_zero_runs_of_every_length() {
        // Hits every position relative to the 254-byte block boundary.
        for n in 0..=1024 {
            assert_round_trips(&vec![0x01; n]);
        }
    }

    #[test]
    fn round_trips_zero_runs_of_every_length() {
        for n in 0..=600 {
            assert_round_trips(&vec![0x00; n]);
        }
    }

    #[test]
    fn round_trips_mixed_data() {
        // Deterministic pseudo-random bytes with roughly 1 in 8 zeros, so
        // blocks of all sizes show up, including some full ones.
        let mut state: u32 = 0x1234_5678;
        let mut next = || {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        };
        for len in [1, 2, 253, 254, 255, 256, 508, 509, 1000, 2048] {
            let src: Vec<u8> = (0..len)
                .map(|_| {
                    let b = next();
                    if b < 32 { 0x00 } else { b }
                })
                .collect();
            assert_round_trips(&src);
        }
        // Long runs without zeros, cut by occasional ones.
        let src: Vec<u8> = (0..2048u32)
            .map(|i| if i % 600 == 0 { 0 } else { 0xAB })
            .collect();
        assert_round_trips(&src);
    }

    #[test]
    fn decoded_data_is_at_the_front_of_the_buffer() {
        let mut buf = [0x03, 0x11, 0x22, 0x02, 0x33];
        let len = cobs_decode(&mut buf).unwrap();
        assert_eq!(&buf[..len], [0x11, 0x22, 0x00, 0x33]);
    }

    #[test]
    fn empty_frame_is_corrupt() {
        // Every valid encoding has at least one code byte, so an empty frame
        // (two delimiters back to back on the wire) can't be valid.
        assert_eq!(cobs_decode(&mut []), None);
    }

    #[test]
    fn zero_code_byte_is_corrupt() {
        assert_eq!(decode(&[0x00]), None);
        assert_eq!(decode(&[0x00, 0x11, 0x22]), None);
    }

    #[test]
    fn zero_code_byte_after_the_first_block_is_corrupt() {
        assert_eq!(decode(&[0x02, 0x11, 0x00, 0x33]), None);
    }

    #[test]
    fn code_byte_overrunning_the_frame_is_corrupt() {
        assert_eq!(decode(&[0x05, 0x11, 0x22]), None);
        assert_eq!(decode(&[0xFF]), None);
    }

    #[test]
    fn later_code_byte_overrunning_the_frame_is_corrupt() {
        // First block is fine; the second claims more bytes than remain.
        assert_eq!(decode(&[0x02, 0x11, 0x04, 0x22]), None);
    }

    #[test]
    fn truncated_frame_is_corrupt() {
        // A frame that lost its tail on the wire, e.g. dropped UART bytes.
        let encoded = encode(&(0x01..=0xFF).collect::<Vec<u8>>());
        let frame = &encoded[..encoded.len() - 1];
        for cut in 1..frame.len() {
            // Only cuts inside a block are detectable; a cut exactly at a
            // block boundary is a valid shorter frame and is the CRC's job.
            if cut == 255 {
                continue;
            }
            assert_eq!(decode(&frame[..cut]), None, "cut at {cut} not detected");
        }
    }
}
