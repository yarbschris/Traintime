#![cfg_attr(not(test), no_std)]
#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

mod cobs;
mod crc;

pub mod arrival;
pub mod client_display;
pub mod frame;
