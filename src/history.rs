//! Local play history.
//!
//! Spotify does not record playback from librespot clients. Spotifast stores
//! local plays and merges them with `/me/player/recently-played`, which covers
//! other devices. A track counts only after enough listening time, so skips do
//! not fill the history. Each play keeps how long it was listened to and
//! where it was played from, which the Home map can size its nodes by.
//!
//! The newest plays live in `history.json`. With Keep every play, the plays
//! past the newest [`KEPT`] move, a batch at a time, to files of their own in
//! the archive folder instead of being let go. A batch leaves `history.json`
//! only once its file is on the disk, so a crash or a full disk can leave a
//! play in both places, never in neither; reading counts each play once.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use crate::api::models::{Album, Context, Image, PlayHistory, Track};

/// A play counts after 30 seconds, or halfway through a shorter track.
const COUNTS_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

/// The plays `history.json` keeps, newest first; with Keep every play off,
/// the oldest past these are let go.
const KEPT: usize = 500;

/// With Keep every play, once `history.json` holds this many plays, those
/// past the newest [`KEPT`] move to the archive together.
const ARCHIVE_AT: usize = 1000;

/// A play made here and a play of the same song Spotify reports within this
/// many seconds of it are the same play.
const SAME_PLAY: i64 = 60;

/// When a song has been listened to long enough to count.
pub fn counts_after(duration_ms: u32) -> std::time::Duration {
    let half = std::time::Duration::from_millis(u64::from(duration_ms) / 2);
    COUNTS_AFTER
        .min(half)
        .max(std::time::Duration::from_secs(1))
}

/// What listening time is counted under for a context: the playlist,
/// album, artist or show itself, and Liked Songs, whose ever they are, as
/// one.
pub fn context_key(uri: &str) -> String {
    if uri.ends_with(":collection") {
        return "spotify:collection".into();
    }
    crate::link::canonical_context_uri(uri)
}

/// What makes a play itself: when it was and what it was.
type PlayKey = (String, String);

fn key_of(play: &PlayHistory) -> Option<PlayKey> {
    Some((play.played_at.clone()?, play.track.uri.clone()))
}

fn millis(ms: std::time::Duration) -> u64 {
    ms.as_millis().try_into().unwrap_or(u64::MAX)
}

/// Adds a play's listening time to where it was played from.
fn count(totals: &mut HashMap<String, u64>, play: &PlayHistory) {
    if let (Some(context), Some(ms)) = (&play.context, play.listened_ms)
        && !context.uri.is_empty()
        && ms > 0
    {
        *totals.entry(context_key(&context.uri)).or_default() += ms;
    }
}

/// Work for the archive's own thread, done in the order it is handed over,
/// so a clear always follows the batches written before it.
enum Job {
    /// Count every archived play not among `skip`, which `history.json`
    /// holds and counts already.
    Scan { epoch: u64, skip: HashSet<PlayKey> },
    /// Write a batch of plays to a file of its own.
    Store {
        epoch: u64,
        path: PathBuf,
        text: String,
        count: usize,
    },
    /// Delete the whole archive, every play up to `before`, in
    /// milliseconds since the epoch.
    Clear { before: i64 },
}

enum Event {
    Scanned {
        epoch: u64,
        totals: HashMap<String, u64>,
    },
    Stored {
        epoch: u64,
        count: usize,
        written: bool,
    },
}

/// The archive folder and the thread that reads and writes it, off the UI
/// thread.
struct Archive {
    dir: PathBuf,
    jobs: mpsc::Sender<Job>,
    events: Arc<Mutex<Vec<Event>>>,
    /// Jobs handed over, and jobs the thread has finished.
    sent: u64,
    done: Arc<AtomicU64>,
}

impl Archive {
    fn send(&mut self, job: Job) -> bool {
        let sent = self.jobs.send(job).is_ok();
        self.sent += u64::from(sent);
        sent
    }
}

