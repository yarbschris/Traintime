use crate::{
    client_display::{CLIENT_DISPLAY_MAX_ENCODED_LEN, ClientDisplay},
    cobs::cobs_encode,
    crc::{CRC_LEN, crc32},
};

const VERSION_LEN: usize = 1;

const UNENCODED_MAX_LEN: usize = VERSION_LEN + CLIENT_DISPLAY_MAX_ENCODED_LEN + CRC_LEN;
/// Maximum length of a frame of `[Version][ClientDisplay][CRC]`, excludes `00` delimiter
pub const MAX_FRAME_LEN: usize = UNENCODED_MAX_LEN + UNENCODED_MAX_LEN.div_ceil(254);
/// Maximum number of bytes on wire: MAX_FRAME_LEN + `00` delimiter
pub const MAX_BYTES_ON_WIRE: usize = MAX_FRAME_LEN + 1;
/// First byte of a frame, encodes the frame version. Frames with unrecognized version are dropped.
/// Bumped on breaking changes.
pub const VERSION_BYTE: u8 = 1;

pub fn encode<'a>(
    client_display: &ClientDisplay,
    out: &'a mut [u8; MAX_BYTES_ON_WIRE],
) -> Result<&'a [u8], EncodeError> {
    // Scratch buffer. COBS cannot encode inplace, so this buffer is used
    // to build the payload pre-COBS
    let mut scratch: [u8; UNENCODED_MAX_LEN] = [0u8; UNENCODED_MAX_LEN];
    // Running written length of scratch buffer
    let mut filled: usize = 0;

    // Write version byte to scratch and increment `filled` by `VERSION_LEN`
    scratch[0] = VERSION_BYTE;
    filled += VERSION_LEN;

    // Write serialized `client_display` to scratch buffer, take the len() of returned slice,
    // dropping the mutable reference to the slice in the process
    // Then increase `filled` by the `client_display_len`
    let client_display_len = postcard::to_slice(
        client_display,
        scratch
            .get_mut(filled..)
            .ok_or(EncodeError::IncorrectConstants)?,
    )
    .map_err(|_| EncodeError::IncorrectConstants)?
    .len();
    filled += client_display_len;

    // Calculate crc of buffer to `filled` and save as little endian bytes
    let crc_bytes = crc32(&scratch[..filled]).to_le_bytes();
    // Write CRC bytes to scratch and increase `filled` by `CRC_LEN`
    scratch
        .get_mut(filled..filled + CRC_LEN)
        .ok_or(EncodeError::IncorrectConstants)?
        .copy_from_slice(&crc_bytes);
    filled += CRC_LEN;

    // COBS encode should never fail when UNENCODED_MAX_LEN and MAX_BYTES_ON_WIRE are calculated correctly
    let len = cobs_encode(&scratch[..filled], out).ok_or(EncodeError::IncorrectConstants)?;

    out.get(..len).ok_or(EncodeError::IncorrectConstants)
}

/// Why a frame couldn't be encoded
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// This error will only occur when an incorrectly defined constant, such as
    /// [MAX_BYTES_ON_WIRE], `UNENCODED_MAX_LEN`, [MAX_FRAME_LEN], `VERSION_LEN`,
    /// or `CRC_LEN`, causes the frame encoder to fail because a frame buffer is too small
    IncorrectConstants,
}

impl core::fmt::Display for EncodeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            EncodeError::IncorrectConstants => {
                write!(
                    f,
                    "a frame buffer was too small: the frame size constants are wrong"
                )
            }
        }
    }
}
impl core::error::Error for EncodeError {}
