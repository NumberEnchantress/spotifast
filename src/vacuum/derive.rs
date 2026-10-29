//! Names and pictures from a seed: the Unfamiliar Sounds derivation, ported
//! exactly from the extension's `core/derive.js`. Every stand-in is a pure
//! function of the key, the kind of thing and its name as shown:
//!
//! ```text
//! digest = HMAC-SHA-256(key, kind + ":" + normalize(name))
//! bytes  0..16 -> one big integer -> successive remainders pick the words
//! bytes 16..32 -> one big integer -> successive remainders pick the picture
//! ```
//!
//! The same name and seed give the same stand-in here and in the extension,
//! so someone who learnt that "Mailbox Manatee Yack" is a band they like
//! finds it under that name in both.

use std::collections::HashMap;
use std::sync::OnceLock;

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

/// The reviewed word list, frozen. Refuse to run on any other.
const WORDLIST: &[u8] = include_bytes!("../../assets/vacuum/wordlist_v2-full.tsv");
const WORDLIST_SHA256: &str = "d4610605d28ebcc8efe24548b2cc38044618ee2f9147cafe6dbea770d6ebd0ab";
const WORDLIST_LENGTH: usize = 10_000;

/// Longest name derived, in UTF-16 units as the extension counts them.
const MAX_NAME_LENGTH: usize = 500;

/// Palette p16, approved on 16 October 2026. Changing it changes
/// every picture.
pub const PALETTE: [&str; 16] = [
    "#1A1A1A", "#332288", "#0077BB", "#77AADD", "#99DDFF", "#225555", "#009E73", "#666633",
    "#F0E442", "#E69F00", "#D55E00", "#663333", "#CC79A7", "#AA4499", "#777777", "#F4EFE6",
];

/// `CONTRAST[i]`: the palette colours that may sit next to colour `i`, a
/// shape on its square or the neighbouring ring.
pub const CONTRAST: [&[u8]; 16] = [
    &[2, 3, 4, 6, 7, 8, 9, 10, 12, 13, 14, 15],
    &[3, 4, 6, 7, 8, 9, 10, 11, 12, 14, 15],
    &[0, 4, 7, 8, 9, 10, 11, 14, 15],
    &[0, 1, 5, 7, 8, 9, 10, 11, 14, 15],
    &[0, 1, 2, 5, 6, 7, 8, 9, 10, 11, 13, 14],
    &[3, 4, 6, 7, 8, 9, 10, 12, 15],
    &[0, 1, 4, 5, 8, 11, 13, 15],
    &[0, 1, 2, 3, 4, 5, 8, 9, 12, 13, 15],
    &[0, 1, 2, 3, 4, 5, 6, 7, 10, 11, 12, 13, 14],
    &[0, 1, 2, 3, 4, 5, 7, 11, 13, 14, 15],
    &[0, 1, 2, 3, 4, 5, 8, 11, 14, 15],
    &[1, 2, 3, 4, 6, 8, 9, 10, 12, 14, 15],
    &[0, 1, 5, 7, 8, 11, 15],
    &[0, 4, 6, 7, 8, 9, 15],
    &[0, 1, 2, 3, 4, 8, 9, 10, 11, 15],
    &[0, 1, 2, 3, 5, 6, 7, 9, 10, 11, 12, 13, 14],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Shape {
    Circle,
    Square,
    Triangle,
    Diamond,
    HalfDisc,
    Ring,
    Bar,
    Cross,
}

pub const SHAPES: [Shape; 8] = [
    Shape::Circle,
    Shape::Square,
    Shape::Triangle,
    Shape::Diamond,
    Shape::HalfDisc,
    Shape::Ring,
    Shape::Bar,
    Shape::Cross,
];

impl Shape {
    /// The extension's name for it.
    pub fn name(self) -> &'static str {
        match self {
            Shape::Circle => "circle",
            Shape::Square => "square",
            Shape::Triangle => "triangle",
            Shape::Diamond => "diamond",
            Shape::HalfDisc => "half-disc",
            Shape::Ring => "ring",
            Shape::Bar => "bar",
            Shape::Cross => "cross",
        }
    }
}

const TURNS: u128 = 4;
const SQUARES: usize = 4;
const RINGS: usize = 8;
const RING_PROFILES: u128 = 4;

/// What a stand-in is for. Songs, podcasts, episodes and the like are
/// named as albums (see [`super::name_kind`]), so a single and its only
/// song share one stand-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Track,
    Album,
    Artist,
    Playlist,
    User,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Track => "track",
            Kind::Album => "album",
            Kind::Artist => "artist",
            Kind::Playlist => "playlist",
            Kind::User => "user",
        }
    }

    pub fn from_label(label: &str) -> Option<Self> {
        Some(match label {
            "track" => Kind::Track,
            "album" => Kind::Album,
            "artist" => Kind::Artist,
            "playlist" => Kind::Playlist,
            "user" => Kind::User,
            _ => return None,
        })
    }

    fn words(self) -> usize {
        if self == Kind::Artist { 3 } else { 2 }
    }

    /// Artists and people get rings; everything else a cover.
    pub fn round(self) -> bool {
        matches!(self, Kind::Artist | Kind::User)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Square {
    pub background: u8,
    pub color: u8,
    pub shape: Shape,
    pub turn: u8,
}

/// A generated picture: a 2 x 2 cover, or eight rings for a person.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Picture {
    Cover([Square; SQUARES]),
    Rings { rings: [u8; RINGS], profile: u8 },
}

/// One name's stand-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandIn {
    pub kind: Kind,
    /// "Mailbox Manatee Yack".
    pub name: String,
    pub picture: Picture,
    pub digest: [u8; 32],
}

