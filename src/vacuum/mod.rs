//! Vacuum: listening without knowing who you are listening to. The
//! Unfamiliar Sounds Firefox extension, brought into the app. Every song,
//! album, artist, playlist and user name is replaced by a stand-in, two or
//! three words from a frozen word list ("Mailbox Manatee Yack"); covers and
//! artist pictures by museum artworks or generated shapes and rings; and
//! what centres on people and popularity is hidden.
//!
//! This module has no interface: the derivation, the seed, the museum
//! chain, generated pictures and the playlist rules. Stand-ins match the
//! extension's exactly for the same seed (see the golden-vector tests). The
//! fixed strings inside the derivation (`unfamiliar sounds seed:`,
//! `picture:`) keep the extension's name, or stand-ins would stop matching.

pub mod art;
pub mod collections;
pub mod derive;
pub mod gate;
pub mod museums;
pub mod pictures;
pub mod seed;
mod shared_seed;
pub mod shown;

pub use derive::{Deriver, Kind, Picture, StandIn};
pub use seed::Seed;

/// The derivation for a seed.
pub fn deriver(seed: &Seed) -> Deriver {
    Deriver::new(seed.key())
}

#[cfg(test)]
mod tests;
