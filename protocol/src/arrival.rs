use heapless::String;
use serde::{Deserialize, Serialize};

/// Maximum length of a route ID, in UTF-8 bytes.
///
/// A transport limit, not a display limit: changing it means bumping the
/// protocol version.
pub const MAX_ROUTE_ID_LEN: usize = 10;
const _: () = assert!(
    MAX_ROUTE_ID_LEN < 128,
    "route_id length prefix must stay at 1 byte"
);
/// Maximum length of a headsign, in UTF-8 bytes (not characters).
///
/// A transport limit, not a display limit: changing it means bumping the
/// protocol version.
pub const MAX_HEADSIGN_LEN: usize = 64;
const _: () = assert!(
    MAX_HEADSIGN_LEN < 128,
    "headsign length prefix must stay at 1 byte"
);

/// The largest an [`Arrival`] can be once encoded with postcard, in bytes.
///
/// Worked out from the wire format; a test encodes a worst-case arrival and
/// checks this is exact.
pub const ARRIVAL_MAX_ENCODED_LEN: usize =
    // route_id: 1-byte length prefix + bytes
    (1 + MAX_ROUTE_ID_LEN)
    // headsign: 1-byte length prefix + bytes
    + (1 + MAX_HEADSIGN_LEN)
    // minutes: u16 varint, worst case 3 bytes
    + 3
    // delay: Some tag + zigzag i16 varint, worst case 3 bytes
    + (1 + 3);

/// One upcoming train at the selected station, as sent to the sign.
///
/// The field order is the order on the wire: reordering, adding or removing
/// a field means bumping the protocol version.
#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct Arrival {
    route_id: String<MAX_ROUTE_ID_LEN>,
    headsign: String<MAX_HEADSIGN_LEN>,
    minutes: u16,
    delay: Option<i16>,
}

impl Arrival {
    /// Builds an arrival, fitting it to the transport limits.
    ///
    /// A headsign longer than [`MAX_HEADSIGN_LEN`] bytes is truncated on a
    /// character boundary, so it may come out shorter than the limit. A route
    /// ID is never truncated, because a shortened ID is a different route.
    ///
    /// `minutes` and `delay` are taken as given: converting and saturating
    /// from the server's wider types is the caller's job.
    ///
    /// # Errors
    ///
    /// [`ArrivalError::RouteIdTooLong`] if `route_id` is longer than
    /// [`MAX_ROUTE_ID_LEN`] bytes.
    pub fn new(
        route_id: &str,
        headsign: &str,
        minutes: u16,
        delay: Option<i16>,
    ) -> Result<Self, ArrivalError> {
        // Return ArrivalError on route_id capacity error
        let mut route_id_string: String<MAX_ROUTE_ID_LEN> = String::new();
        route_id_string
            .push_str(route_id)
            .map_err(|_| ArrivalError::RouteIdTooLong {
                len: route_id.len(),
            })?;

        // Truncate on a char boundary: push whole chars until one doesn't fit
        let mut headsign_string: String<MAX_HEADSIGN_LEN> = String::new();
        for character in headsign.chars() {
            if headsign_string.push(character).is_err() {
                break;
            }
        }

        Ok(Arrival {
            route_id: route_id_string,
            headsign: headsign_string,
            minutes,
            delay,
        })
    }

    /// The route ID as the feed gives it (e.g. `"6X"`), at most
    /// [`MAX_ROUTE_ID_LEN`] bytes. The layout uses it to pick the route's
    /// bullet and color.
    ///
    /// May be empty, if the feed sent an empty route ID. That's deliberate:
    /// the arrival is still worth showing, and how to draw a missing route is
    /// the layout's decision.
    pub fn route_id(&self) -> &str {
        &self.route_id
    }

    /// Where the train is going: the name of its last stop, at most
    /// [`MAX_HEADSIGN_LEN`] bytes. May have been truncated on a character
    /// boundary; fitting it to a display is the layout's job.
    pub fn headsign(&self) -> &str {
        &self.headsign
    }

    /// Whole minutes until arrival, counted from when the frame was sent.
    /// The sender saturates at `u16::MAX`.
    pub fn minutes(&self) -> u16 {
        self.minutes
    }

    /// The feed's reported delay, in seconds.
    ///
    /// Positive means late, negative means early, `Some(0)` means on time.
    /// `None` means the feed didn't report a delay, which is not the same as
    /// on time. The sender saturates at the `i16` range (about ±9 hours).
    pub fn delay(&self) -> Option<i16> {
        self.delay
    }
}

/// Why an [`Arrival`] couldn't be built.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum ArrivalError {
    /// The route ID is longer than [`MAX_ROUTE_ID_LEN`] bytes.
    RouteIdTooLong {
        /// The length in bytes of the rejected `route_id`
        len: usize,
    },
}

impl core::error::Error for ArrivalError {}