impl Archive {
    fn start(dir: PathBuf) -> Option<Self> {
        let (jobs, inbox) = mpsc::channel::<Job>();
        let events: Arc<Mutex<Vec<Event>>> = Arc::default();
        let outbox = events.clone();
        let done: Arc<AtomicU64> = Arc::default();
        let finished = done.clone();
        let folder = dir.clone();
        std::thread::Builder::new()
            .name("play-archive".into())
            .spawn(move || {
                // Ends when the history, and with it the sender, is gone.
                for job in inbox {
                    let event = match job {
                        Job::Scan { epoch, skip } => Some(Event::Scanned {
                            epoch,
                            totals: scan(&folder, skip),
                        }),
                        Job::Store {
                            epoch,
                            path,
                            text,
                            count,
                        } => {
                            let written = fastframe_file::write_private(&path, text)
                                .map_err(|error| log::warn!("could not archive plays: {error}"))
                                .is_ok();
                            Some(Event::Stored {
                                epoch,
                                count,
                                written,
                            })
                        }
                        Job::Clear { before } => {
                            clear(&folder, before);
                            None
                        }
                    };
                    if let Some(event) = event {
                        outbox
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .push(event);
                    }
                    finished.fetch_add(1, Ordering::SeqCst);
                }
            })
            .map_err(|error| log::error!("could not start the play archive: {error}"))
            .ok()?;
        Some(Self {
            dir,
            jobs,
            events,
            sent: 0,
            done,
        })
    }
}

/// Where a clear that has not finished yet is written down: beside the
/// archive, so deleting the archive leaves it.
fn cleared_marker(dir: &Path) -> PathBuf {
    dir.with_extension("cleared")
}

fn played_ms(play: &PlayHistory) -> Option<i64> {
    play.played_at
        .as_deref()
        .and_then(|at| at.parse::<jiff::Timestamp>().ok())
        .map(|at| at.as_millisecond())
}

/// Deletes the archive. Until that has worked, a marker says every play up
/// to `before` is gone, so a file the system would not let go of yet (one
/// held open by a virus scanner, say) is never counted again, and the next
/// scan deletes it.
fn clear(dir: &Path, before: i64) {
    if let Err(error) = fastframe_file::write_private(&cleared_marker(dir), before.to_string()) {
        log::warn!("could not note the play archive's clearing: {error}");
    }
    match std::fs::remove_dir_all(dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            log::warn!("could not clear the play archive: {error}");
            return;
        }
    }
    let _ = std::fs::remove_file(cleared_marker(dir));
}

/// Listening time by where it was played from, over every archived play
/// but those in `seen`, each play once however many files hold it. A file
/// that cannot be read is left as it is and skipped. Plays a clear has not
/// yet deleted are left out, and their files deleted.
fn scan(dir: &Path, mut seen: HashSet<PlayKey>) -> HashMap<String, u64> {
    let mut totals = HashMap::new();
    let marker = cleared_marker(dir);
    let cleared = std::fs::read_to_string(&marker)
        .ok()
        .and_then(|text| text.trim().parse::<i64>().ok());
    let gone = |play: &PlayHistory| {
        cleared.is_some_and(|before| played_ms(play).is_none_or(|at| at <= before))
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        if cleared.is_some() {
            let _ = std::fs::remove_file(&marker);
        }
        return totals;
    };
    let mut lingering = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let plays = std::fs::read_to_string(&path)
            .map_err(|error| error.to_string())
            .and_then(|text| {
                serde_json::from_str::<Vec<PlayHistory>>(&text).map_err(|error| error.to_string())
            });
        match plays {
            Ok(plays) => {
                if !plays.is_empty() && plays.iter().all(gone) {
                    if let Err(error) = std::fs::remove_file(&path) {
                        log::warn!(
                            "could not delete cleared plays in {}: {error}",
                            path.display()
                        );
                        lingering = true;
                    }
                    continue;
                }
                for play in plays.iter().filter(|play| !gone(play)) {
                    if key_of(play).is_some_and(|key| seen.insert(key)) {
                        count(&mut totals, play);
                    }
                }
                lingering |= plays.iter().any(gone);
            }
            Err(error) => log::warn!("skipping archived plays in {}: {error}", path.display()),
        }
    }
    if cleared.is_some() && !lingering {
        let _ = std::fs::remove_file(&marker);
    }
    totals
}

