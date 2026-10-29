//! The guessing game: on shuffle, name as much of a song as you can before
//! its first lyric. Title 1 point, artist 2, album 3, year 4, version 5,
//! each scored on its own. Or pick its cover from a grid, or say what
//! colour its cover mostly is. Songs missed come back after a day, then
//! three, then seven. Everything stays on this computer.
//!
//! This module has no interface: scoring, matching typed answers, the
//! deadline, the record of rounds and the reviews. Hiding the song while it
//! is guessed is Vacuum's gate, at the level the player picks.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::api::models::Track;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Title,
    Artist,
    Album,
    Year,
    Version,
}

pub const CATEGORIES: [Category; 5] = [
    Category::Title,
    Category::Artist,
    Category::Album,
    Category::Year,
    Category::Version,
];

impl Category {
    pub fn points(self) -> u32 {
        match self {
            Category::Title => 1,
            Category::Artist => 2,
            Category::Album => 3,
            Category::Year => 4,
            Category::Version => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Name it, ticking what you got once the song is revealed.
    #[default]
    Honor,
    /// Name it, typing the answers. Only these count in the stats.
    Typed,
    /// Pick the song's cover from a grid.
    CoverGrid,
    /// Say what colour the cover mostly is.
    MainColour,
}

/// What a song really is, read from Spotify's data, never guessed: a
/// version Spotify does not name is unknown.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Truth {
    pub title: String,
    pub artists: Vec<String>,
    pub album: String,
    pub year: Option<String>,
    pub version: Option<String>,
}

/// Words that mark a part of a title as the version rather than the song.
const VERSION_WORDS: [&str; 19] = [
    "remaster",
    "remastered",
    "live",
    "edit",
    "version",
    "mix",
    "remix",
    "mono",
    "stereo",
    "acoustic",
    "demo",
    "deluxe",
    "anniversary",
    "instrumental",
    "take",
    "session",
    "unplugged",
    "extended",
    "single",
];

impl Truth {
    pub fn of(track: &Track) -> Self {
        let (title, version) = split_version(&track.name);
        Self {
            title,
            artists: track
                .artists
                .iter()
                .map(|artist| artist.name.clone())
                .collect(),
            album: track
                .album
                .as_ref()
                .map(|album| split_version(&album.name).0)
                .unwrap_or_default(),
            year: track
                .album
                .as_ref()
                .and_then(|album| album.release_date.as_deref())
                .and_then(|date| date.get(..4))
                .filter(|year| year.bytes().all(|byte| byte.is_ascii_digit()))
                .map(str::to_owned),
            version,
        }
    }
}

/// A title without its version, and the version: "Heroes - 2017 Remaster"
/// is "Heroes" and "2017 Remaster"; "Song (Live)" is "Song" and "Live";
/// "Song - Live - 2011 Remaster" is "Song" and "Live - 2011 Remaster". A
/// part that names no version, such as "(feat. Someone)", stays a part of
/// the title.
pub fn split_version(title: &str) -> (String, Option<String>) {
    let names_version = |part: &str| {
        let words = normalize(part);
        words.split(' ').any(|word| VERSION_WORDS.contains(&word))
    };
    let mut base = title.trim().to_string();
    let mut parts: Vec<String> = Vec::new();
    loop {
        let dash = base
            .rsplit_once(" - ")
            .filter(|(_, part)| names_version(part))
            .map(|(rest, part)| (rest.trim().to_string(), part.trim().to_string()));
        let bracket = || {
            [('(', ')'), ('[', ']')]
                .into_iter()
                .find_map(|(open, close)| {
                    let (rest, part) = base.strip_suffix(close)?.rsplit_once(open)?;
                    names_version(part).then(|| (rest.trim().to_string(), part.trim().to_string()))
                })
        };
        match dash.or_else(bracket) {
            Some((rest, part)) if !rest.is_empty() => {
                parts.insert(0, part);
                base = rest;
            }
            _ => break,
        }
    }
    let version = (!parts.is_empty()).then(|| parts.join(" - "));
    (base, version)
}

/// An answer as it is compared: lower case, without accents, punctuation,
/// "feat." or "(with ...)" credits, "&" as "and", each run of spaces one
/// space.
pub fn normalize(text: &str) -> String {
    let mut text: String = text
        .nfd()
        .filter(|c| !unicode_normalization::char::is_combining_mark(*c))
        .collect::<String>()
        .to_lowercase()
        .replace('&', " and ");
    // "feat. X", "ft. X", "featuring X" credit someone else, whether in
    // brackets or after a dash.
    for credit in [
        "(feat.",
        "(ft.",
        "[feat.",
        "(featuring",
        "(with ",
        "[with ",
        " feat. ",
        " ft. ",
        " featuring ",
    ] {
        if let Some(at) = text.find(credit) {
            text.truncate(at);
        }
    }
    text.chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(|word| match word {
            "remastered" => "remaster",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Edits between two strings, by characters.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, a) in a.chars().enumerate() {
        let mut previous = row[0];
        row[0] = i + 1;
        for (j, b) in b.iter().enumerate() {
            let substitution = previous + usize::from(a != *b);
            previous = row[j + 1];
            row[j + 1] = substitution.min(row[j] + 1).min(previous + 1);
        }
    }
    row[b.len()]
}

/// Whether a typed answer names `truth`: the same after normalizing, or a
/// slip of a letter or two in a longer name. Numbers must be exact: 1998 is
/// not 1999, and a 2009 remaster is not the 2011 one.
pub fn matches(guess: &str, truth: &str) -> bool {
    close(&normalize(guess), &normalize(truth))
}

/// Whether a typed version names `truth`, its words in any order:
/// "Remastered 2011" is "2011 Remaster", and "Live Version" is "Live".
pub fn matches_version(guess: &str, truth: &str) -> bool {
    let words = |text: &str| {
        let normalized = normalize(text);
        let mut words: Vec<&str> = normalized
            .split(' ')
            .filter(|word| !word.is_empty() && *word != "version")
            .collect();
        words.sort_unstable();
        words.join(" ")
    };
    close(&words(guess), &words(truth))
}

fn close(guess: &str, truth: &str) -> bool {
    if guess.is_empty() || truth.is_empty() {
        return false;
    }
    if guess == truth {
        return true;
    }
    let digits = |text: &str| {
        text.chars()
            .filter(char::is_ascii_digit)
            .collect::<String>()
    };
    if digits(guess) != digits(truth) {
        return false;
    }
    let length = truth.chars().count();
    length >= 4 && distance(guess, truth) <= (length / 6).max(1)
}

/// Typed answers, by category; an empty one is no answer.
pub type Answers = HashMap<Category, String>;

/// Which categories a typed set of answers got right. A version Spotify
/// does not name cannot be scored, and is left out.
pub fn score(truth: &Truth, answers: &Answers) -> HashMap<Category, bool> {
    let answer = |category| answers.get(&category).map(String::as_str).unwrap_or("");
    let mut got = HashMap::new();
    got.insert(
        Category::Title,
        matches(answer(Category::Title), &truth.title),
    );
    got.insert(
        Category::Artist,
        // Any one of the song's artists is its artist.
        truth
            .artists
            .iter()
            .any(|artist| matches(answer(Category::Artist), artist)),
    );
    got.insert(
        Category::Album,
        matches(answer(Category::Album), &truth.album),
    );
    if let Some(year) = &truth.year {
        got.insert(Category::Year, answer(Category::Year).trim() == year);
    }
    if let Some(version) = &truth.version {
        got.insert(
            Category::Version,
            matches_version(answer(Category::Version), version),
        );
    }
    got
}

/// Points for what was got: each category on its own.
pub fn points(got: &HashMap<Category, bool>) -> u32 {
    got.iter()
        .filter(|(_, right)| **right)
        .map(|(category, _)| category.points())
        .sum()
}

/// When the guessing stops, in milliseconds into the song: the first line
/// of its synced lyrics, or, without them (an instrumental, say), a fifth
/// of the song.
pub fn deadline_ms(first_line_ms: Option<u32>, duration_ms: u32) -> u32 {
    first_line_ms.unwrap_or(duration_ms / 5).max(1_000)
}

/// One song played in the game.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Round {
    /// When it was played, seconds since the epoch.
    pub at: i64,
    pub uri: String,
    pub mode: Mode,
    /// What was got, by category; for the cover grid and the colour, the
    /// title's place holds the one answer.
    pub got: HashMap<Category, bool>,
    /// How far into the song the answer came, in milliseconds.
    pub answer_ms: Option<u32>,
    pub points: u32,
}

/// When a missed song comes back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Review {
    /// Seconds since the epoch.
    pub due: i64,
    /// How many times it has come back: after a day, then three, then seven.
    pub step: u8,
}