impl core::fmt::Display for ArrivalError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            ArrivalError::RouteIdTooLong { len } => {
                write!(
                    f,
                    "route_id is {len} bytes, limit is {MAX_ROUTE_ID_LEN} bytes"
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headsign_at_limit_is_kept_whole() {
        let headsign = "a".repeat(MAX_HEADSIGN_LEN);

        let arrival = Arrival::new("6", &headsign, 4, None).unwrap();

        assert_eq!(arrival.headsign(), headsign);
    }

    #[test]
    fn headsign_over_limit_is_truncated_to_limit() {
        let headsign = "a".repeat(MAX_HEADSIGN_LEN + 1);

        let arrival = Arrival::new("6", &headsign, 4, None).unwrap();

        assert_eq!(arrival.headsign(), &headsign[..MAX_HEADSIGN_LEN]);
    }

    #[test]
    fn headsign_is_truncated_on_a_char_boundary() {
        // "é" is 2 bytes, so it would straddle the limit: bytes 63 and 64.
        let headsign = "a".repeat(MAX_HEADSIGN_LEN - 1) + "é";
        assert_eq!(headsign.len(), MAX_HEADSIGN_LEN + 1);

        let arrival = Arrival::new("6", &headsign, 4, None).unwrap();

        assert_eq!(arrival.headsign(), "a".repeat(MAX_HEADSIGN_LEN - 1));
        assert_eq!(arrival.headsign().len(), MAX_HEADSIGN_LEN - 1);
    }

    #[test]
    fn route_id_at_limit_is_accepted() {
        let route_id = "R".repeat(MAX_ROUTE_ID_LEN);

        let arrival = Arrival::new(&route_id, "Pelham Bay Park", 4, None).unwrap();

        assert_eq!(arrival.route_id(), route_id);
    }

    #[test]
    fn route_id_over_limit_is_rejected() {
        let route_id = "R".repeat(MAX_ROUTE_ID_LEN + 1);

        let result = Arrival::new(&route_id, "Pelham Bay Park", 4, None);

        assert_eq!(
            result,
            Err(ArrivalError::RouteIdTooLong {
                len: route_id.len()
            })
        );
    }

    #[test]
    fn empty_route_id_is_accepted() {
        // Deliberate: see `Arrival::route_id`. The layout handles a missing
        // route; `new` must not reject it.
        let arrival = Arrival::new("", "Pelham Bay Park", 4, None).unwrap();

        assert_eq!(arrival.route_id(), "");
    }

    #[test]
    fn arrival_round_trips_through_postcard() {
        let arrivals = [
            Arrival::new("6", "Pelham Bay Park", 4, Some(-90)).unwrap(),
            Arrival::new("GS", "Grand Central-42 St", 0, None).unwrap(),
            // Multi-byte UTF-8 must survive decoding too.
            Arrival::new("S1", "Köln Hbf", u16::MAX, Some(i16::MIN)).unwrap(),
        ];

        for arrival in arrivals {
            let mut buf = [0u8; 256];
            let encoded = postcard::to_slice(&arrival, &mut buf).unwrap();
            let decoded: Arrival = postcard::from_bytes(encoded).unwrap();

            assert_eq!(decoded, arrival);
        }
    }

    #[test]
    fn worst_case_arrival_encodes_to_exactly_the_max_len() {
        // Every field at its largest encoding. i16::MIN zigzags to 65535, the
        // largest zigzag value, so it's the true worst case for `delay`.
        let arrival = Arrival::new(
            &"R".repeat(MAX_ROUTE_ID_LEN),
            &"a".repeat(MAX_HEADSIGN_LEN),
            u16::MAX,
            Some(i16::MIN),
        )
        .unwrap();

        let mut buf = [0u8; 256];
        let encoded = postcard::to_slice(&arrival, &mut buf).unwrap();

        // Equality, not <=: an overestimate is a wrong number too.
        assert_eq!(encoded.len(), ARRIVAL_MAX_ENCODED_LEN);
    }

    #[test]
    fn arrival_encodes_to_known_bytes() {
        // Golden bytes, worked out by hand from the postcard wire format.
        // Pins the field order and encodings: if this fails, the wire format
        // changed, and the protocol version needs a bump.
        let arrival = Arrival::new("6", "Pelham Bay Park", 4, Some(-90)).unwrap();

        #[rustfmt::skip]
        let expected = [
            0x01, b'6',                         // route_id: len 1, "6"
            0x0F,                               // headsign: len 15
            b'P', b'e', b'l', b'h', b'a', b'm', b' ',
            b'B', b'a', b'y', b' ',
            b'P', b'a', b'r', b'k',
            0x04,                               // minutes: 4
            0x01, 0xB3, 0x01,                   // delay: Some, zigzag(-90) = 179
        ];

        let mut buf = [0u8; 256];
        let encoded = postcard::to_slice(&arrival, &mut buf).unwrap();

        assert_eq!(encoded, expected);
    }
}
