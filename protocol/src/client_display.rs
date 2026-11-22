use crate::arrival::{ARRIVAL_MAX_ENCODED_LEN, Arrival};
use heapless::Vec;
use serde::{Deserialize, Serialize};

/// Maximum number of arrivals
///
/// Sets max worst case wire time at ~60ms
/// Transport limit, bumping this means bumping versions
pub const MAX_ARRIVALS: usize = 8;
const _: () = assert!(
    MAX_ARRIVALS < 128,
    "max arrivals length prefix must stay at 1 byte"
);

/// Vector of Arrivals, an empty vector means no trains
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrivalList(Vec<Arrival, MAX_ARRIVALS>);

/// What the screen shows based on server status
///
/// | `ServerStatus`           | `ClientDisplay`       |
/// |--------------------------|-----------------------|
/// | `NoSelection`            | `WaitingForSelection` |
/// | `NoStationSelected`      | `WaitingForSelection` |
/// | `Starting`               | `Loading`             |
/// | `LoadingStaticData`      | `Loading`             |
/// | `Resolving`              | `Loading`             |
/// | `FetchFailed`            | `NetworkError`        |
/// | `UnknownStation(String)` | `ConfigError`         |
/// | `NoEndpointsForStation`  | `ConfigError`         |
/// | `Ready`                  | `Arrivals`            |
#[expect(
    clippy::large_enum_variant,
    reason = "no heap to box into; a struct of status + arrivals would be the same size"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientDisplay {
    /// Waiting for user to select station
    WaitingForSelection,
    /// Fetching or parsing static data
    Loading,
    /// Fetch failure
    NetworkError,
    /// Unknown station or no endpoints
    ConfigError,
    /// Show next trains
    Arrivals(ArrivalList),
}

/// The largest a [`ClientDisplay`] can be once encoded with postcard, in
/// bytes. This is the payload only: the version byte, CRC and COBS overhead
/// come on top at the framing step.
///
/// Worked out from the wire format; a test encodes a worst-case display and
/// checks this is exact.
pub const CLIENT_DISPLAY_MAX_ENCODED_LEN: usize =
    // variant tag: varint, 1 byte while there are fewer than 128 variants
    1
    // arrivals length prefix: 1 byte (MAX_ARRIVALS < 128)
    + 1
    // every arrival at its worst case
    + MAX_ARRIVALS * ARRIVAL_MAX_ENCODED_LEN;

#[cfg(test)]
mod tests {
    use super::*;

    // Builds a list directly through the private field. Switch to the public
    // constructor once it exists.
    fn list(arrivals: &[Arrival]) -> ArrivalList {
        ArrivalList(arrivals.iter().cloned().collect())
    }

    fn encode(display: &ClientDisplay) -> std::vec::Vec<u8> {
        let mut buf = [0u8; 1024];
        postcard::to_slice(display, &mut buf).unwrap().to_vec()
    }

    fn pelham() -> Arrival {
        Arrival::new("6", "Pelham Bay Park", 4, Some(-90)).unwrap()
    }

    #[test]
    fn variants_without_data_encode_to_known_bytes() {
        // Golden bytes: a variant is its position in the enum, as a varint.
        // If this fails, the variant order changed: bump the protocol version.
        assert_eq!(encode(&ClientDisplay::WaitingForSelection), [0x00]);
        assert_eq!(encode(&ClientDisplay::Loading), [0x01]);
        assert_eq!(encode(&ClientDisplay::NetworkError), [0x02]);
        assert_eq!(encode(&ClientDisplay::ConfigError), [0x03]);
    }

    #[test]
    fn no_trains_encodes_to_known_bytes() {
        let display = ClientDisplay::Arrivals(list(&[]));

        // variant 4, then a list of length 0
        assert_eq!(encode(&display), [0x04, 0x00]);
    }

    #[test]
    fn arrivals_encode_to_known_bytes() {
        let display = ClientDisplay::Arrivals(list(&[pelham()]));

        #[rustfmt::skip]
        let expected = [
            0x04,                               // variant: Arrivals
            0x01,                               // list length: 1
            // the arrival, same bytes as arrival::tests::arrival_encodes_to_known_bytes
            0x01, b'6',
            0x0F,
            b'P', b'e', b'l', b'h', b'a', b'm', b' ',
            b'B', b'a', b'y', b' ',
            b'P', b'a', b'r', b'k',
            0x04,
            0x01, 0xB3, 0x01,
        ];

        assert_eq!(encode(&display), expected);
    }

    #[test]
    fn every_variant_round_trips_through_postcard() {
        let displays = [
            ClientDisplay::WaitingForSelection,
            ClientDisplay::Loading,
            ClientDisplay::NetworkError,
            ClientDisplay::ConfigError,
            ClientDisplay::Arrivals(list(&[])),
            ClientDisplay::Arrivals(list(&[
                pelham(),
                Arrival::new("GS", "Grand Central-42 St", 0, None).unwrap(),
            ])),
        ];

        for display in displays {
            let encoded = encode(&display);
            let decoded: ClientDisplay = postcard::from_bytes(&encoded).unwrap();

            assert_eq!(decoded, display);
        }
    }

    #[test]
    fn worst_case_display_encodes_to_exactly_the_max_len() {
        let worst_arrival = Arrival::new(
            &"R".repeat(crate::arrival::MAX_ROUTE_ID_LEN),
            &"a".repeat(crate::arrival::MAX_HEADSIGN_LEN),
            u16::MAX,
            Some(i16::MIN),
        )
        .unwrap();
        let display = ClientDisplay::Arrivals(list(&std::vec![worst_arrival; MAX_ARRIVALS]));

        // Equality, not <=: an overestimate is a wrong number too.
        assert_eq!(encode(&display).len(), CLIENT_DISPLAY_MAX_ENCODED_LEN);
    }

    #[test]
    fn more_arrivals_than_the_limit_are_rejected_on_decode() {
        // What an older receiver sees if a newer sender raises MAX_ARRIVALS:
        // the whole frame fails to decode, so the limit is part of the
        // protocol (see the design decision).
        fn arrivals_frame(count: usize) -> std::vec::Vec<u8> {
            let mut bytes = std::vec![0x04, count as u8];
            // The smallest arrival: empty route, empty headsign, 0 minutes, None.
            for _ in 0..count {
                bytes.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
            }
            bytes
        }

        // Control: the same bytes at the limit decode, so the failure below
        // is the count, not malformed input.
        assert!(postcard::from_bytes::<ClientDisplay>(&arrivals_frame(MAX_ARRIVALS)).is_ok());

        assert!(postcard::from_bytes::<ClientDisplay>(&arrivals_frame(MAX_ARRIVALS + 1)).is_err());
    }

    #[test]
    fn unknown_variant_is_rejected_on_decode() {
        // What an older receiver sees if a newer sender adds a variant.
        assert!(postcard::from_bytes::<ClientDisplay>(&[0x05]).is_err());
    }
}