const DAY: i64 = 24 * 60 * 60;
const STEPS: [i64; 3] = [1, 3, 7];

/// How much of a song shows while it is guessed: Vacuum's levels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Hiding {
    /// The real names and covers.
    Real,
    /// Vacuum's stand-ins and pictures.
    Disguised,
    /// "???" and black squares.
    #[default]
    Blanked,
}

/// How the next game is played, kept from game to game.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Setup {
    pub mode: Mode,
    pub hiding: Hiding,
    /// How many covers the grid shows: 4, 9, 16 or 25.
    pub grid: u8,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            mode: Mode::Honor,
            hiding: Hiding::Blanked,
            grid: 9,
        }
    }
}

pub const GRID_SIZES: [u8; 4] = [4, 9, 16, 25];

impl Setup {
    /// The setup as kept, with a grid size the game offers: a file edited
    /// by hand cannot ask for another.
    pub fn valid(mut self) -> Self {
        if !GRID_SIZES.contains(&self.grid) {
            self.grid = Setup::default().grid;
        }
        self
    }
}

/// Everything the game remembers: kept in `game.json`, never uploaded.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    pub rounds: Vec<Round>,
    pub reviews: HashMap<String, Review>,
    pub setup: Setup,
}

/// A game being played.
#[derive(Clone, Debug, Default)]
pub struct Session {
    pub setup: Setup,
    /// What it plays from: a playlist's or album's URI, Liked Songs'
    /// collection, or `LIBRARY`.
    pub source: String,
    /// The songs, in the order they play: songs due to come back first.
    pub songs: Vec<Track>,
    pub round: Option<RoundState>,
    /// The songs that have had their round: played again (Previous), they
    /// are not guessed again.
    pub done: std::collections::HashSet<String>,
    pub played: u32,
    pub points: u32,
    /// Whether the player had shuffle on, to give it back when the game
    /// ends.
    pub shuffle_before: bool,
}

