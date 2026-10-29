//! The guessing game in the app: starting a game from a source, following
//! the songs it plays, the deadline, revealing, and keeping the record.
//! See [`crate::game`] for the rules.

use super::*;
use crate::game::{Category, Colour, LIBRARY, Mode, Record, RoundState, Session};

/// Songs a game plays at most: a long playlist is sampled, due songs first.
const GAME_SONGS: usize = 200;

/// A round that begins further into its song than this was joined late.
const JOINED_LATE_MS: u32 = 5_000;

/// How long a song joined late is still guessed.
const LATE_GRACE_MS: u32 = 30_000;

/// How often the cache is looked in for the cover whose colours are read.
const COLOUR_CHECK: std::time::Duration = std::time::Duration::from_millis(250);

/// When the guessing stops for a round that began at `started_ms`: the
/// first lyric, or a fifth of the song; a song joined late, midway, still
/// gets half a minute.
fn round_deadline(first_line_ms: Option<u32>, duration_ms: u32, started_ms: u32) -> u32 {
    let deadline = crate::game::deadline_ms(first_line_ms, duration_ms);
    if started_ms > JOINED_LATE_MS {
        deadline.max(started_ms.saturating_add(LATE_GRACE_MS))
    } else {
        deadline
    }
}

impl App {
    /// Where the game's record is kept, beside the play history.
    pub(crate) fn game_file(&self) -> std::path::PathBuf {
        self.dirs.state.join("game.json")
    }

