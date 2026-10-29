//! The one gate every name and picture passes on its way to the screen.
//!
//! The app keeps its real data; what is drawn asks the gate first. Off, the
//! gate hands back what it was given, so the app is exactly as it was.
//! On, it hands back a stand-in (Vacuum's level) or a blank (the game's
//! hardest level). It fails closed: a name it cannot derive is blanked,
//! never shown.
//!
//! The gate's state lives on the thread that draws, set by the app whenever
//! Vacuum or a game changes it, so the hundreds of places that draw a name
//! need no handle to pass around.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use super::collections::{Verdict, is_spotify_playlist};
use super::derive::{Deriver, Kind, Picture};

/// How much of what a song is the screen may tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Level {
    /// Stand-ins and generated or museum pictures: Vacuum.
    Disguised,
    /// "???" and black squares.
    Blanked,
}

/// Where pictures come from while disguised.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Pictures {
    /// Public-domain artworks from three museums, the generated picture
    /// until one arrives.
    #[default]
    Museums,
    /// Shapes and rings drawn from the name, offline.
    Generated,
    /// A list of picture addresses of your own.
    Custom,
}

/// What Vacuum shows and hides, from its settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub hide_artist_descriptions: bool,
    pub hide_playlist_descriptions: bool,
    /// Spotify's own theme playlists keep their real titles.
    pub show_theme_titles: bool,
    pub replace_own_profile: bool,
    pub replace_own_playlists: bool,
    /// Pages about a person or popularity show a notice instead.
    pub hide_pages: bool,
    pub pictures: Pictures,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            hide_artist_descriptions: true,
            hide_playlist_descriptions: true,
            show_theme_titles: false,
            replace_own_profile: false,
            replace_own_playlists: false,
            hide_pages: true,
            pictures: Pictures::Museums,
        }
    }
}

/// Who "you" are, so your own things can stay as they are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Own {
    pub user_id: Option<String>,
    pub user_name: Option<String>,
    /// Your own playlists, by ID, never Spotify's.
    pub playlists: HashSet<String>,
}

/// What a blank looks like.
pub const BLANK: &str = "???";

/// The gate while it is on.
pub struct Gate {
    pub level: Level,
    pub deriver: Arc<Deriver>,
    pub seed_id: String,
    pub options: Options,
    /// Who you are; brought up to date in place as your library loads.
    own: RefCell<Own>,
    /// Verdicts on playlists, by ID: hidden unless a theme. Brought up to
    /// date in place as playlists are judged.
    verdicts: RefCell<HashMap<String, Verdict>>,
    /// Stand-in (lower case) to the real name, so searching for a stand-in
    /// finds the real thing.
    reverse: RefCell<HashMap<String, String>>,
    /// Real name (normalized) to its stand-in, to swap names inside text.
    known: RefCell<HashMap<String, String>>,
    /// Names learnt since the swapper was built, swapped one by one until
    /// it is built again.
    fresh: RefCell<Vec<(String, String)>>,
    swapper: RefCell<Option<Swapper>>,
}

/// Finds every name in `known` inside a text at once.
struct Swapper {
    /// How many names it was built from, and when.
    names: usize,
    built: std::time::Instant,
    matcher: aho_corasick::AhoCorasick,
    stand_ins: Vec<String>,
}

/// Whether `c` is a letter or a digit, as JavaScript's `\p{L}\p{N}` is.
fn word_char(c: char) -> bool {
    static WORD: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    WORD.get_or_init(|| regex::Regex::new(r"^[\p{L}\p{N}]$").expect("a valid class"))
        .is_match(c.encode_utf8(&mut [0; 4]))
}

