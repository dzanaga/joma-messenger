//! Joma's desktop share between Jami peers.
//!
//! - [`signal`]: the in-call handshake (`Hello`, `Offer`, `Accept`, `Decline`, `Stop`).
//! - [`share`]: capture and encode on the sharer (gliff's pipeline).
//! - [`view`]: decode on the viewer.
//! - [`preview`]: thumbnails for the picker and a live preview of what is shared.
//! - [`gpu`]: which hardware encodes and decodes ([`va`] is the VA-API-only tier).
//! - [`portal`]: the fallback, Jami's own screen share through the desktop portal.

pub mod gpu;
pub mod ink;
pub mod portal;
pub mod preview;
#[allow(dead_code)] // vendored whole from gliff
mod rate;
pub mod share;
pub mod signal;
pub mod va;
pub mod view;
