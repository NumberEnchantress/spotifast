//! The seed everyone shares by default, so the same name gets the same
//! stand-in for everyone, in this app and in the Unfamiliar Sounds
//! extension. It ships inside the app, as it does inside the extension, so
//! anyone who digs can recover it; it is scrambled here only so it is not in
//! plain sight. Never show, log, export or save it: settings only ever say
//! the shared seed is in use.

use sha2::{Digest, Sha256};

const SCRAMBLED: [u8; 32] = [
    0xb6, 0x7c, 0xc0, 0x97, 0xe1, 0x99, 0x5a, 0x00, 0x82, 0x15, 0xda, 0x89, 0xc1, 0x1a, 0x70, 0xc7,
    0x20, 0xb2, 0x24, 0xa3, 0x24, 0x8e, 0x52, 0xd5, 0x38, 0xcf, 0xe8, 0x4c, 0x92, 0xe0, 0x22, 0xaf,
];

const MASK_LABEL: &[u8] = b"sirensong vacuum: not a secret, just not in plain sight";

/// The shared seed's key.
pub(super) fn key() -> [u8; 32] {
    let mask = Sha256::digest(MASK_LABEL);
    std::array::from_fn(|index| SCRAMBLED[index] ^ mask[index])
}