/// `text` with every whole-phrase `name` (lower case), ignoring case,
/// replaced by `stand_in`.
fn swap_phrase(text: &str, name: &str, stand_in: &str) -> String {
    let mut lower = String::with_capacity(text.len());
    let mut origin: Vec<usize> = Vec::with_capacity(text.len() + 1);
    for (at, c) in text.char_indices() {
        let before = lower.len();
        lower.extend(c.to_lowercase());
        origin.extend(std::iter::repeat_n(at, lower.len() - before));
    }
    origin.push(text.len());
    if !lower.contains(name) {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut from = 0;
    while let Some(found) = lower[from..].find(name).map(|at| at + from) {
        let end = found + name.len();
        from = found + 1;
        while from < lower.len() && !lower.is_char_boundary(from) {
            from += 1;
        }
        let (start, stop) = (origin[found], origin[end]);
        if start < copied
            || (found > 0 && origin[found - 1] == start)
            || (end < lower.len() && origin[end] == origin[end - 1])
        {
            continue;
        }
        let before = text[..start].chars().next_back();
        let after = text[stop..].chars().next();
        if before.is_some_and(word_char) || after.is_some_and(word_char) {
            continue;
        }
        out.push_str(&text[copied..start]);
        out.push_str(stand_in);
        copied = stop;
        from = end;
    }
    out.push_str(&text[copied..]);
    out
}

impl Gate {
    pub fn new(
        level: Level,
        deriver: Arc<Deriver>,
        seed_id: String,
        options: Options,
        own: Own,
    ) -> Self {
        Self {
            level,
            deriver,
            seed_id,
            options,
            own: RefCell::new(own),
            verdicts: RefCell::default(),
            reverse: RefCell::default(),
            known: RefCell::default(),
            fresh: RefCell::default(),
            swapper: RefCell::default(),
        }
    }

    /// Learnt stand-ins carried over from a gate with the same seed.
    pub fn inherit(&self, other: &Gate) {
        if other.seed_id == self.seed_id {
            self.reverse
                .borrow_mut()
                .extend(other.reverse.borrow().clone());
            self.known.borrow_mut().extend(other.known.borrow().clone());
            *self.verdicts.borrow_mut() = other.verdicts.borrow().clone();
        }
    }

    /// Who you are, when your library or account changes.
    pub fn set_own(&self, own: Own) {
        *self.own.borrow_mut() = own;
    }

    /// A playlist's verdict, as it is reached.
    pub fn set_verdict(&self, id: &str, verdict: Verdict) {
        self.verdicts.borrow_mut().insert(id.to_string(), verdict);
    }

    pub fn verdict(&self, id: &str) -> Option<Verdict> {
        self.verdicts.borrow().get(id).copied()
    }

    /// The stand-in for `real` as a `kind`, learnt for search and text.
    pub fn stand_in(&self, kind: Kind, real: &str) -> String {
        if self.level == Level::Blanked {
            return BLANK.into();
        }
        if real.trim().is_empty() {
            return String::new();
        }
        let Some(derived) = self.deriver.derive(kind, real) else {
            // Overlong: nothing to derive from, so nothing is shown.
            return BLANK.into();
        };
        self.reverse
            .borrow_mut()
            .entry(derived.name.to_lowercase())
            .or_insert_with(|| real.trim().to_string());
        if let std::collections::hash_map::Entry::Vacant(entry) = self
            .known
            .borrow_mut()
            .entry(super::derive::normalize_name(real))
        {
            self.fresh
                .borrow_mut()
                .push((entry.key().clone(), derived.name.clone()));
            entry.insert(derived.name.clone());
        }
        derived.name
    }

    /// The stand-in a name has had, if the gate has shown it.
    pub fn known_stand_in(&self, real: &str) -> Option<String> {
        self.known
            .borrow()
            .get(&super::derive::normalize_name(real))
            .cloned()
    }

    /// A stand-in for `real` that search can map back, but that is not
    /// swapped into other text: a message or a past search is not a name
    /// to look for in lyrics.
    pub fn stand_in_alone(&self, kind: Kind, real: &str) -> String {
        if self.level == Level::Blanked {
            return BLANK.into();
        }
        let Some(derived) = self.deriver.derive(kind, real) else {
            return BLANK.into();
        };
        self.reverse
            .borrow_mut()
            .entry(derived.name.to_lowercase())
            .or_insert_with(|| real.trim().to_string());
        derived.name
    }

    /// The real name behind a stand-in this gate has shown, if any.
    pub fn real_for(&self, stand_in: &str) -> Option<String> {
        self.reverse
            .borrow()
            .get(&stand_in.trim().to_lowercase())
            .cloned()
    }

    /// Whether `text` is a stand-in this gate has shown.
    pub fn is_stand_in(&self, text: &str) -> bool {
        self.reverse
            .borrow()
            .contains_key(&text.trim().to_lowercase())
    }

    /// Every stand-in learnt, with its real name, to keep between runs.
    pub fn learnt(&self) -> Vec<(String, String)> {
        self.reverse
            .borrow()
            .iter()
            .map(|(stand_in, real)| (stand_in.clone(), real.clone()))
            .collect()
    }

    /// Stand-ins learnt in an earlier run.
    pub fn learn(&self, pairs: impl IntoIterator<Item = (String, String)>) {
        let mut reverse = self.reverse.borrow_mut();
        for (stand_in, real) in pairs {
            reverse.entry(stand_in.to_lowercase()).or_insert(real);
        }
    }

    /// `text` with every name the gate has shown swapped for its stand-in,
    /// whole phrases only, the longest first. Names it has never seen stay
    /// as they are: the cost of showing free text at all, which is why
    /// descriptions are hidden by default. Should the matcher fail to
    /// build, nothing is shown.
    pub fn swap_known(&self, text: &str) -> String {
        if self.level == Level::Blanked {
            return BLANK.into();
        }
        let known = self.known.borrow();
        if known.is_empty() || text.is_empty() {
            return text.to_string();
        }
        let mut swapper = self.swapper.borrow_mut();
        // Built again as names arrive, at most once a second: a name seen
        // in the last moment has its stand-in drawn already.
        let fresh_count = self.fresh.borrow().len();
        let stale = swapper.as_ref().is_none_or(|swapper| {
            swapper.names != known.len()
                && (swapper.built.elapsed() >= std::time::Duration::from_secs(1)
                    || fresh_count > 256)
        });
        if stale {
            let (names, stand_ins): (Vec<&str>, Vec<String>) = known
                .iter()
                .filter(|(name, _)| name.chars().count() >= 2)
                .map(|(name, stand_in)| (name.as_str(), stand_in.clone()))
                .unzip();
            match aho_corasick::AhoCorasick::builder()
                .match_kind(aho_corasick::MatchKind::LeftmostLongest)
                .build(&names)
            {
                Ok(matcher) => {
                    self.fresh.borrow_mut().clear();
                    *swapper = Some(Swapper {
                        names: known.len(),
                        built: std::time::Instant::now(),
                        matcher,
                        stand_ins,
                    });
                }
                Err(error) => {
                    log::warn!("Vacuum could not build its name swapper: {error}");
                    return BLANK.into();
                }
            }
        }
        let Some(swapper) = swapper.as_ref() else {
            return BLANK.into();
        };
        // Matched in lower case, mapped back to the text as written.
        let mut lower = String::with_capacity(text.len());
        let mut origin: Vec<usize> = Vec::with_capacity(text.len() + 1);
        let mut starts: Vec<bool> = Vec::with_capacity(text.len() + 1);
        for (at, c) in text.char_indices() {
            let before = lower.len();
            lower.extend(c.to_lowercase());
            for byte in before..lower.len() {
                origin.push(at);
                starts.push(byte == before);
            }
        }
        origin.push(text.len());
        starts.push(true);
        let mut out = String::with_capacity(text.len());
        let mut copied = 0;
        for found in swapper.matcher.find_iter(&lower) {
            if !starts[found.start()] || !starts[found.end()] {
                continue;
            }
            let (from, to) = (origin[found.start()], origin[found.end()]);
            if from < copied {
                continue;
            }
            let before = text[..from].chars().next_back();
            let after = text[to..].chars().next();
            if before.is_some_and(word_char) || after.is_some_and(word_char) {
                continue;
            }
            out.push_str(&text[copied..from]);
            out.push_str(&swapper.stand_ins[found.pattern().as_usize()]);
            copied = to;
        }
        out.push_str(&text[copied..]);
        // Names learnt since the matcher was built, one at a time.
        for (name, stand_in) in self.fresh.borrow().iter() {
            if name.chars().count() >= 2 {
                out = swap_phrase(&out, name, stand_in);
            }
        }
        out
    }

    fn own_user(&self, id: Option<&str>, name: &str) -> bool {
        let own = self.own.borrow();
        match (id, &own.user_id) {
            (Some(id), Some(mine)) => id == mine,
            // By name only until the account's id is known.
            (None, _) | (_, None) => own.user_name.as_deref().is_some_and(|mine| mine == name),
        }
    }

    fn is_spotify(id: Option<&str>, name: &str) -> bool {
        id.is_some_and(|id| id.eq_ignore_ascii_case("spotify"))
            || name.eq_ignore_ascii_case("spotify")
    }

    /// Whether a playlist is yours: owned by your account and not one of
    /// Spotify's own, which list you as their owner but never are yours.
    pub fn own_playlist(&self, id: &str, owner_id: Option<&str>) -> bool {
        if is_spotify_playlist(id) {
            return false;
        }
        let own = self.own.borrow();
        own.playlists.contains(id)
            || owner_id.is_some_and(|owner| own.user_id.as_deref() == Some(owner))
    }

    /// Whether a playlist may be shown: yours always; others unless judged
    /// to be about a person or popularity (or unsure). One not judged yet
    /// shows: every playlist the app holds is judged before it is drawn.
    pub fn playlist_shown(&self, id: &str, owner_id: Option<&str>) -> bool {
        self.own_playlist(id, owner_id) || self.verdict(id).is_none_or(|verdict| verdict.shown())
    }
}

/// Whether a picture's address is ready to draw, starting it if not.
type Probe = Box<dyn Fn(&str) -> bool>;

thread_local! {
    static GATE: RefCell<Option<Rc<Gate>>> = const { RefCell::new(None) };
    static PROBE: RefCell<Option<Probe>> = const { RefCell::new(None) };
}

/// How the gate learns whether a museum picture is ready to draw.
pub fn set_probe(probe: Option<Probe>) {
    PROBE.with(|current| *current.borrow_mut() = probe);
}

fn ready(uri: &str) -> bool {
    PROBE.with(|probe| probe.borrow().as_ref().is_none_or(|probe| probe(uri)))
}

/// Puts `gate` in charge of this thread's drawing, or nothing: everything
/// real again, at once.
pub fn set(gate: Option<Gate>) {
    GATE.with(|current| {
        let gate = gate.map(|gate| {
            if let Some(old) = current.borrow().as_ref() {
                gate.inherit(old);
            }
            Rc::new(gate)
        });
        *current.borrow_mut() = gate;
    });
}

/// The gate in charge, if any.
pub fn current() -> Option<Rc<Gate>> {
    GATE.with(|current| current.borrow().clone())
}

/// Whether anything is being hidden.
pub fn on() -> bool {
    GATE.with(|current| current.borrow().is_some())
}

/// Whether the gate is on at `level`.
pub fn at(level: Level) -> bool {
    current().is_some_and(|gate| gate.level == level)
}

fn shown<'a>(kind: Kind, real: &'a str) -> Cow<'a, str> {
    match current() {
        Some(gate) => Cow::Owned(gate.stand_in(kind, real)),
        None => Cow::Borrowed(real),
    }
}

