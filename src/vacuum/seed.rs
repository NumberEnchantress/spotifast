//! Which seed Vacuum derives from: the one everyone shares, or the user's
//! own. Ported from the extension's `background.js`.

use sha2::{Digest, Sha256};

/// A seed of your own is at least this long, and at most [`SEED_MAX`], in
/// UTF-16 units as the extension counts them.
pub const SEED_MIN: usize = 8;
pub const SEED_MAX: usize = 500;

/// The key a seed derives from, and a short id for naming its caches. The
/// id never reveals the seed.
#[derive(Clone)]
pub struct Seed {
    key: [u8; 32],
    /// Set for a seed of your own; the shared seed is never shown.
    pub custom: bool,
    pub id: String,
}

impl std::fmt::Debug for Seed {
    // Never prints the key.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seed")
            .field("custom", &self.custom)
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl Seed {
    /// The seed everyone shares.
    pub fn shared() -> Self {
        Self::from_key(super::shared_seed::key(), false)
    }

    /// `text` as the user's own seed, or `None` when it is too short or too
    /// long. Exactly 64 hex digits, spaces around them aside, are the key
    /// itself, as keys saved by early versions of the extension were.
    pub fn custom(text: &str) -> Option<Self> {
        let length = text.encode_utf16().count();
        if !(SEED_MIN..=SEED_MAX).contains(&length) {
            return None;
        }
        Some(Self::from_key(custom_key(text), true))
    }

    /// The user's seed if one is set, else the shared one. As the
    /// extension does with a stored seed, only its least length is checked:
    /// [`Self::custom`] keeps longer ones from being set.
    pub fn from_setting(text: Option<&str>) -> Self {
        match text.filter(|text| text.encode_utf16().count() >= SEED_MIN) {
            Some(text) => Self::from_key(custom_key(text), true),
            None => Self::shared(),
        }
    }

    fn from_key(key: [u8; 32], custom: bool) -> Self {
        let id = super::derive::hex(&Sha256::digest(key)[..8]);
        Self { key, custom, id }
    }

    pub(super) fn key(&self) -> [u8; 32] {
        self.key
    }
}

fn custom_key(text: &str) -> [u8; 32] {
    let trimmed = text.trim_matches(super::derive::js_space);
    if trimmed.len() == 64 && trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        let mut key = [0u8; 32];
        for (index, byte) in key.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&trimmed[index * 2..index * 2 + 2], 16).unwrap_or_default();
        }
        return key;
    }
    Sha256::digest(format!("unfamiliar sounds seed:{text}").as_bytes()).into()
}

/// The key behind a seed, for tests that check it against the extension.
#[cfg(test)]
pub(super) fn key_hex(seed: &Seed) -> String {
    super::derive::hex(&seed.key)
}