    /// The record kept in `path`, and, when the file is there but cannot be
    /// read, its text, so it is set aside rather than written over.
    pub(crate) fn load_game_record(path: &std::path::Path) -> (Record, Option<String>) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return (Record::default(), None);
        };
        match serde_json::from_str::<Record>(&text) {
            Ok(mut record) => {
                record.setup = record.setup.valid();
                (record, None)
            }
            Err(error) => {
                log::warn!("game.json could not be read, so it is set aside: {error}");
                (Record::default(), Some(text))
            }
        }
    }

    pub(crate) fn save_game_record(&mut self) {
        if let Some(text) = self.game_unreadable.take() {
            self.files.write(
                &self.dirs.state.join("game.json.unreadable"),
                text,
                fastframe_file::Access::Private,
            );
        }
        if let Ok(text) = serde_json::to_string(&self.game_record) {
            self.files
                .write(&self.game_file(), text, fastframe_file::Access::Private);
        }
    }

    /// The songs a source holds, once loaded: `None` while they are not.
    pub(crate) fn game_songs(&self, source: &str) -> Option<Vec<Track>> {
        let tracks = |page: &PlaylistPage| {
            page.items
                .items
                .iter()
                .filter_map(|item| match item.playable() {
                    Some(PlayableItem::Track(track)) => Some(track.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let playlist = |id: &str| {
            self.playlist_pages
                .get(id)
                .filter(|page| page.items.is_complete())
                .map(tracks)
        };
        let liked = || {
            self.library.liked.is_complete().then(|| {
                self.library
                    .liked
                    .items
                    .iter()
                    .map(|saved| saved.track.clone())
                    .collect::<Vec<_>>()
            })
        };
        let songs = if source == LIBRARY {
            // Liked Songs and the playlists in your library whose songs are
            // all here; not a playlist only looked at.
            let mut songs = liked()?;
            for playlist in self.library.playlists.get().into_iter().flatten() {
                if let Some(page) = self
                    .playlist_pages
                    .get(&playlist.id)
                    .filter(|page| page.items.is_complete())
                {
                    songs.extend(tracks(page));
                }
            }
            songs
        } else if source.ends_with(":collection") {
            liked()?
        } else if let Some(id) = source.strip_prefix("spotify:playlist:") {
            playlist(id)?
        } else {
            let id = source.strip_prefix("spotify:album:")?;
            let page = self
                .album_pages
                .get(id)
                .filter(|page| page.tracks.is_complete())?;
            let album = page.album.get();
            page.tracks
                .items
                .iter()
                .cloned()
                .map(|mut track| {
                    if track.album.is_none() {
                        track.album = album.cloned();
                    }
                    track
                })
                .collect()
        };
        let mut seen = HashSet::new();
        Some(
            songs
                .into_iter()
                .filter(|track| {
                    !track.is_local && !track.uri.is_empty() && seen.insert(track.uri.clone())
                })
                .collect(),
        )
    }

    /// Asks for a source's songs, if they are not here yet.
    pub(crate) fn load_game_songs(&mut self, source: &str) {
        if source == LIBRARY || source.ends_with(":collection") {
            self.ensure_liked_songs();
        } else if let Some(id) = source.strip_prefix("spotify:playlist:") {
            self.ensure_loaded(Page::Playlist(id.to_string()));
        } else if let Some(id) = source.strip_prefix("spotify:album:") {
            self.ensure_loaded(Page::Album(id.to_string()));
        }
    }

    /// Starts a game from `source`: its songs shuffled, those due to come
    /// back first, played in that order.
    pub(crate) fn start_game(&mut self, source: String) {
        let Some(songs) = self.game_songs(&source) else {
            self.load_game_songs(&source);
            return;
        };
        if songs.is_empty() {
            self.toast(gettext(self.locale, "Nothing to play in this game"));
            return;
        }
        // A game started over another one ends that one first, keeping
        // its last round (Play again after an Honor round's ticks).
        self.finish_round();
        let shuffle_before = match self.game.take() {
            Some(session) => session.shuffle_before,
            None => self.shuffle_wanted,
        };
        let now = jiff::Timestamp::now().as_second();
        let (mut due, mut rest): (Vec<Track>, Vec<Track>) = songs
            .into_iter()
            .partition(|track| self.game_record.due(&track.uri, now));
        use rand::seq::SliceRandom;
        let mut random = rand::rng();
        due.shuffle(&mut random);
        rest.shuffle(&mut random);
        due.extend(rest);
        due.truncate(GAME_SONGS);
        let uris: Vec<String> = due.iter().map(|track| track.uri.clone()).collect();
        self.game = Some(Session {
            setup: self.game_record.setup,
            source,
            songs: due,
            shuffle_before,
            ..Default::default()
        });
        // The game's own order, not the player's shuffle, which comes back
        // when the game ends.
        self.actions.push(Action::SetShuffle(false));
        self.actions.push(Action::PlayUris { uris, index: 0 });
        self.open(Page::Game);
    }

    /// Plays the songs of the game not played yet, after another song
    /// interrupted it.
    pub(crate) fn continue_game(&mut self) {
        let Some(session) = &self.game else {
            return;
        };
        let uris: Vec<String> = session
            .songs
            .iter()
            .filter(|track| !session.done.contains(&track.uri))
            .map(|track| track.uri.clone())
            .collect();
        if !uris.is_empty() {
            self.actions.push(Action::SetShuffle(false));
            self.actions.push(Action::PlayUris { uris, index: 0 });
        }
    }

    /// How much of the playing song to hide: the game's level while a
    /// round is being guessed, and while a song of the game that has not
    /// been guessed yet plays before its round begins (after Next, or at
    /// the start), so not a frame of it shows; otherwise nothing of the
    /// game's.
    pub(crate) fn game_hiding(&self) -> Option<crate::vacuum::gate::Level> {
        let session = self.game.as_ref()?;
        let level = match session.setup.hiding {
            crate::game::Hiding::Real => return None,
            crate::game::Hiding::Disguised => crate::vacuum::gate::Level::Disguised,
            crate::game::Hiding::Blanked => crate::vacuum::gate::Level::Blanked,
        };
        let guessing = session.round.as_ref().is_some_and(|round| !round.revealed);
        let coming = || {
            self.now_playing().is_some_and(|now| {
                session
                    .round
                    .as_ref()
                    .is_none_or(|round| round.track.uri != now.uri)
                    && session.pending(&now.uri).is_some()
            })
        };
        (guessing || coming()).then_some(level)
    }

    /// Whether the queue is kept out of sight: while a game hides its
    /// songs, the queue would name the next ones.
    pub(crate) fn game_hides_queue(&self) -> bool {
        self.game
            .as_ref()
            .is_some_and(|session| session.setup.hiding != crate::game::Hiding::Real)
    }

    /// Follows the game each frame: a round for each of its songs as it
    /// starts, the deadline from the lyrics once they are in, and the
    /// reveal when the deadline passes. A song outside the game, or one
    /// already guessed, ends the round without starting another.
    pub(crate) fn sync_game(&mut self, ctx: &egui::Context) {
        let Some(now) = self.now_playing() else {
            return;
        };
        let Some(session) = &self.game else {
            return;
        };
        let current = session.round.as_ref().map(|round| round.track.uri.clone());
        let (mode, grid) = (session.setup.mode, usize::from(session.setup.grid));
        if current.as_deref() != Some(now.uri.as_str()) {
            let Some(track) = session.pending(&now.uri).cloned() else {
                // Not the game's, or guessed already (Previous): nothing is
                // guessed, and nothing hidden, until a new song of the game.
                if current.is_some() {
                    self.finish_round();
                    if let Some(session) = &mut self.game {
                        session.round = None;
                    }
                }
                return;
            };
            // A song of the game began, by its own order or a skip.
            self.finish_round();
            self.request_lyrics();
            let duration = [now.duration_ms, track.duration_ms]
                .into_iter()
                .find(|duration| *duration > 0)
                .unwrap_or(150_000);
            let mut round = RoundState::new(
                track,
                round_deadline(None, duration, now.position_ms),
                false,
            );
            round.started_ms = now.position_ms;
            round.duration_ms = duration;
            if mode == Mode::CoverGrid {
                round.grid = self.game_grid(&round.track, grid);
            }
            if let Some(session) = &mut self.game {
                session.done.insert(round.track.uri.clone());
                session.round = Some(round);
            }
        }
        // The first synced lyric, once the lyrics arrive.
        let first_line = match (&self.lyrics, &self.lyrics_uri) {
            (Loadable::Loaded(Some(lyrics)), Some(uri)) if *uri == now.uri && lyrics.synced => {
                lyrics
                    .lines
                    .iter()
                    .find(|line| !line.text.trim().is_empty())
                    .and_then(|line| line.at_ms)
            }
            _ => None,
        };
        let art = now.art_url.clone().or(now.art_small.clone());
        let Some(session) = &mut self.game else {
            return;
        };
        let mode = session.setup.mode;
        let Some(round) = &mut session.round else {
            return;
        };
        if !round.timed_by_lyrics
            && let Some(at) = first_line
        {
            round.deadline_ms = round_deadline(Some(at), round.duration_ms, round.started_ms);
            round.timed_by_lyrics = true;
        }
        // The main colour is read from the real cover, off this thread,
        // once it is in the cache; the cache is looked in now and then.
        if mode == Mode::MainColour
            && !round.colours_asked
            && round
                .colours_checked
                .is_none_or(|at| at.elapsed() >= COLOUR_CHECK)
        {
            round.colours_checked = Some(Instant::now());
            let real = round
                .track
                .album
                .as_ref()
                .and_then(|album| pick_image(&album.images, 300))
                .map(str::to_owned)
                .or(art);
            if let Some(url) = real {
                self.backend.art().prefetch(ctx, &url);
                if let Some(path) = self.backend.art().cached_file(&url) {
                    round.colours_asked = true;
                    let colours = round.colours.clone();
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let read = std::fs::read(path)
                            .map(|bytes| crate::game::cover_colours(&bytes))
                            .unwrap_or_default();
                        *colours
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(read);
                        ctx.request_repaint();
                    });
                }
            }
        }
        if !round.revealed && now.position_ms >= round.deadline_ms {
            self.reveal_round();
        } else if !round.revealed {
            // Count the time down.
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    /// The covers a grid offers: this song's album's, and others from the
    /// game's songs, then from your library when the game has too few, in a
    /// random order. Only albums with a cover are offered, so no cell
    /// stands out.
    fn game_grid(&self, track: &Track, size: usize) -> Vec<(String, Option<String>)> {
        use rand::seq::SliceRandom;
        let Some(session) = &self.game else {
            return Vec::new();
        };
        let cover = |album: &crate::api::models::Album| {
            pick_image(&album.images, 300).map(|image| (album.uri.clone(), Some(image.to_owned())))
        };
        let Some(own) = track.album.as_ref().and_then(cover) else {
            return Vec::new();
        };
        // Two albums can share a cover (a single and its album): each
        // cover is offered once, so only one cell can be right.
        let mut seen = HashSet::from([own.0.clone()]);
        let mut seen_images: HashSet<String> = own.1.iter().cloned().collect();
        let mut gather = |songs: &mut dyn Iterator<Item = &Track>| {
            let mut found: Vec<(String, Option<String>)> = Vec::new();
            for song in songs {
                if let Some(album) = &song.album
                    && !seen.contains(&album.uri)
                    && let Some(cell) = cover(album)
                    && cell
                        .1
                        .as_ref()
                        .is_none_or(|image| !seen_images.contains(image))
                {
                    seen.insert(album.uri.clone());
                    seen_images.extend(cell.1.clone());
                    found.push(cell);
                }
            }
            found
        };
        let wanted = size.saturating_sub(1);
        let mut random = rand::rng();
        let mut others = gather(&mut session.songs.iter());
        others.shuffle(&mut random);
        if others.len() < wanted {
            let mut more = gather(
                &mut self
                    .library
                    .liked
                    .items
                    .iter()
                    .map(|saved| &saved.track)
                    .chain(self.playlist_pages.values().flat_map(|page| {
                        page.items
                            .items
                            .iter()
                            .filter_map(|item| match item.playable() {
                                Some(PlayableItem::Track(track)) => Some(track),
                                _ => None,
                            })
                    })),
            );
            more.shuffle(&mut random);
            others.extend(more);
        }
        others.truncate(wanted);
        others.push(own);
        others.shuffle(&mut random);
        others
    }

    /// Ends the guessing for the round: judged, and kept, except in Honor
    /// mode, where what was got is ticked after the reveal. A round that
    /// cannot be scored (no cover read, too few covers) is not kept.
    pub(crate) fn reveal_round(&mut self) {
        let position = self.now_playing().map(|now| now.position_ms);
        let Some(session) = &mut self.game else {
            return;
        };
        let mode = session.setup.mode;
        let Some(round) = &mut session.round else {
            return;
        };
        if round.revealed {
            return;
        }
        round.revealed = true;
        if round.answer_ms.is_none() {
            round.answer_ms = position.map(|position| position.min(round.deadline_ms));
        }
        if mode != Mode::Honor {
            if round.scoreable(mode) {
                round.judge(mode);
                self.keep_round();
            } else {
                round.recorded = true;
                round.unscored = true;
            }
        }
    }

    /// Keeps the round's result in the record, once.
    fn keep_round(&mut self) {
        let Some(session) = &mut self.game else {
            return;
        };
        let mode = session.setup.mode;
        let Some(round) = &mut session.round else {
            return;
        };
        if round.recorded {
            return;
        }
        round.recorded = true;
        session.played += 1;
        session.points += round.points;
        let kept = crate::game::Round {
            at: jiff::Timestamp::now().as_second(),
            uri: round.track.uri.clone(),
            mode,
            got: round.got.clone(),
            answer_ms: round.answer_ms,
            points: round.points,
        };
        self.game_record.keep(kept);
        self.save_game_record();
    }

    /// The round as it stands, kept before the next song: Honor's ticks
    /// are judged here. A round skipped or ended before its reveal had
    /// nothing answered, and is not kept.
    fn finish_round(&mut self) {
        let Some(session) = &mut self.game else {
            return;
        };
        let mode = session.setup.mode;
        let Some(round) = &mut session.round else {
            return;
        };
        if !round.revealed {
            round.revealed = true;
            round.recorded = true;
            round.unscored = true;
            return;
        }
        if !round.recorded {
            round.judge(mode);
            self.keep_round();
        }
    }

    /// Moves on to the next song of the game.
    pub(crate) fn next_game_song(&mut self) {
        self.finish_round();
        self.actions.push(Action::Next);
    }

    /// Ends the game, giving the player's shuffle back.
    pub(crate) fn end_game(&mut self) {
        self.finish_round();
        if let Some(session) = self.game.take()
            && session.shuffle_before
        {
            self.actions.push(Action::SetShuffle(true));
        }
    }

    /// A tick in Honor mode, after the reveal.
    pub(crate) fn tick_round(&mut self, category: Category) {
        if let Some(round) = self
            .game
            .as_mut()
            .and_then(|session| session.round.as_mut())
            .filter(|round| round.revealed && !round.recorded)
            && !round.ticked.remove(&category)
        {
            round.ticked.insert(category);
        }
    }

    /// A cover picked from the grid: the answer, revealed.
    pub(crate) fn pick_cover(&mut self, index: usize) {
        if let Some(round) = self
            .game
            .as_mut()
            .and_then(|session| session.round.as_mut())
            .filter(|round| !round.revealed && index < round.grid.len())
        {
            round.picked = Some(index);
            self.reveal_round();
        }
    }

    /// A colour said: the answer, revealed, once the cover has been read.
    pub(crate) fn pick_colour(&mut self, colour: Colour) {
        if let Some(round) = self
            .game
            .as_mut()
            .and_then(|session| session.round.as_mut())
            .filter(|round| !round.revealed && round.scoreable(Mode::MainColour))
        {
            round.colour = Some(colour);
            self.reveal_round();
        }
    }

    /// Whether `category` was right in the round shown, for the reveal.
    pub(crate) fn round_got(round: &RoundState, category: Category) -> Option<bool> {
        round.got.get(&category).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_song_joined_late_gets_extra_time() {
        assert_eq!(round_deadline(Some(12_000), 200_000, 0), 12_000);
        assert_eq!(round_deadline(None, 120_000, 300), 24_000);
        assert_eq!(
            round_deadline(Some(12_000), 200_000, 60_000),
            90_000,
            "joined a minute in"
        );
        assert_eq!(round_deadline(Some(100_000), 200_000, 60_000), 100_000);
    }
}
