//! Is a playlist about a theme, a person or popularity? Rules only, ported
//! from the extension's `core/collections.js`.
//!
//! - person: built around one artist or public figure. Hidden.
//! - popularity: charts and top lists. Hidden.
//! - unsure: can't tell. Hidden, failing closed.
//! - theme: a mood, activity, genre or idea. Shown.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Person,
    Popularity,
    Unsure,
    Theme,
}

impl Verdict {
    pub fn shown(self) -> bool {
        self == Verdict::Theme
    }

    /// Hidden verdicts that stick once reached: the evidence for them, such
    /// as track rows, is not on every page.
    pub fn sticky(self) -> bool {
        matches!(self, Verdict::Person | Verdict::Popularity)
    }
}

/// What is known about a playlist, all of it real.
#[derive(Clone, Debug, Default)]
pub struct Evidence<'a> {
    pub title: &'a str,
    pub description: &'a str,
    /// Names of the playlist's own artists.
    pub names: Vec<&'a str>,
    /// Artist names seen anywhere this session.
    pub session_names: Vec<&'a str>,
    /// The artists of each track row seen, by ID.
    pub rows: Vec<Vec<&'a str>>,
    /// Spotify's format for its own playlists, when known.
    pub format: &'a str,
}

/// Spotify's own playlists have IDs that start so.
pub const SPOTIFY_ID_PREFIX: &str = "37i9dQZF1";

pub fn is_spotify_playlist(id: &str) -> bool {
    id.starts_with(SPOTIFY_ID_PREFIX)
}

const DOMINANT_SHARE: f64 = 0.5;
const DOMINANT_MIN_TRACKS: usize = 8;
/// Names shorter than this are not looked for in titles.
const MIN_NAME_LENGTH: usize = 3;

struct Rules {
    title_person: Vec<Regex>,
    title_popularity: Vec<Regex>,
    description_person: Vec<Regex>,
    description_popularity: Vec<Regex>,
    list_start: Regex,
    list_end: Regex,
    sentence: Regex,
    split: Regex,
}

/// JavaScript's `\s`, which differs from Rust's: U+FEFF is in, U+0085 out.
const WS: &str = r"[\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";

fn js_trim(text: &str) -> &str {
    text.trim_matches(super::derive::js_space)
}

fn compile(patterns: &[&str]) -> Vec<Regex> {
    patterns
        .iter()
        .map(|pattern| Regex::new(pattern).expect("a valid rule"))
        .collect()
}

// The extension's patterns are JavaScript regular expressions without the
// `u` flag: `\b`, `\d` and case-insensitivity are ASCII, written here with
// `(?i-u)`. Its `\s` is JavaScript's whitespace, written out as `WS`.
fn rules() -> &'static Rules {
    static RULES: OnceLock<Rules> = OnceLock::new();
    RULES.get_or_init(|| Rules {
        title_person: compile(&[
            r"(?i-u)^this is ",
            r"(?i-u) radio$",
            r"(?i-u)^best of ",
            r"(?i-u)'s picks$",
        ]),
        title_popularity: compile(&[
            r"(?i-u)\btop (\d+|hits|songs|tracks|artists)\b",
            r"(?i-u)\bviral \d+",
            r"(?i-u)\bcharts?\b",
            r"(?i-u)\bhot(test)? \d+",
            r"(?i-u)\bhot hits\b",
        ]),
        description_person: compile(&[
            r"(?i-u)^this is ",
            r"(?i-u)\bessential (tracks|songs)\b",
            r"(?i-u)\bbiggest (hits|songs)\b",
            r"(?i-u)\bgreatest hits\b",
            r"(?i-u)\bpicked by\b",
            r"(?i-u)\ball in one playlist\b",
            r"(?i-u)\bbest of\b",
        ]),
        description_popularity: compile(&[
            r"(?i-u)\bhottest \d+\b",
            r"(?i-u)\bmost (played|streamed)\b",
            r"(?i-u)\bcharts?\b",
        ]),
        list_start: Regex::new(&format!(r"(?i-u:^with){WS}+")).expect("rule"),
        list_end: Regex::new(&format!(r",?{WS}+(?i-u:and more)$")).expect("rule"),
        sentence: Regex::new(r"[.!?:]").expect("rule"),
        split: Regex::new(&format!(r"{WS}*,{WS}*|{WS}+and{WS}+|{WS}+&{WS}+")).expect("rule"),
    })
}