/// A song's title. Songs, podcasts, episodes and audiobooks are named as
/// albums, so a single and its only song share one stand-in.
pub fn song(title: &str) -> Cow<'_, str> {
    shown(Kind::Album, title)
}

pub fn album(name: &str) -> Cow<'_, str> {
    shown(Kind::Album, name)
}

pub fn artist(name: &str) -> Cow<'_, str> {
    shown(Kind::Artist, name)
}

/// A podcast, episode or audiobook: named as albums, as songs are.
pub fn show(name: &str) -> Cow<'_, str> {
    shown(Kind::Album, name)
}

/// Artists joined as the app lists them.
pub fn artists<'a>(names: impl IntoIterator<Item = &'a str>, separator: &str) -> String {
    names
        .into_iter()
        .map(|name| artist(name).into_owned())
        .collect::<Vec<_>>()
        .join(separator)
}

/// A person: a playlist's owner, a profile, a friend. You, and Spotify,
/// stay as you are unless asked.
pub fn user<'a>(id: Option<&str>, name: &'a str) -> Cow<'a, str> {
    let Some(gate) = current() else {
        return Cow::Borrowed(name);
    };
    if gate.level == Level::Disguised
        && (Gate::is_spotify(id, name)
            || (gate.own_user(id, name) && !gate.options.replace_own_profile))
    {
        return Cow::Borrowed(name);
    }
    Cow::Owned(gate.stand_in(Kind::User, name))
}