/// Successive remainders of one big-endian integer.
struct Drawer(u128);

impl Drawer {
    fn new(bytes: &[u8]) -> Self {
        Self(
            bytes
                .iter()
                .fold(0u128, |n, byte| (n << 8) | u128::from(*byte)),
        )
    }

    fn draw(&mut self, count: usize) -> usize {
        let count = count as u128;
        let picked = self.0 % count;
        self.0 /= count;
        picked as usize
    }
}

/// JavaScript's `\s`: what the extension collapses and trims. It differs
/// from Rust's whitespace: U+FEFF is in, U+0085 is out.
pub(super) fn js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{0B}' | '\u{0C}' | '\r' | ' ' | '\u{A0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

/// The same name however it is spaced or capitalised: NFC, each run of
/// whitespace one space, trimmed, lower case.
pub fn normalize_name(name: &str) -> String {
    let composed: String = name.nfc().collect();
    let mut spaced = String::with_capacity(composed.len());
    let mut in_space = false;
    for c in composed.chars() {
        if js_space(c) {
            if !in_space {
                spaced.push(' ');
            }
            in_space = true;
        } else {
            spaced.push(c);
            in_space = false;
        }
    }
    // The run collapsed to one space is trimmed like any other.
    spaced.trim_matches(' ').to_lowercase()
}

/// The word list, checked against its pinned checksum.
fn words() -> &'static [&'static str] {
    static WORDS: OnceLock<Vec<&'static str>> = OnceLock::new();
    WORDS.get_or_init(|| {
        let actual = hex(&Sha256::digest(WORDLIST));
        assert_eq!(
            actual, WORDLIST_SHA256,
            "Vacuum's word list does not match its pinned checksum; refusing to run"
        );
        let text = std::str::from_utf8(WORDLIST).expect("the word list is UTF-8");
        let words: Vec<&str> = text
            .lines()
            .map(|line| line.split('\t').next().unwrap_or_default())
            .collect();
        assert_eq!(words.len(), WORDLIST_LENGTH, "the word list's length");
        words
    })
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

type HmacSha256 = Hmac<Sha256>;

/// Derives stand-ins for one key, each distinct name once.
pub struct Deriver {
    key: [u8; 32],
    cache: std::sync::Mutex<HashMap<(Kind, String), StandIn>>,
}

impl Deriver {
    pub fn new(key: [u8; 32]) -> Self {
        // Check the word list now rather than on the first name.
        let _ = words();
        Self {
            key,
            cache: Default::default(),
        }
    }

    /// A keyed digest of any label; pictures use `picture:<kind>:<id>#<n>`.
    pub fn mac(&self, label: &str) -> [u8; 32] {
        let mut mac = HmacSha256::new_from_slice(&self.key).expect("HMAC takes any key length");
        mac.update(label.as_bytes());
        mac.finalize().into_bytes().into()
    }

    /// The stand-in for `name` as a `kind`, or `None` for an empty or
    /// overlong name.
    pub fn derive(&self, kind: Kind, name: &str) -> Option<StandIn> {
        let normalized = normalize_name(name);
        if normalized.is_empty() || normalized.encode_utf16().count() > MAX_NAME_LENGTH {
            return None;
        }
        let key = (kind, normalized);
        if let Some(found) = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&key)
        {
            return Some(found.clone());
        }
        let digest = self.mac(&format!("{}:{}", kind.label(), key.1));
        let list = words();
        let mut draw = Drawer::new(&digest[..16]);
        let name = (0..kind.words())
            .map(|_| title_case(list[draw.draw(list.len())]))
            .collect::<Vec<_>>()
            .join(" ");
        let picture = picture_from(&digest[16..], kind.round());
        let stand_in = StandIn {
            kind,
            name,
            picture,
            digest,
        };
        let mut cache = self
            .cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Bounded: a long session sees many names, each cheap to derive again.
        if cache.len() > 50_000 {
            cache.clear();
        }
        cache.insert(key, stand_in.clone());
        Some(stand_in)
    }
}

/// The picture drawn from a digest's second half: eight rings, outermost
/// first, each differing clearly from the one outside it; or four squares.
pub fn picture_from(bytes: &[u8], round: bool) -> Picture {
    let mut draw = Drawer::new(bytes);
    if round {
        let mut rings = [0u8; RINGS];
        rings[0] = draw.draw(PALETTE.len()) as u8;
        for index in 1..RINGS {
            rings[index] = next_to(&mut draw, rings[index - 1]);
        }
        Picture::Rings {
            rings,
            profile: draw.draw(RING_PROFILES as usize) as u8,
        }
    } else {
        Picture::Cover(std::array::from_fn(|_| {
            let background = draw.draw(PALETTE.len()) as u8;
            Square {
                background,
                color: next_to(&mut draw, background),
                shape: SHAPES[draw.draw(SHAPES.len())],
                turn: draw.draw(TURNS as usize) as u8,
            }
        }))
    }
}

fn next_to(draw: &mut Drawer, color: u8) -> u8 {
    let allowed = CONTRAST[usize::from(color)];
    allowed[draw.draw(allowed.len())]
}

fn title_case(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Two frame colours for an artist picture from a candidate's bytes: the
/// outer ring and one that contrasts with it.
pub fn frame(bytes: &[u8; 32]) -> [u8; 2] {
    let outer = bytes[16] % PALETTE.len() as u8;
    let ring = CONTRAST[usize::from(outer)];
    [outer, ring[usize::from(bytes[17]) % ring.len()]]
}
