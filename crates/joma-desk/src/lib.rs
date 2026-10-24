//! Joma's desktop share between Jami peers.
//!
//! - [`signal`]: the in-call handshake (`Hello`, `Offer`, `Accept`, `Decline`, `Stop`).
//! - [`share`]: capture and encode on the sharer (gliff's pipeline).
//! - [`view`]: decode on the viewer.
//! - [`portal`]: the fallback, Jami's own screen share through the desktop portal.

pub mod portal;
#[allow(dead_code)] // vendored whole from gliff
mod rate;
pub mod share;
pub mod signal;
pub mod view;