/// A playlist's title. Your own keep theirs unless asked; so do Spotify's
/// theme playlists with Show theme titles.
pub fn playlist<'a>(id: &str, owner_id: Option<&str>, title: &'a str) -> Cow<'a, str> {
    let Some(gate) = current() else {
        return Cow::Borrowed(title);
    };
    if gate.level == Level::Disguised {
        let own = gate.own_playlist(id, owner_id) && !gate.options.replace_own_playlists;
        let theme = gate.options.show_theme_titles
            && is_spotify_playlist(id)
            && gate.verdict(id) == Some(Verdict::Theme);
        if own || theme {
            return Cow::Borrowed(title);
        }
    }
    Cow::Owned(gate.stand_in(Kind::Playlist, title))
}

/// An artist's biography: "???" by default, else with known names swapped.
pub fn artist_description(text: &str) -> Cow<'_, str> {
    description(text, |options| options.hide_artist_descriptions)
}

/// A playlist's description, likewise. Your own playlists' stay unless
/// asked.
pub fn playlist_description<'a>(id: &str, owner_id: Option<&str>, text: &'a str) -> Cow<'a, str> {
    if let Some(gate) = current()
        && gate.level == Level::Disguised
        && gate.own_playlist(id, owner_id)
        && !gate.options.replace_own_playlists
    {
        return Cow::Borrowed(text);
    }
    description(text, |options| options.hide_playlist_descriptions)
}

