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
            .map_err(|_| ArrivalError::RouteIdTooLong)?;

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
    RouteIdTooLong,
}
