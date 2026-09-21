//! Platform-independent core of the Cardmic client.
//!
//! Nothing in this crate touches an audio device, a UI, or the network stack
//! beyond plain data. That is deliberate: it keeps the protocol logic unit
//! testable without hardware, and it is the layer an outside contributor can
//! work on without owning a Cardputer.

pub mod dsp;
pub mod pairing;
pub mod protocol;
pub mod sequencer;