fn description<'a>(text: &'a str, hidden: impl Fn(&Options) -> bool) -> Cow<'a, str> {
    let Some(gate) = current() else {
        return Cow::Borrowed(text);
    };
    if text.trim().is_empty() {
        return Cow::Borrowed(text);
    }
    if gate.level == Level::Blanked || hidden(&gate.options) {
        return Cow::Borrowed(BLANK);
    }
    Cow::Owned(gate.swap_known(text))
}

/// What a search for `query` asks Spotify for: the real names behind any
/// stand-ins in it. A whole stand-in, or the start of exactly one, is its
/// real name; stand-ins inside a longer query are swapped in place. The
/// query shown, and kept in the history, stays as typed.
pub fn real_query(query: &str) -> String {
    let Some(gate) = current() else {
        return query.to_string();
    };
    if let Some(real) = gate.real_for(query) {
        return real;
    }
    let lower = query.trim().to_lowercase();
    let reverse = gate.reverse.borrow();
    if lower.chars().count() >= 3 {
        let mut starts = reverse
            .iter()
            .filter(|(stand_in, _)| stand_in.starts_with(&lower));
        if let (Some((_, real)), None) = (starts.next(), starts.next()) {
            return real.clone();
        }
    }
    // Stand-ins inside a longer query, the longest first, whole phrases.
    let mut inside: Vec<(&String, &String)> = reverse
        .iter()
        .filter(|(stand_in, _)| query.to_lowercase().contains(stand_in.as_str()))
        .collect();
    inside.sort_by_key(|(stand_in, _)| std::cmp::Reverse(stand_in.len()));
    let mut out = query.to_string();
    for (stand_in, real) in inside {
        let lowered = out.to_lowercase();
        // Stand-ins are plain words, so lower case keeps their places.
        if lowered.len() != out.len() {
            continue;
        }
        let mut from = 0;
        while let Some(at) = lowered[from..].find(stand_in.as_str()).map(|at| at + from) {
            let end = at + stand_in.len();
            let before = out[..at].chars().next_back();
            let after = out[end..].chars().next();
            if !before.is_some_and(word_char) && !after.is_some_and(word_char) {
                out.replace_range(at..end, real);
                break;
            }
            from = end;
        }
    }
    out
}