impl Session {
    /// The game's song `uri`, if it has not had its round yet.
    pub fn pending(&self, uri: &str) -> Option<&Track> {
        if self.done.contains(uri) {
            return None;
        }
        self.songs.iter().find(|track| track.uri == uri)
    }
}

/// The source that plays Liked Songs and every playlist page loaded.
pub const LIBRARY: &str = "library";

/// One song being guessed.
#[derive(Clone, Debug)]
pub struct RoundState {
    pub track: Track,
    pub truth: Truth,
    pub deadline_ms: u32,
    /// Where in the song the round began.
    pub started_ms: u32,
    /// How long the song is, for the deadline once its lyrics arrive.
    pub duration_ms: u32,
    /// The deadline is the first synced lyric, not a fifth of the song.
    pub timed_by_lyrics: bool,
    pub answers: Answers,
    /// What the player says they got, in Honor mode.
    pub ticked: std::collections::HashSet<Category>,
    pub revealed: bool,
    pub recorded: bool,
    /// The cover grid: each cell's album URI and cover address.
    pub grid: Vec<(String, Option<String>)>,
    pub picked: Option<usize>,
    pub colour: Option<Colour>,
    /// The cover's main colours, once read.
    pub colours: std::sync::Arc<std::sync::Mutex<Option<Vec<Colour>>>>,
    pub colours_asked: bool,
    /// When the cache was last looked in for the cover.
    pub colours_checked: Option<std::time::Instant>,
    pub answer_ms: Option<u32>,
    pub got: HashMap<Category, bool>,
    pub points: u32,
    /// Skipped before the reveal, or impossible to score: not kept.
    pub unscored: bool,
}

impl RoundState {
    pub fn new(track: Track, deadline_ms: u32, timed_by_lyrics: bool) -> Self {
        Self {
            truth: Truth::of(&track),
            track,
            deadline_ms,
            started_ms: 0,
            duration_ms: 0,
            timed_by_lyrics,
            answers: Answers::new(),
            ticked: Default::default(),
            revealed: false,
            recorded: false,
            grid: Vec::new(),
            picked: None,
            colour: None,
            colours: Default::default(),
            colours_asked: false,
            colours_checked: None,
            answer_ms: None,
            got: HashMap::new(),
            points: 0,
            unscored: false,
        }
    }

