//! Which museum artwork stands in for a cover or an artist picture, ported
//! exactly from the extension's `core/museums.js`. From the key and the
//! Spotify ID of a release, artist or playlist comes a fixed chain of
//! candidates:
//!
//! ```text
//! bytes_n = HMAC-SHA-256(key, "picture:" + kind + ":" + id + "#" + n)
//! museum  = bytes 0..8 as a number, mod the sum of the weights
//! object  = bytes 8..16 as a number, mod the museum's ID range
//! ```
//!
//! The first candidate that exists, is free to use and has an image wins.
//! The pick is then kept for good, so a museum changing later cannot move
//! a picture.

use super::derive::{Deriver, frame};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Museum {
    /// The Metropolitan Museum of Art.
    Met,
    /// The Art Institute of Chicago.
    Aic,
    /// The Cleveland Museum of Art.
    Cma,
}

struct Range {
    museum: Museum,
    weight: u64,
    min: u64,
    max: u64,
}

/// Each museum's count of free images with pictures, and its ID range,
/// checked against each API on 17 October 2026.
const MUSEUMS: [Range; 3] = [
    Range {
        museum: Museum::Met,
        weight: 371_112,
        min: 1,
        max: 900_000,
    },
    Range {
        museum: Museum::Aic,
        weight: 59_063,
        min: 1,
        max: 288_000,
    },
    Range {
        museum: Museum::Cma,
        weight: 41_820,
        min: 90_000,
        max: 180_000,
    },
];

const TOTAL_WEIGHT: u64 = 371_112 + 59_063 + 41_820;

/// After this many candidates the generated picture stays.
pub const MAX_CANDIDATES: u32 = 40;

/// The width the Art Institute's image server is asked for.
const AIC_WIDTH: u32 = 400;

/// One link in a picture's chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub museum: Museum,
    pub object: u64,
    /// The two frame colours an artist picture gets from this candidate.
    pub frame: [u8; 2],
}

pub fn label(kind: &str, id: &str, n: u32) -> String {
    format!("picture:{kind}:{id}#{n}")
}

fn number(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0u64, |n, byte| (n << 8) | u64::from(*byte))
}

/// Candidate `n` for the picture of `kind` (`album`, `artist`,
/// `playlist` or `track`) with Spotify ID `id`.
pub fn candidate(deriver: &Deriver, kind: &str, id: &str, n: u32) -> Candidate {
    let bytes = deriver.mac(&label(kind, id, n));
    let mut pick = number(&bytes[..8]) % TOTAL_WEIGHT;
    let mut range = &MUSEUMS[MUSEUMS.len() - 1];
    for museum in &MUSEUMS {
        if pick < museum.weight {
            range = museum;
            break;
        }
        pick -= museum.weight;
    }
    let span = range.max - range.min + 1;
    Candidate {
        museum: range.museum,
        object: range.min + number(&bytes[8..16]) % span,
        frame: frame(&bytes),
    }
}

/// With a list of picture addresses of your own, the entry a picture uses,
/// and its frame colours.
pub fn custom_pick(
    deriver: &Deriver,
    kind: &str,
    id: &str,
    count: usize,
) -> Option<(usize, [u8; 2])> {
    if count == 0 {
        return None;
    }
    let bytes = deriver.mac(&label(kind, id, 0));
    Some(((number(&bytes[..8]) % count as u64) as usize, frame(&bytes)))
}

/// Where to look an object up. The Art Institute is asked in batches, see
/// [`aic_batch_url`].
pub fn lookup_url(museum: Museum, object: u64) -> Option<String> {
    match museum {
        Museum::Met => Some(format!(
            "https://collectionapi.metmuseum.org/public/collection/v1/objects/{object}"
        )),
        Museum::Cma => Some(format!(
            "https://openaccess-api.clevelandart.org/api/artworks/{object}"
        )),
        Museum::Aic => None,
    }
}

/// The Art Institute answers up to 100 IDs at once and leaves out those
/// that do not exist.
pub fn aic_batch_url(objects: &[u64]) -> String {
    let ids: Vec<String> = objects.iter().map(u64::to_string).collect();
    format!(
        "https://api.artic.edu/api/v1/artworks?ids={}&fields=id,is_public_domain,image_id&limit=100",
        ids.join(",")
    )
}

/// The Art Institute asks callers to say who they are.
pub const AIC_USER_AGENT: &str =
    "unfamiliar sounds (personal, non-commercial listening experiment)";

/// The image of a Met object, if it is public domain and has one.
pub fn met_image(json: &serde_json::Value) -> Option<String> {
    (json.get("isPublicDomain")? == true)
        .then(|| json.get("primaryImageSmall")?.as_str())
        .flatten()
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
}

/// The image of a Cleveland object, if it is CC0 and has one.
pub fn cma_image(json: &serde_json::Value) -> Option<String> {
    let data = json.get("data")?;
    (data.get("share_license_status")?.as_str()? == "CC0")
        .then(|| data.pointer("/images/web/url")?.as_str())
        .flatten()
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
}

/// The image of one record in an Art Institute batch, if public domain.
pub fn aic_image(record: &serde_json::Value) -> Option<String> {
    let image = record
        .get("image_id")?
        .as_str()
        .filter(|id| !id.is_empty())?;
    (record.get("is_public_domain")? == true)
        .then(|| format!("https://www.artic.edu/iiif/2/{image}/full/{AIC_WIDTH},/0/default.jpg"))
}

/// Only images on the museums' own image servers are fetched.
pub fn is_museum_image(url: &str) -> bool {
    [
        "https://images.metmuseum.org/",
        "https://www.artic.edu/iiif/",
        "https://openaccess-cdn.clevelandart.org/",
    ]
    .iter()
    .any(|host| url.starts_with(host))
}