/// A name inside a message, kind unknown: a song, album or playlist named
/// in a toast, a search kept in the history. Its stand-in where Vacuum
/// knows the name or names inside it; a stand-in stays as it is; anything
/// else gets a stand-in of its own, so nothing real is shown.
pub fn name(text: &str) -> Cow<'_, str> {
    let Some(gate) = current() else {
        return Cow::Borrowed(text);
    };
    if gate.level == Level::Blanked {
        return Cow::Borrowed(BLANK);
    }
    if text.trim().is_empty() || gate.is_stand_in(text) {
        return Cow::Borrowed(text);
    }
    if let Some(stand_in) = gate.known_stand_in(text) {
        return Cow::Owned(stand_in);
    }
    Cow::Owned(gate.stand_in_alone(Kind::Album, text))
}

/// A podcast's or an episode's description, hidden as a playlist's is.
pub fn show_description(text: &str) -> Cow<'_, str> {
    description(text, |options| options.hide_playlist_descriptions)
}

/// Free text that may hold names, such as lyrics or a toast: every name the
/// gate has shown swapped for its stand-in.
pub fn text(text: &str) -> Cow<'_, str> {
    match current() {
        Some(gate) => Cow::Owned(gate.swap_known(text)),
        None => Cow::Borrowed(text),
    }
}

/// Whether counts that measure people and popularity (followers, monthly
/// listeners, saves, plays) are hidden.
pub fn hide_counts() -> bool {
    on()
}

/// Whether a playlist may appear in lists and shelves.
pub fn playlist_listed(id: &str, owner_id: Option<&str>) -> bool {
    current().is_none_or(|gate| gate.level == Level::Blanked || gate.playlist_shown(id, owner_id))
}

/// What a picture of something shows, as an image address the app's
/// picture loader answers. `kind` is the name space its name is derived in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtKind {
    Album,
    /// A song, by its album's ID when known.
    Track,
    Artist,
    Playlist,
    User,
    /// A podcast, episode or audiobook.
    Show,
}

impl ArtKind {
    fn name_kind(self) -> Kind {
        match self {
            ArtKind::Album | ArtKind::Track | ArtKind::Show => Kind::Album,
            ArtKind::Artist => Kind::Artist,
            ArtKind::Playlist => Kind::Playlist,
            ArtKind::User => Kind::User,
        }
    }

    /// The kind museum pictures are keyed by, for those that get one.
    fn museum_kind(self) -> Option<&'static str> {
        match self {
            ArtKind::Album => Some("album"),
            ArtKind::Track => Some("track"),
            ArtKind::Artist => Some("artist"),
            ArtKind::Playlist => Some("playlist"),
            ArtKind::User | ArtKind::Show => None,
        }
    }
}

/// The scheme of a generated picture's address.
pub const GENERATED: &str = "vacuum-gen:";
/// The scheme of a museum or listed picture's address.
pub const ART: &str = "vacuum-art:";
/// A black square, for the blanked level.
pub const BLANK_ART: &str = "vacuum-blank:";