/// The plays made here, newest first.
#[derive(Default)]
pub struct History {
    plays: Vec<PlayHistory>,
    /// Set when the in-memory list differs from the file.
    dirty: bool,
    /// Set by the file writer when a handed-over write failed.
    failed: Arc<AtomicBool>,
    /// Keep every play: past the newest [`KEPT`], they move to the archive
    /// rather than being let go.
    keep_all: bool,
    /// The plays past the newest [`KEPT`] were kept on purpose and go to
    /// the archive before any is let go, Keep every play on or not.
    holding: bool,
    archive: Option<Archive>,
    /// Listening time in the archive by where it was played from; `None`
    /// until it has been read.
    archived: Option<HashMap<String, u64>>,
    /// Listening time by where it was played from, archive and all.
    totals: HashMap<String, u64>,
    /// How many of the oldest plays are on their way to the archive.
    storing: Option<usize>,
    /// Bumped by a clear, so work handed over before it is not counted.
    epoch: u64,
}

impl History {
    /// Reads the history, or returns an empty history if the file is unreadable.
    pub fn load(path: &Path) -> Self {
        let plays = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<PlayHistory>>(&text).ok())
            .unwrap_or_default();
        // Without Keep every play the file never holds more than KEPT, so
        // more were kept on purpose and wait for the archive.
        let holding = plays.len() > KEPT;
        let mut history = Self {
            plays,
            holding,
            ..Self::default()
        };
        history.recount();
        history
    }

    /// Keeps the plays past the newest [`KEPT`] in `dir`, and starts
    /// reading what it holds already, off this thread.
    pub fn with_archive(mut self, dir: PathBuf) -> Self {
        self.archive = Archive::start(dir);
        match &mut self.archive {
            Some(archive) => {
                let skip = self.plays.iter().filter_map(key_of).collect();
                archive.send(Job::Scan {
                    epoch: self.epoch,
                    skip,
                });
            }
            // With nowhere to keep them, nothing is archived.
            None => self.archived = Some(HashMap::new()),
        }
        self
    }

    pub fn plays(&self) -> &[PlayHistory] {
        &self.plays
    }

    pub fn is_empty(&self) -> bool {
        self.plays.is_empty()
    }

    /// Listening time in milliseconds by where it was played from, keyed by
    /// [`context_key`]: every play kept, archived or not.
    pub fn listening(&self) -> &HashMap<String, u64> {
        &self.totals
    }

    /// Whether the file may not hold the plays in memory.
    pub fn needs_save(&self) -> bool {
        self.dirty || self.failed.load(Ordering::SeqCst)
    }

    /// Turns Keep every play on or off. Off, the plays already kept past
    /// the newest [`KEPT`] still go to the archive, not away.
    pub fn set_keep_all(&mut self, keep_all: bool) {
        if self.keep_all == keep_all {
            return;
        }
        self.keep_all = keep_all;
        self.trim();
    }

    /// Hands the history to `files` if it changed since the last save. A
    /// write that fails leaves it to be written by the next save.
    pub fn save(&mut self, files: &fastframe_file::Writer, path: &Path) {
        let failed = self.failed.swap(false, Ordering::SeqCst);
        if !std::mem::take(&mut self.dirty) && !failed {
            return;
        }
        match serde_json::to_string(&self.plays) {
            Ok(text) => {
                // Replaced whole, as settings are: the history is written
                // when the session ends, and a shutdown that cuts the write
                // short must not leave half a file behind. The writer waits
                // until the new file and its move reach the disk, off the UI
                // thread, so a power cut cannot leave an empty history.
                // What someone listened to is theirs: on Unix the file is
                // readable by its owner only, whatever the old one allowed.
                let failed = self.failed.clone();
                files.write_or(path, text, fastframe_file::Access::Private, move || {
                    failed.store(true, Ordering::SeqCst)
                });
            }
            Err(error) => {
                self.dirty = true;
                log::warn!("could not write the play history: {error}");
            }
        }
    }

    /// Takes in what the archive's thread has done. Returns whether the
    /// plays or the listening time changed.
    pub fn poll(&mut self) -> bool {
        let Some(archive) = &self.archive else {
            return false;
        };
        let events = std::mem::take(
            &mut *archive
                .events
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        let mut changed = false;
        for event in events {
            match event {
                Event::Scanned { epoch, totals } if epoch == self.epoch => {
                    self.archived = Some(totals);
                    changed = true;
                }
                Event::Stored {
                    epoch,
                    count: stored,
                    written,
                } if epoch == self.epoch => {
                    self.storing = None;
                    // Kept in memory, and in the file, until written; a
                    // failed write is tried again with the next play.
                    if written && stored <= self.plays.len() {
                        let gone = self.plays.split_off(self.plays.len() - stored);
                        let archived = self.archived.get_or_insert_with(HashMap::new);
                        for play in &gone {
                            count(archived, play);
                        }
                        self.dirty = true;
                        changed = true;
                    }
                }
                _ => {}
            }
        }
        if changed {
            self.trim();
            self.recount();
        }
        changed
    }

    /// Writes down that `track` was played at `at` from `from`, listened to
    /// for `listened` so far, newest first.
    pub fn record(
        &mut self,
        track: Track,
        at: jiff::Timestamp,
        from: Option<&str>,
        listened: std::time::Duration,
    ) {
        let context = from.filter(|uri| !uri.is_empty()).map(|uri| {
            let uri = crate::link::canonical_context_uri(uri);
            Context {
                kind: if uri.ends_with(":collection") {
                    "collection".into()
                } else {
                    crate::util::uri_kind(&uri).unwrap_or_default().to_string()
                },
                uri,
            }
        });
        self.plays.insert(
            0,
            PlayHistory {
                track,
                played_at: Some(at.to_string()),
                context,
                listened_ms: Some(millis(listened)),
            },
        );
        self.holding |= self.keep_all;
        self.dirty = true;
        self.trim();
        self.recount();
    }

    /// Sets how long the play of `uri` written down at `at` has been
    /// listened to. Returns whether that changed anything.
    pub fn listened(&mut self, uri: &str, at: &str, listened: std::time::Duration) -> bool {
        let ms = millis(listened);
        let Some(play) = self
            .plays
            .iter_mut()
            .find(|play| play.track.uri == uri && play.played_at.as_deref() == Some(at))
        else {
            return false;
        };
        if play.listened_ms == Some(ms) {
            return false;
        }
        play.listened_ms = Some(ms);
        self.dirty = true;
        self.recount();
        true
    }

    /// Lets go of the oldest plays past [`KEPT`] or, kept on purpose, hands
    /// them to the archive.
    fn trim(&mut self) {
        if self.plays.len() <= KEPT {
            self.holding = false;
            return;
        }
        if self.storing.is_some() {
            return;
        }
        // Kept on purpose, they wait for an archive rather than go.
        if !self.holding && !self.keep_all {
            self.plays.truncate(KEPT);
            self.dirty = true;
            return;
        }
        // With Keep every play on they go in batches; off, at once, so the
        // file soon holds only what it would without the archive.
        if self.keep_all && self.plays.len() < ARCHIVE_AT {
            return;
        }
        // Not before the archive has been read: a batch written while it
        // is being read could be counted twice.
        if self.archived.is_none() {
            return;
        }
        let Some(archive) = &mut self.archive else {
            return;
        };
        let batch = &self.plays[KEPT..];
        let at = |play: Option<&PlayHistory>| {
            play.and_then(|play| play.played_at.as_deref())
                .and_then(|at| at.parse::<jiff::Timestamp>().ok())
                .map_or(0, |at| at.as_millisecond())
        };
        // Named for the plays it holds, oldest to newest.
        let path = archive
            .dir
            .join(format!("{}-{}.json", at(batch.last()), at(batch.first())));
        match serde_json::to_string(batch) {
            Ok(text) => {
                let count = batch.len();
                let job = Job::Store {
                    epoch: self.epoch,
                    path,
                    text,
                    count,
                };
                if archive.send(job) {
                    self.storing = Some(count);
                }
            }
            Err(error) => log::warn!("could not archive plays: {error}"),
        }
    }

    fn recount(&mut self) {
        let mut totals = self.archived.clone().unwrap_or_default();
        for play in &self.plays {
            count(&mut totals, play);
        }
        self.totals = totals;
    }

    /// Waits, for at most `timeout`, until the archive's thread has done
    /// everything handed to it: a batch being written, or a clear. Exit
    /// calls it, as it waits for the file writer.
    pub fn flush(&self, timeout: std::time::Duration) -> bool {
        let Some(archive) = &self.archive else {
            return true;
        };
        let deadline = std::time::Instant::now() + timeout;
        while archive.done.load(Ordering::SeqCst) < archive.sent {
            if std::time::Instant::now() >= deadline {
                log::warn!("the play archive is still busy at exit");
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        true
    }

    /// Lets go of every play, the archived ones too.
    pub fn clear(&mut self) {
        self.epoch += 1;
        self.storing = None;
        self.holding = false;
        if let Some(archive) = &mut self.archive {
            let before = jiff::Timestamp::now().as_millisecond();
            archive.send(Job::Clear { before });
            self.archived = Some(HashMap::new());
        }
        if !self.plays.is_empty() {
            self.plays.clear();
            self.dirty = true;
        }
        self.recount();
    }

    /// Waits until the archive's thread has done everything handed to it
    /// and takes it in, for tests.
    #[cfg(test)]
    pub(crate) fn settle(&mut self) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < deadline {
            self.poll();
            if self.archived.is_some() && self.storing.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("the play archive did not settle");
    }
}

/// Converts the playing track into a history record.
pub fn played_track(now: &crate::app::NowPlaying) -> Track {
    Track {
        id: now.id.clone(),
        name: now.title.clone(),
        uri: now.uri.clone(),
        duration_ms: now.duration_ms,
        artists: now.artists.clone(),
        album: Some(Album {
            id: now.album_id.clone().unwrap_or_default(),
            name: now.album_name.clone(),
            images: now
                .art_url
                .as_deref()
                .or(now.art_small.as_deref())
                .map(|url| {
                    vec![Image {
                        url: url.to_string(),
                        width: None,
                        height: None,
                    }]
                })
                .unwrap_or_default(),
            ..Album::default()
        }),
        ..Track::default()
    }
}

/// Merges local and Spotify history, newest first.
///
/// A play Spotify reports within the duplicate window of one made here is
/// that same play, and is shown once. Plays from one source are never
/// merged with each other: each is its own listen, however close together
/// they start. Entries without a timestamp sort to the end.
pub fn merged(local: &[PlayHistory], remote: &[PlayHistory]) -> Vec<PlayHistory> {
    let time = |play: &PlayHistory| {
        play.played_at
            .as_deref()
            .and_then(|at| at.parse::<jiff::Timestamp>().ok())
            .map(|at| at.as_second())
    };
    // The plays made here that no report from Spotify has matched yet.
    let mut unmatched: HashMap<&str, Vec<i64>> = HashMap::new();
    let mut out: Vec<(Option<i64>, PlayHistory)> = Vec::new();
    for play in local {
        let at = time(play);
        if let Some(at) = at {
            unmatched
                .entry(play.track.uri.as_str())
                .or_default()
                .push(at);
        }
        out.push((at, play.clone()));
    }
    for play in remote {
        let at = time(play);
        // Each play made here stands for at most one report: the closest.
        if let Some(at) = at
            && let Some(times) = unmatched.get_mut(play.track.uri.as_str())
            && let Some(nearest) = times
                .iter()
                .enumerate()
                .filter(|&(_, &held)| (held - at).abs() <= SAME_PLAY)
                .min_by_key(|&(_, &held)| (held - at).abs())
                .map(|(index, _)| index)
        {
            times.swap_remove(nearest);
            continue;
        }
        out.push((at, play.clone()));
    }
    out.sort_by(|a, b| match (a.0, b.0) {
        (Some(a), Some(b)) => b.cmp(&a),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    });
    out.into_iter().map(|(_, play)| play).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastframe_file::Writer;
    use std::time::Duration;

    fn play(uri: &str, at: &str) -> PlayHistory {
        PlayHistory {
            track: Track {
                uri: uri.to_string(),
                ..Track::default()
            },
            played_at: Some(at.to_string()),
            context: None,
            listened_ms: None,
        }
    }

    /// A play counts after 30 seconds or halfway through a shorter track.
    #[test]
    fn a_song_counts_after_half_a_minute_or_half_of_it() {
        assert_eq!(counts_after(240_000).as_secs(), 30, "a four minute song");
        assert_eq!(counts_after(40_000).as_secs(), 20, "a forty second song");
        // Always require at least one second.
        assert!(counts_after(0) >= std::time::Duration::from_secs(1));
    }

    /// A counted play is not added again on later frames.
    #[test]
    fn counting_never_overflows_however_long_a_song_runs() {
        let threshold = counts_after(240_000);
        let mut listened = std::time::Duration::ZERO;
        let mut recorded = 0;
        // Continue for an hour after crossing the threshold.
        for _ in 0..3_600 {
            listened += std::time::Duration::from_secs(1);
            if recorded == 0 && listened >= threshold {
                recorded += 1;
            }
        }
        assert_eq!(recorded, 1, "written down once, and it did not panic");
    }

    /// Both sources are sorted together, newest first.
    #[test]
    fn the_two_histories_interleave_by_time() {
        let local = vec![
            play("spotify:track:here-late", "2026-09-01T15:00:00Z"),
            play("spotify:track:here-early", "2026-09-01T09:00:00Z"),
        ];
        let remote = vec![play("spotify:track:phone", "2026-09-01T12:00:00Z")];
        let rows = merged(&local, &remote);
        let uris: Vec<&str> = rows.iter().map(|play| play.track.uri.as_str()).collect();
        assert_eq!(
            uris,
            vec![
                "spotify:track:here-late",
                "spotify:track:phone",
                "spotify:track:here-early"
            ]
        );
    }

    /// Separate plays of the same track remain separate rows.
    #[test]
    fn the_same_song_played_twice_is_two_rows() {
        let local = vec![
            play("spotify:track:a", "2026-09-01T15:00:00Z"),
            play("spotify:track:a", "2026-09-01T09:00:00Z"),
        ];
        assert_eq!(merged(&local, &[]).len(), 2);
    }

    /// A play reported by both sources appears once.
    #[test]
    fn one_play_seen_twice_is_one_row() {
        let local = vec![play("spotify:track:a", "2026-09-01T15:00:00Z")];
        let remote = vec![play("spotify:track:a", "2026-09-01T15:00:20Z")];
        assert_eq!(merged(&local, &remote).len(), 1, "twenty seconds apart");
        let distant = vec![play("spotify:track:a", "2026-09-01T15:05:00Z")];
        assert_eq!(merged(&local, &distant).len(), 2, "five minutes apart");
    }

    /// Each source reports every play it heard: a short song played twice
    /// in a row is two plays, however close together they start. Only a
    /// play that both sources report becomes one row.
    #[test]
    fn a_short_song_played_twice_by_one_source_is_two_rows() {
        // A forty-second interlude played twice, on another device.
        let remote = vec![
            play("spotify:track:short", "2026-09-01T15:00:40Z"),
            play("spotify:track:short", "2026-09-01T15:00:00Z"),
        ];
        assert_eq!(merged(&[], &remote).len(), 2, "two plays on another device");
        // The same two plays, made here.
        assert_eq!(merged(&remote, &[]).len(), 2, "two plays on this computer");
        // Made here and reported by Spotify as well: two plays, each once.
        let reported = vec![
            play("spotify:track:short", "2026-09-01T15:00:45Z"),
            play("spotify:track:short", "2026-09-01T15:00:05Z"),
        ];
        assert_eq!(merged(&remote, &reported).len(), 2, "each play once");
    }

    /// A play without a timestamp sorts to the end.
    #[test]
    fn a_play_with_no_time_sinks_to_the_end() {
        let mut timeless = play("spotify:track:timeless", "");
        timeless.played_at = None;
        let local = vec![timeless, play("spotify:track:a", "2026-09-01T09:00:00Z")];
        let rows = merged(&local, &[]);
        let uris: Vec<&str> = rows.iter().map(|play| play.track.uri.as_str()).collect();
        assert_eq!(uris, vec!["spotify:track:a", "spotify:track:timeless"]);
    }

    /// The newest play comes first and the list is capped.
    #[test]
    fn the_newest_play_is_first_and_the_list_is_capped() {
        let mut history = History::default();
        let at: jiff::Timestamp = "2026-09-01T09:00:00Z".parse().unwrap();
        for index in 0..KEPT + 10 {
            history.record(
                Track {
                    uri: format!("spotify:track:{index}"),
                    ..Track::default()
                },
                at,
                None,
                Duration::ZERO,
            );
        }
        assert_eq!(history.plays().len(), KEPT, "the oldest fall off the end");
        assert_eq!(
            history.plays()[0].track.uri,
            format!("spotify:track:{}", KEPT + 9),
            "the newest is first"
        );
    }

    /// With Keep every play, nothing is let go: past the newest KEPT, plays
    /// move to the archive and are still counted, once each, read back or
    /// stored twice. Turned off, the plays already kept go to the archive
    /// too, and Clear history takes everything.
    #[test]
    fn every_play_is_kept_and_counted_once() {
        let dir =
            std::env::temp_dir().join(format!("spotifast-archive-{:016x}", rand::random::<u64>()));
        let path = dir.join("history.json");
        let archive = dir.join("plays");
        let files = Writer::new();
        let mut history = History::load(&path).with_archive(archive.clone());
        history.set_keep_all(true);
        history.settle();
        let start: jiff::Timestamp = "2026-09-01T09:00:00Z".parse().unwrap();
        let total = ARCHIVE_AT + 5;
        for index in 0..total {
            history.record(
                Track {
                    uri: format!("spotify:track:{index}"),
                    ..Track::default()
                },
                start + jiff::SignedDuration::from_secs(index as i64 * 200),
                Some("spotify:user:me:playlist:mix"),
                Duration::from_secs(1),
            );
        }
        history.settle();
        history.save(&files, &path);
        assert!(files.flush(Duration::from_secs(10)));
        assert_eq!(history.plays().len(), KEPT + 5, "the oldest moved out");
        assert_eq!(
            history.plays()[0].track.uri,
            format!("spotify:track:{}", total - 1)
        );
        let every = Some(&(total as u64 * 1000));
        assert_eq!(history.listening().get("spotify:playlist:mix"), every);
        let stored: Vec<_> = std::fs::read_dir(&archive)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(stored.len(), 1, "one batch");
        // A batch written again, as after a crash, counts once.
        std::fs::copy(&stored[0], archive.join("again.json")).unwrap();
        drop(history);

        let mut again = History::load(&path).with_archive(archive.clone());
        again.set_keep_all(true);
        again.settle();
        assert_eq!(again.plays().len(), KEPT + 5);
        assert_eq!(again.listening().get("spotify:playlist:mix"), every);

        // Off, the newest KEPT stay and the rest are archived, not lost.
        again.set_keep_all(false);
        again.settle();
        assert_eq!(again.plays().len(), KEPT);
        assert_eq!(again.listening().get("spotify:playlist:mix"), every);
        again.record(
            Track::default(),
            start + jiff::SignedDuration::from_secs(total as i64 * 200),
            None,
            Duration::from_secs(1),
        );
        assert_eq!(again.plays().len(), KEPT, "off, the oldest is let go");

        again.clear();
        assert!(again.flush(Duration::from_secs(10)), "the clear is done");
        again.settle();
        assert!(again.listening().is_empty());
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while archive.exists() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!archive.exists(), "the archive is gone too");
        drop(again);

        // A clear the system did not let finish, a file held open: the
        // marker it left keeps those plays from counting, and the next
        // read deletes them.
        std::fs::create_dir_all(&archive).unwrap();
        let mut stale = History::default();
        stale.record(
            Track::default(),
            start,
            Some("spotify:playlist:mix"),
            Duration::from_secs(1),
        );
        std::fs::write(
            archive.join("held.json"),
            serde_json::to_string(stale.plays()).unwrap(),
        )
        .unwrap();
        std::fs::write(
            cleared_marker(&archive),
            (start.as_millisecond() + 1).to_string(),
        )
        .unwrap();
        let mut later = History::load(&dir.join("none.json")).with_archive(archive.clone());
        later.settle();
        assert!(later.listening().is_empty(), "cleared plays stay cleared");
        assert!(!archive.join("held.json").exists());
        assert!(!cleared_marker(&archive).exists(), "the clear is done");
        drop(later);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A save replaces the whole file and leaves no temporary behind, and a
    /// second save over it reads back the newer history.
    #[test]
    fn a_saved_history_reads_back_whole() {
        let dir =
            std::env::temp_dir().join(format!("spotifast-history-{:016x}", rand::random::<u64>()));
        let path = dir.join("history.json");
        let at: jiff::Timestamp = "2026-09-01T09:00:00Z".parse().unwrap();
        let files = Writer::new();
        let mut history = History::default();
        for uri in ["spotify:track:a", "spotify:track:b"] {
            history.record(
                Track {
                    uri: uri.into(),
                    ..Track::default()
                },
                at,
                None,
                Duration::ZERO,
            );
            history.save(&files, &path);
        }
        assert!(files.flush(Duration::from_secs(10)));
        let read = History::load(&path);
        let uris: Vec<_> = read
            .plays()
            .iter()
            .map(|play| play.track.uri.as_str())
            .collect();
        assert_eq!(uris, ["spotify:track:b", "spotify:track:a"]);
        assert!(crate::util::temporary_files(path.parent().unwrap()).is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A save that cannot write keeps the plays to write, so the next save
    /// still has them rather than losing the song just recorded.
    #[test]
    fn a_failed_save_keeps_the_history_to_write() {
        let dir =
            std::env::temp_dir().join(format!("spotifast-history-{:016x}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        // A file where the history's folder should be: nothing can be
        // written under it.
        let blocked = dir.join("blocked");
        std::fs::write(&blocked, "").unwrap();
        let mut history = History::default();
        history.record(
            Track {
                uri: "spotify:track:a".into(),
                ..Track::default()
            },
            "2026-09-01T09:00:00Z".parse().unwrap(),
            None,
            Duration::ZERO,
        );
        let files = Writer::new();
        history.save(&files, &blocked.join("history.json"));
        assert!(files.flush(Duration::from_secs(10)));
        assert!(history.needs_save(), "the failed write is still to be done");

        let path = dir.join("history.json");
        history.save(&files, &path);
        assert!(files.flush(Duration::from_secs(10)));
        assert!(!history.needs_save());
        assert_eq!(History::load(&path).plays().len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The history is readable by its owner only, even when it replaces a
    /// file others could read.
    #[cfg(unix)]
    #[test]
    fn the_history_is_kept_from_other_accounts() {
        use std::os::unix::fs::PermissionsExt;
        let dir =
            std::env::temp_dir().join(format!("spotifast-history-{:016x}", rand::random::<u64>()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("history.json");
        std::fs::write(&path, "[]").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let mut history = History::default();
        history.record(
            Track {
                uri: "spotify:track:a".into(),
                ..Track::default()
            },
            "2026-09-01T09:00:00Z".parse().unwrap(),
            None,
            Duration::ZERO,
        );
        let files = Writer::new();
        history.save(&files, &path);
        assert!(files.flush(Duration::from_secs(10)));
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