/// "With A, B and C" or "A, B and more" give their names; anything that
/// reads like a sentence gives none.
pub fn names_in_description(description: &str) -> Vec<String> {
    let rules = rules();
    let text = js_trim(description);
    let looks_like_list = rules.list_start.is_match(text) || rules.list_end.is_match(text);
    if text.is_empty() || (!looks_like_list && rules.sentence.is_match(text)) {
        return Vec::new();
    }
    let text = rules.list_start.replace(text, "");
    let text = rules.list_end.replace(&text, "");
    rules
        .split
        .split(&text)
        .map(js_trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Whether `title` names `name` as a whole phrase, ignoring case, bounded
/// by anything that is not a letter or digit (JavaScript's `/iu`). Each
/// place the name occurs is tried, overlapping ones included; the Unicode
/// classes are compiled once, not once per name.
pub fn title_names(title: &str, name: &str) -> bool {
    static WORD: OnceLock<Regex> = OnceLock::new();
    let word = WORD.get_or_init(|| Regex::new(r"^[\p{L}\p{N}]$").expect("rule"));
    let is_word = |c: char| word.is_match(c.encode_utf8(&mut [0; 4]));
    // Each name is compiled once: a session sees the same names against
    // many titles.
    static NAMES: OnceLock<std::sync::Mutex<HashMap<String, Option<Regex>>>> = OnceLock::new();
    let pattern = {
        let mut names = NAMES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if names.len() > 20_000 {
            names.clear();
        }
        names
            .entry(name.to_string())
            .or_insert_with(|| {
                regex::RegexBuilder::new(&regex::escape(name))
                    .case_insensitive(true)
                    .build()
                    .ok()
            })
            .clone()
    };
    let Some(pattern) = pattern else {
        return false;
    };
    let mut start = 0;
    while start <= title.len() {
        let Some(found) = pattern.find_at(title, start) else {
            return false;
        };
        let before = title[..found.start()].chars().next_back();
        let after = title[found.end()..].chars().next();
        if before.is_none_or(|c| !is_word(c)) && after.is_none_or(|c| !is_word(c)) {
            return true;
        }
        start = found.start()
            + title[found.start()..]
                .chars()
                .next()
                .map_or(1, char::len_utf8);
    }
    false
}

/// The share of rows on which the most frequent artist appears.
pub fn dominant_share(rows: &[Vec<&str>]) -> f64 {
    if rows.is_empty() {
        return 0.0;
    }
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for artists in rows {
        for artist in artists.iter().copied().collect::<HashSet<_>>() {
            *counts.entry(artist).or_default() += 1;
        }
    }
    counts.values().copied().max().unwrap_or(0) as f64 / rows.len() as f64
}

/// The verdict on a playlist, with the rule that decided it.
pub fn classify(evidence: &Evidence) -> (Verdict, &'static str) {
    let rules = rules();
    let title = js_trim(evidence.title);
    let description = js_trim(evidence.description);
    match evidence.format {
        "inspiredby-mix" | "artist-mix-reader" | "artistsets" => {
            return (Verdict::Person, "0 format");
        }
        "chart" => return (Verdict::Popularity, "0 format"),
        _ => {}
    }
    if title.is_empty() {
        return (Verdict::Unsure, "no title yet");
    }
    if rules.title_person.iter().any(|rule| rule.is_match(title)) {
        return (Verdict::Person, "1 title pattern");
    }
    if rules
        .title_popularity
        .iter()
        .any(|rule| rule.is_match(title))
    {
        return (Verdict::Popularity, "1 title pattern");
    }
    // Rules 2 and 5: the title names an artist.
    let mut candidates: Vec<String> = names_in_description(description);
    candidates.extend(evidence.names.iter().map(|name| name.to_string()));
    candidates.extend(evidence.session_names.iter().map(|name| name.to_string()));
    let mut weak = false;
    let mut seen = HashSet::new();
    for name in candidates {
        if !seen.insert(name.clone()) {
            continue;
        }
        if name.encode_utf16().count() < MIN_NAME_LENGTH || !title_names(title, &name) {
            continue;
        }
        if js_trim(&name).contains(super::derive::js_space) {
            return (Verdict::Person, "2 title names an artist");
        }
        // A one-word name may just be a word.
        weak = true;
    }
    if evidence.rows.len() >= DOMINANT_MIN_TRACKS
        && dominant_share(&evidence.rows) >= DOMINANT_SHARE
    {
        return (Verdict::Person, "3 one artist dominates");
    }
    if rules
        .description_person
        .iter()
        .any(|rule| rule.is_match(description))
    {
        return (Verdict::Person, "4 description");
    }
    if rules
        .description_popularity
        .iter()
        .any(|rule| rule.is_match(description))
    {
        return (Verdict::Popularity, "4 description");
    }
    if weak {
        return (Verdict::Unsure, "5 one-word name in title");
    }
    (Verdict::Theme, "6 none of the above")
}