/// The address of a generated picture: whether it is round, and the 16
/// bytes it is drawn from. The real name never appears in it.
fn generated(picture_bytes: &[u8], round: bool) -> String {
    format!(
        "{GENERATED}{}{}",
        if round { 'r' } else { 'c' },
        super::derive::hex(picture_bytes)
    )
}

/// Whether a Spotify ID can key a museum picture.
fn museum_id(id: &str) -> bool {
    id.len() == 22 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

/// Spotify's own covers for a playlist made from its songs: a mosaic of
/// four album covers, or one album's cover.
fn automatic_cover(url: &str) -> bool {
    url.contains("mosaic.scdn.co") || url.contains("/image/ab67616d")
}

/// The picture to draw for something, in place of `real_url`. `name` is
/// the name shown beside it, `id` its Spotify ID (a song's album's). Off,
/// that is `real_url` itself.
pub fn art(kind: ArtKind, id: Option<&str>, name: &str, real_url: Option<&str>) -> Option<String> {
    let Some(gate) = current() else {
        return real_url.map(str::to_owned);
    };
    if gate.level == Level::Blanked {
        return Some(BLANK_ART.into());
    }
    // Your own playlists keep the covers you gave them; covers Spotify made
    // from the songs in them are still replaced.
    if kind == ArtKind::Playlist
        && let Some(url) = real_url
        && id.is_some_and(|id| gate.own_playlist(id, None))
        && !gate.options.replace_own_playlists
        && !automatic_cover(url)
    {
        return Some(url.to_owned());
    }
    // Your own picture stays unless asked.
    if kind == ArtKind::User && gate.own_user(id, name) && !gate.options.replace_own_profile {
        return real_url.map(str::to_owned);
    }
    let name_kind = kind.name_kind();
    // Drawn from the name; failing that, from the ID; failing that, from
    // the address of the real picture. Never an empty hole.
    let label = if !name.trim().is_empty() {
        None
    } else if let Some(id) = id.filter(|id| !id.is_empty()) {
        Some(format!("\u{1}{}:{id}", name_kind.label()))
    } else {
        real_url.map(str::to_owned)
    };
    let derived = gate
        .deriver
        .derive(name_kind, label.as_deref().unwrap_or(name))
        .or_else(|| gate.deriver.derive(name_kind, "\u{1}"))?;
    let picture = generated(&derived.digest[16..], name_kind.round());
    if gate.options.pictures == Pictures::Generated {
        return Some(picture);
    }
    match (kind.museum_kind(), id.filter(|id| museum_id(id))) {
        (Some(museum_kind), Some(id)) => {
            let art = format!(
                "{ART}{}/{museum_kind}/{id}/{}",
                gate.seed_id,
                &picture[GENERATED.len()..]
            );
            Some(if ready(&art) { art } else { picture })
        }
        _ => Some(picture),
    }
}

/// The generated picture behind a `vacuum-gen:` or `vacuum-art:` address,
/// and whether it is round.
pub fn generated_picture(uri: &str) -> Option<(Picture, bool)> {
    let code = if let Some(code) = uri.strip_prefix(GENERATED) {
        code
    } else {
        uri.strip_prefix(ART)?.rsplit('/').next()?
    };
    let (round, hex) = (code.starts_with('r'), code.get(1..)?);
    if hex.len() != 32 {
        return None;
    }
    let bytes: Vec<u8> = (0..16)
        .map(|index| u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16))
        .collect::<Result<_, _>>()
        .ok()?;
    Some((super::derive::picture_from(&bytes, round), round))
}

/// Whether the app's picture loader should answer `uri` as Vacuum's.
pub fn is_vacuum_art(uri: &str) -> bool {
    uri.starts_with(GENERATED) || uri.starts_with(ART) || uri.starts_with(BLANK_ART)
}