    /// The cover's main colours, once they are read.
    pub fn colours_read(&self) -> Option<Vec<Colour>> {
        self.colours
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Whether the round can be scored at all: the cover grid needs the
    /// song's own cover among others, and the main colour needs the cover
    /// read. A round that cannot be scored is not kept.
    pub fn scoreable(&self, mode: Mode) -> bool {
        match mode {
            Mode::Typed | Mode::Honor => true,
            Mode::CoverGrid => {
                let album = self.track.album.as_ref().map(|album| album.uri.as_str());
                self.grid.len() >= 2
                    && self
                        .grid
                        .iter()
                        .any(|(uri, image)| Some(uri.as_str()) == album && image.is_some())
            }
            Mode::MainColour => self
                .colours_read()
                .is_some_and(|colours| !colours.is_empty()),
        }
    }

    /// What was got, as the mode scores it: typed answers matched, ticks
    /// taken as said, the cover or the colour as the title's one answer.
    pub fn judge(&mut self, mode: Mode) {
        self.got = match mode {
            Mode::Typed => score(&self.truth, &self.answers),
            Mode::Honor => {
                let mut got: HashMap<Category, bool> = CATEGORIES
                    .iter()
                    .map(|category| (*category, self.ticked.contains(category)))
                    .collect();
                if self.truth.year.is_none() {
                    got.remove(&Category::Year);
                }
                if self.truth.version.is_none() {
                    got.remove(&Category::Version);
                }
                got
            }
            Mode::CoverGrid => {
                let album = self.track.album.as_ref().map(|album| album.uri.as_str());
                let right = self
                    .picked
                    .and_then(|index| self.grid.get(index))
                    .is_some_and(|(uri, _)| Some(uri.as_str()) == album);
                [(Category::Title, right)].into()
            }
            Mode::MainColour => {
                let colours = self.colours_read().unwrap_or_default();
                let right = self.colour.is_some_and(|colour| colours.contains(&colour));
                [(Category::Title, right)].into()
            }
        };
        self.points = match mode {
            Mode::Typed | Mode::Honor => points(&self.got),
            Mode::CoverGrid | Mode::MainColour => u32::from(self.got.values().any(|got| *got)),
        };
    }
}

/// Rounds the stats wait for before showing anything.
pub const STATS_AFTER: usize = 10;

impl Record {
    /// Keeps a round, and schedules the song again if it was missed: a
    /// day after a miss, then, got right each time it comes back, three
    /// days and seven, and then it is done with. A miss when it comes back
    /// starts it over; a miss before it is due changes nothing, so playing
    /// it early never puts it off.
    pub fn keep(&mut self, round: Round) {
        let missed = round.got.get(&Category::Title).is_some_and(|got| !got);
        let at = round.at;
        match (missed, self.reviews.get(&round.uri).copied()) {
            (_, Some(review)) if review.due > at => {}
            (true, _) => {
                self.reviews.insert(
                    round.uri.clone(),
                    Review {
                        due: at + STEPS[0] * DAY,
                        step: 0,
                    },
                );
            }
            (false, Some(review)) => {
                if usize::from(review.step) + 1 >= STEPS.len() {
                    self.reviews.remove(&round.uri);
                } else {
                    self.reviews.insert(
                        round.uri.clone(),
                        Review {
                            due: at + STEPS[usize::from(review.step) + 1] * DAY,
                            step: review.step + 1,
                        },
                    );
                }
            }
            (false, None) => {}
        }
        self.rounds.push(round);
    }

    /// Whether `uri` is due to come back at `now`.
    pub fn due(&self, uri: &str, now: i64) -> bool {
        self.reviews
            .get(uri)
            .is_some_and(|review| review.due <= now)
    }

    /// The stats for typed Name it rounds, once there are enough of them.
    pub fn stats(&self) -> Option<Stats> {
        let typed: Vec<&Round> = self
            .rounds
            .iter()
            .filter(|round| round.mode == Mode::Typed)
            .collect();
        if typed.len() < STATS_AFTER {
            return None;
        }
        let accuracy = CATEGORIES
            .iter()
            .filter_map(|category| {
                let scored: Vec<bool> = typed
                    .iter()
                    .filter_map(|round| round.got.get(category).copied())
                    .collect();
                (!scored.is_empty()).then(|| {
                    (
                        *category,
                        scored.iter().filter(|got| **got).count() as f32 / scored.len() as f32,
                    )
                })
            })
            .collect();
        let times: Vec<u32> = typed.iter().filter_map(|round| round.answer_ms).collect();
        Some(Stats {
            songs: typed.len(),
            accuracy,
            average_answer_ms: (!times.is_empty()).then(|| {
                (times.iter().map(|ms| u64::from(*ms)).sum::<u64>() / times.len() as u64) as u32
            }),
            points_per_song: typed.iter().map(|round| round.points).sum::<u32>() as f32
                / typed.len() as f32,
            // A week of rounds against the week before, to show progress.
            recent_points: weekly_points(&typed),
        })
    }
}

/// The stats shown once there are enough typed rounds.
#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub songs: usize,
    /// Share right, by category, of the rounds that could score it.
    pub accuracy: Vec<(Category, f32)>,
    pub average_answer_ms: Option<u32>,
    pub points_per_song: f32,
    /// Points per song in each of the last eight weeks with rounds, oldest
    /// first.
    pub recent_points: Vec<f32>,
}

