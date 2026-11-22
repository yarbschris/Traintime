#![cfg_attr(not(test), no_std)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]
pub mod arrival;
pub mod client_display;
mod crc;