fn weekly_points(rounds: &[&Round]) -> Vec<f32> {
    let mut weeks: std::collections::BTreeMap<i64, (u32, u32)> = Default::default();
    for round in rounds {
        let week = weeks.entry(round.at.div_euclid(7 * DAY)).or_default();
        week.0 += round.points;
        week.1 += 1;
    }
    let mut points: Vec<f32> = weeks
        .values()
        .map(|(sum, count)| *sum as f32 / *count as f32)
        .collect();
    if points.len() > 8 {
        points.drain(..points.len() - 8);
    }
    points
}

/// The colours a cover can be said to mostly be, as hues and the greys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Colour {
    Red,
    Orange,
    Yellow,
    Green,
    Teal,
    Blue,
    Purple,
    Pink,
    Brown,
    Black,
    Grey,
    White,
}

pub const COLOURS: [Colour; 12] = [
    Colour::Red,
    Colour::Orange,
    Colour::Yellow,
    Colour::Green,
    Colour::Teal,
    Colour::Blue,
    Colour::Purple,
    Colour::Pink,
    Colour::Brown,
    Colour::Black,
    Colour::Grey,
    Colour::White,
];

impl Colour {
    /// How the swatch is drawn.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Colour::Red => [214, 48, 49],
            Colour::Orange => [235, 132, 40],
            Colour::Yellow => [240, 210, 60],
            Colour::Green => [70, 160, 70],
            Colour::Teal => [40, 160, 160],
            Colour::Blue => [50, 100, 210],
            Colour::Purple => [130, 70, 190],
            Colour::Pink => [225, 110, 170],
            Colour::Brown => [125, 80, 45],
            Colour::Black => [20, 20, 20],
            Colour::Grey => [128, 128, 128],
            Colour::White => [240, 240, 240],
        }
    }

    /// The colour an average cover pixel colour is closest to: greys by
    /// lightness, brown for dark oranges, the rest by hue.
    pub fn nearest(rgb: [u8; 3]) -> Colour {
        let [r, g, b] = rgb.map(|channel| f32::from(channel) / 255.0);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        let lightness = (max + min) / 2.0;
        let saturation = if max == min {
            0.0
        } else {
            (max - min) / (1.0 - (2.0 * lightness - 1.0).abs())
        };
        if saturation < 0.18 || max - min < 0.08 {
            return if lightness < 0.2 {
                Colour::Black
            } else if lightness > 0.8 {
                Colour::White
            } else {
                Colour::Grey
            };
        }
        if lightness < 0.12 {
            return Colour::Black;
        }
        let hue = if max == r {
            60.0 * (((g - b) / (max - min)).rem_euclid(6.0))
        } else if max == g {
            60.0 * ((b - r) / (max - min) + 2.0)
        } else {
            60.0 * ((r - g) / (max - min) + 4.0)
        };
        match hue {
            h if !(15.0..345.0).contains(&h) => Colour::Red,
            h if h < 45.0 => {
                if lightness < 0.35 {
                    Colour::Brown
                } else {
                    Colour::Orange
                }
            }
            h if h < 70.0 => Colour::Yellow,
            h if h < 160.0 => Colour::Green,
            h if h < 195.0 => Colour::Teal,
            h if h < 255.0 => Colour::Blue,
            h if h < 290.0 => Colour::Purple,
            _ => Colour::Pink,
        }
    }
}

/// The colours a cover mostly is: each pixel of a small copy is binned to
/// its nearest named colour, and the largest bin wins. A bin nearly as
/// large (four fifths of it) counts too, so a cover split between two
/// colours takes either.
pub fn cover_colours(bytes: &[u8]) -> Vec<Colour> {
    let Ok(image) = image::load_from_memory(bytes) else {
        return Vec::new();
    };
    let small = image
        .resize_exact(32, 32, image::imageops::FilterType::Triangle)
        .to_rgb8();
    let mut bins: HashMap<Colour, usize> = HashMap::new();
    for pixel in small.pixels() {
        *bins.entry(Colour::nearest(pixel.0)).or_default() += 1;
    }
    let most = bins.values().copied().max().unwrap_or(0);
    let mut colours: Vec<(Colour, usize)> = bins
        .into_iter()
        .filter(|(_, count)| *count * 5 >= most * 4)
        .collect();
    colours.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    colours.into_iter().map(|(colour, _)| colour).collect()
}

#[cfg(test)]
mod tests;
