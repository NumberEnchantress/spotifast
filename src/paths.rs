//! Where Sirensong keeps its files.
//!
//! Configuration, durable non-secret state, and disposable caches live in the
//! platform's conventional directories, or, for a copy started from the
//! portable Windows zip, in `sirensong-data` next to the executable (#703).
//! The first start under the name Sirensong brings over the folders kept
//! under the old name, Spotifast.
//! Spotify grants use the platform store; the token paths below are retained
//! only for migration and sign-out cleanup.

use std::path::{Path, PathBuf};

/// Where Sirensong's folders are: the per-user folders, or a portable copy's
/// own. A new portable folder copies the per-user settings and state, except
/// the single-instance files a running installed copy holds locked and the
/// logs of its runs.
const FOLDERS: fastframe_dirs::App = fastframe_dirs::App {
    not_copied: &["instance", "sirensong.log", "panic.log", "local-files"],
    ..fastframe_dirs::App::new("sirensong", "me", "paolino", "sirensong")
};

/// The folders kept under the old name, Spotifast, which the first start
/// as Sirensong brings over.
const SPOTIFAST: fastframe_dirs::App =
    fastframe_dirs::App::new("spotifast", "me", "paolino", "spotifast");

/// What a folder holds that is never brought over: a running copy's lock,
/// and the logs of past runs.
const NOT_MIGRATED: &[&str] = &[
    "instance",
    "spotifast.log",
    "sirensong.log",
    "panic.log",
    MIGRATED,
    MIGRATING,
];

#[derive(Clone, Debug)]
pub struct AppDirs {
    pub config: PathBuf,
    pub state: PathBuf,
    pub cache: PathBuf,
}

impl From<fastframe_dirs::Dirs> for AppDirs {
    fn from(dirs: fastframe_dirs::Dirs) -> Self {
        Self {
            config: dirs.config,
            state: dirs.state,
            cache: dirs.cache,
        }
    }
}

/// The folders [`AppDirs::discover`] chose, and what to log about them once
/// logging has started.
pub struct Found {
    pub dirs: AppDirs,
    lines: Vec<(log::Level, String)>,
}

impl Found {
    /// Folders chosen elsewhere, such as a demo's, with nothing to log.
    pub fn given(dirs: AppDirs) -> Self {
        Self {
            dirs,
            lines: Vec::new(),
        }
    }

    pub fn log(&self) {
        for (level, line) in &self.lines {
            log::log!(*level, "{line}");
        }
    }
}

impl AppDirs {
    /// The folders this copy uses: a portable copy's own, made the first
    /// time from the per-user ones, or the per-user ones. Call it once this
    /// process is the running copy, before the window opens.
    pub fn discover() -> Found {
        let migrated = migrate_from_spotifast();
        let found = FOLDERS.discover(|copy, from| {
            // The copy starts with the sign-in it was made from, under
            // credential entries of its own.
            crate::credentials::adopt_profile(&copy.state, &from.state)
        });
        let dirs = found.dirs.map_or_else(Self::fallback, Self::from);
        let mut lines: Vec<_> = describe(&dirs, &found.mode).into_iter().collect();
        lines.extend(migrated);
        if matches!(found.mode, fastframe_dirs::Mode::Portable { .. }) {
            // Its sign-in follows the folder when it moves to another drive.
            if let Err(error) = crate::credentials::keep_profile(&dirs.state) {
                lines.push((
                    log::Level::Warn,
                    format!("unable to record the portable credential profile: {error}"),
                ));
            }
        }
        Found { dirs, lines }
    }

    /// The per-user folders, which an installed copy uses whether or not
    /// this copy is portable.
    pub fn installed() -> Self {
        FOLDERS.installed().map_or_else(Self::fallback, Self::from)
    }

    /// The single-instance slot's scope for a portable copy, so it never
    /// hands its launches to an installed copy, or to a portable copy in
    /// another folder, which keep other settings.
    pub fn instance_scope() -> Option<String> {
        FOLDERS.portable().instance_scope()
    }

    /// No home folder at all: the current directory.
    fn fallback() -> Self {
        let fallback = std::env::current_dir().unwrap_or_default();
        Self {
            config: fallback.join("sirensong-config"),
            state: fallback.join("sirensong-state"),
            cache: fallback.join("sirensong-cache"),
        }
    }

    pub fn settings_file(&self) -> PathBuf {
        self.config.join("settings.json")
    }

    /// Winamp skins the listener has added, as `.wsz` files or folders.
    pub fn skins_dir(&self) -> PathBuf {
        self.config.join("skins")
    }

    /// MilkDrop presets, as `.milk` files, in folders or not, with any
    /// textures they use in a `textures` folder inside.
    pub fn milkdrop_dir(&self) -> PathBuf {
        self.config.join("milkdrop")
    }

    pub fn session_file(&self) -> PathBuf {
        self.state.join("session.json")
    }

    /// What was played here, which Spotify never hears about and so
    /// cannot tell us later. See [`crate::history`].
    pub fn history_file(&self) -> PathBuf {
        self.state.join("history.json")
    }

    /// Older plays, kept with Keep every play, in files of their own. See
    /// [`crate::history`].
    pub fn plays_dir(&self) -> PathBuf {
        self.state.join("plays")
    }

    pub fn shared_web_token_file(&self) -> PathBuf {
        self.state.join("shared_web_api_token.json")
    }

    pub fn personal_web_token_file(&self) -> PathBuf {
        self.state.join("personal_web_api_token.json")
    }

    pub fn legacy_web_token_file(&self) -> PathBuf {
        self.state.join("web_api_token.json")
    }

    /// The log of the current run, replaced at every start.
    pub fn log_file(&self) -> PathBuf {
        self.state.join("sirensong.log")
    }

    /// Where a panic is recorded before the process dies of it.
    pub fn panic_log(&self) -> PathBuf {
        self.state.join("panic.log")
    }

    pub fn credentials_dir(&self) -> PathBuf {
        self.state.join("credentials")
    }

    /// Optional proxy password, owner-only, never written to settings.json.
    pub fn proxy_secret_file(&self) -> PathBuf {
        self.state.join("proxy_password")
    }

    pub fn volume_dir(&self) -> PathBuf {
        self.state.join("volume")
    }

    /// Copies of the songs added to local files one at a time.
    pub fn added_songs_dir(&self) -> PathBuf {
        self.state.join("local-files")
    }

    pub fn audio_cache_dir(&self) -> PathBuf {
        self.cache.join("audio")
    }

    pub fn art_cache_dir(&self) -> PathBuf {
        self.cache.join("art")
    }

    pub fn lyrics_cache_dir(&self) -> PathBuf {
        self.cache.join("lyrics")
    }

    pub fn playlist_cache_dir(&self) -> PathBuf {
        self.cache.join("playlists")
    }

    pub fn account_playlist_cache_dir(&self, account_id: &str) -> PathBuf {
        self.playlist_cache_dir().join(account_id)
    }

    pub fn liked_songs_cache_file(&self, account_id: &str) -> PathBuf {
        self.cache
            .join("liked-songs")
            .join(account_file_name(account_id))
    }

    /// The account's last playlist list, shown while it is read again.
    pub fn library_cache_file(&self, account_id: &str) -> PathBuf {
        self.cache
            .join("library")
            .join(account_file_name(account_id))
    }

    /// The account's last Home shelves, shown while Home is read again.
    pub fn home_cache_file(&self, account_id: &str) -> PathBuf {
        self.cache.join("home").join(account_file_name(account_id))
    }

    /// The account snapshots the backend writes as it goes: the playlist
    /// list, Home and Liked Songs.
    pub fn account_snapshot_dirs(&self) -> [PathBuf; 3] {
        [
            self.cache.join("library"),
            self.cache.join("home"),
            self.cache.join("liked-songs"),
        ]
    }

    /// Every cache that holds an account's library: the playlist pages and
    /// the snapshots. Signing out removes them all.
    pub fn account_cache_dirs(&self) -> [PathBuf; 4] {
        let [library, home, liked] = self.account_snapshot_dirs();
        [self.playlist_cache_dir(), library, home, liked]
    }

    pub fn ensure(&self) -> std::io::Result<()> {
        for dir in [&self.config, &self.state, &self.cache] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// One account's JSON file name. Hex encoding also keeps unusual account
/// IDs within the cache root.
fn account_file_name(account_id: &str) -> String {
    let account: String = account_id
        .bytes()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{account}.json")
}

/// Brings the folders kept as Spotifast over to Sirensong's on the first
/// start under the new name, for an installed copy: settings and state are
/// copied, so the old copy still works, the sign-in with them, and the cache
/// is moved. The window's place and the start at login follow. Runs before
/// the window opens.
fn migrate_from_spotifast() -> Vec<(log::Level, String)> {
    if FOLDERS.portable().folder().is_some() {
        return Vec::new();
    }
    let (Some(old), Some(new)) = (SPOTIFAST.installed(), FOLDERS.installed()) else {
        return Vec::new();
    };
    let new = AppDirs::from(new);
    let lines = migrate_folders(&AppDirs::from(old), &new);
    if lines.is_empty() || !new.state.join(MIGRATED).is_file() {
        return lines;
    }
    // Where the window was.
    if let (Some(old), Some(new)) = (
        eframe::storage_dir("spotifast"),
        eframe::storage_dir("sirensong"),
    ) {
        let (old, new) = (old.join("app.ron"), new.join("app.ron"));
        if old.is_file() && !new.exists() {
            let _ = std::fs::create_dir_all(new.parent().unwrap_or(&new));
            let _ = std::fs::copy(old, new);
        }
    }
    let mut lines = lines;
    // Starting at login. Windows names the registration by the name shown,
    // so it moves from Spotifast's to Sirensong's.
    #[cfg(windows)]
    {
        let old = fastframe_autostart::Autostart::new("spotifast", "Spotifast");
        if let Some(start) = old.state().start() {
            lines.push(
                match crate::backend::autostart()
                    .set(Some(start))
                    .and_then(|_| old.set(None))
                {
                    Ok(_) => (log::Level::Info, "moved the start at login".to_string()),
                    Err(error) => (
                        log::Level::Warn,
                        format!("unable to move the start at login: {error}"),
                    ),
                },
            );
        }
    }
    // On Linux the entry keeps its name but starts the renamed program. In a
    // Flatpak the portal may ask the person first, so it is left to them.
    #[cfg(target_os = "linux")]
    if std::env::var_os("FLATPAK_ID").is_none() {
        let autostart = crate::backend::autostart();
        if let Some(start) = autostart.state().start() {
            lines.push(match autostart.set(Some(start)) {
                Ok(_) => (
                    log::Level::Info,
                    "pointed the start at login at sirensong".to_string(),
                ),
                Err(error) => (
                    log::Level::Warn,
                    format!("unable to point the start at login at sirensong: {error}"),
                ),
            });
        }
    }
    lines
}

/// Written in the new state folder once Spotifast's folders are brought
/// over, or once Sirensong's folders are found to be its own: from then
/// on, nothing of Spotifast's is brought over again.
const MIGRATED: &str = "migrated-from-spotifast";

/// Written in the new state folder while folders are being brought over; a
/// start that finds it tries again.
const MIGRATING: &str = "migrating-from-spotifast";

/// Copies `old`'s config and state into `new`'s and moves its cache, unless
/// that was done before: what was done, as log lines, or nothing.
///
/// Everything is copied beside its place first, and only then moved in, so a
/// copy that fails (a full disk) changes nothing. A move that stops part way
/// is finished by the next start, which replaces what it finds. The
/// migration counts as done only once both folders are in.
fn migrate_folders(old: &AppDirs, new: &AppDirs) -> Vec<(log::Level, String)> {
    let mut lines = Vec::new();
    if new.state.join(MIGRATED).exists() {
        return lines;
    }
    // Nothing of Spotifast's to bring over, or Sirensong's folders already
    // hold its own (a first start with no Spotifast, used since): they stay
    // Sirensong's for good, whatever appears later.
    let unfinished = new.state.join(MIGRATING).exists();
    if !(old.config.is_dir() || old.state.is_dir())
        || (!unfinished && !(unused(&new.config) && unused(&new.state)))
    {
        let _ = std::fs::create_dir_all(&new.state)
            .and_then(|()| std::fs::write(new.state.join(MIGRATED), ""));
        return lines;
    }
    // Grants are saved under a profile named after the state folder's path
    // unless the folder records one: the old folder records its own, and
    // the copy carries it, so the sign-in comes along.
    if old.state.is_dir()
        && let Err(error) = crate::credentials::keep_profile(&old.state)
    {
        lines.push((
            log::Level::Warn,
            format!(
                "not bringing over the Spotifast folders, as the sign-in would not follow: {error}"
            ),
        ));
        return lines;
    }
    // Each folder once: macOS keeps settings and state in one.
    let mut pairs: Vec<(&Path, &Path)> = Vec::new();
    for (from, to) in [(&old.config, &new.config), (&old.state, &new.state)] {
        if from.is_dir() && !pairs.iter().any(|(_, seen)| *seen == to.as_path()) {
            pairs.push((from, to));
        }
    }
    let clean = |pairs: &[(&Path, &Path)]| {
        for (_, to) in pairs {
            let _ = std::fs::remove_dir_all(partial(to));
        }
    };
    clean(&pairs);
    // Noted before anything is copied: a start after a failed attempt, even
    // one that left the folders as they were, tries again rather than take
    // what that start wrote for Sirensong's own.
    if let Err(error) = std::fs::create_dir_all(&new.state)
        .and_then(|()| std::fs::write(new.state.join(MIGRATING), ""))
    {
        lines.push((
            log::Level::Warn,
            format!("unable to start bringing over the Spotifast folders: {error}"),
        ));
        return lines;
    }
    if let Err(error) = pairs
        .iter()
        .try_for_each(|(from, to)| copy_tree(from, &partial(to), true))
    {
        clean(&pairs);
        lines.push((
            log::Level::Warn,
            format!("unable to copy the Spotifast folders; trying again next start: {error}"),
        ));
        return lines;
    }
    for (from, to) in &pairs {
        if let Err(error) = move_in(&partial(to), to) {
            clean(&pairs);
            lines.push((
                log::Level::Warn,
                format!(
                    "unable to bring over {}; trying again next start: {error}",
                    from.display()
                ),
            ));
            return lines;
        }
        lines.push((
            log::Level::Info,
            format!("brought over {} to {}", from.display(), to.display()),
        ));
    }
    clean(&pairs);
    rename_default_device(&new.config.join("settings.json"));
    match std::fs::write(new.state.join(MIGRATED), "") {
        Ok(()) => {
            let _ = std::fs::remove_file(new.state.join(MIGRATING));
        }
        Err(error) => lines.push((
            log::Level::Warn,
            format!("unable to note that the Spotifast folders were brought over: {error}"),
        )),
    }
    if old.cache.is_dir() && unused(&new.cache) {
        let moved =
            std::fs::create_dir_all(new.cache.parent().unwrap_or(&new.cache)).and_then(|()| {
                let _ = std::fs::remove_dir(&new.cache);
                std::fs::rename(&old.cache, &new.cache)
            });
        lines.push(match moved {
            Ok(()) => (
                log::Level::Info,
                format!("moved the cache to {}", new.cache.display()),
            ),
            // Held open by a running Spotifast, say: the cache fills again.
            Err(error) => (
                log::Level::Info,
                format!("left the cache in {}: {error}", old.cache.display()),
            ),
        });
    }
    lines
}

/// The folder beside `to` that a copy is made in before it is moved in.
fn partial(to: &Path) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(".partial");
    to.with_file_name(name)
}

/// Moves what `partial` holds into `to`, replacing what an unfinished
/// earlier attempt, or a start in between, put there.
fn move_in(partial: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(partial)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if target.is_dir() {
            std::fs::remove_dir_all(&target)?;
        } else if target.symlink_metadata().is_ok() {
            std::fs::remove_file(&target)?;
        }
        std::fs::rename(entry.path(), &target)?;
    }
    Ok(())
}

/// The Connect device name Spotifast gave by default becomes Sirensong's; a
/// name of your own stays.
fn rename_default_device(settings: &Path) {
    let Ok(text) = std::fs::read_to_string(settings) else {
        return;
    };
    let Ok(mut value) =
        serde_json::from_str::<serde_json::Value>(text.strip_prefix('\u{feff}').unwrap_or(&text))
    else {
        return;
    };
    if value.get("device_name").and_then(serde_json::Value::as_str) != Some("Spotifast") {
        return;
    }
    value["device_name"] = "Sirensong".into();
    if let Ok(text) = serde_json::to_string_pretty(&value) {
        let _ = std::fs::write(settings, text);
    }
}

/// Whether a folder is missing, or holds only what is never brought over
/// (the single-instance lock is made before the folders are found).
fn unused(folder: &Path) -> bool {
    std::fs::read_dir(folder).map_or(true, |entries| {
        entries.flatten().all(|entry| {
            NOT_MIGRATED
                .iter()
                .any(|name| entry.file_name() == std::ffi::OsStr::new(name))
                // Omarchy's theme hook can write its palette before the
                // first start.
                || (entry.file_name() == "themes" && only_omarchy(&entry.path()))
        })
    })
}

/// Whether a themes folder holds nothing but what Omarchy's hook writes.
fn only_omarchy(themes: &Path) -> bool {
    std::fs::read_dir(themes).is_ok_and(|entries| {
        entries.flatten().all(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name == "omarchy.json" || name.starts_with(".omarchy.")
        })
    })
}

/// Copies `from` into `to`, following links, so a settings file or themes
/// folder linked from elsewhere comes along as what it holds.
fn copy_tree(from: &Path, to: &Path, top: bool) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if top
            && NOT_MIGRATED
                .iter()
                .any(|name| entry.file_name() == std::ffi::OsStr::new(name))
        {
            continue;
        }
        // A link to nothing is left behind.
        let Ok(metadata) = std::fs::metadata(entry.path()) else {
            continue;
        };
        let target = to.join(entry.file_name());
        if metadata.is_dir() {
            copy_tree(&entry.path(), &target, false)?;
        } else if metadata.is_file() {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// The log line for the folders a copy uses, `dirs`, chosen for `mode`. An
/// installed copy says nothing.
fn describe(dirs: &AppDirs, mode: &fastframe_dirs::Mode) -> Option<(log::Level, String)> {
    Some(match mode {
        fastframe_dirs::Mode::Installed => return None,
        fastframe_dirs::Mode::Portable {
            folder,
            copied_from: None,
        } => (
            log::Level::Info,
            format!("portable copy: keeping files in {}", folder.display()),
        ),
        fastframe_dirs::Mode::Portable {
            folder,
            copied_from: Some(from),
        } => (
            log::Level::Info,
            format!(
                "portable copy: keeping files in {}, starting from a copy of the settings in {} and the state in {}",
                folder.display(),
                from.config.display(),
                from.state.display()
            ),
        ),
        fastframe_dirs::Mode::Fallback { folder, error } => (
            log::Level::Warn,
            format!(
                "portable copy, but {} is not usable ({error}); keeping files in {} and {}",
                folder.display(),
                dirs.config.display(),
                dirs.state.display()
            ),
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The first start as Sirensong copies Spotifast's settings and state,
    /// with the sign-in's profile recorded so its grants stay reachable,
    /// leaves behind the old copy's lock and logs, moves the cache, gives
    /// the Connect device Sirensong's default name in place of Spotifast's,
    /// finishes what an earlier attempt left half done, and never runs
    /// again once it has finished.
    #[test]
    fn the_first_start_as_sirensong_brings_over_spotifasts_folders() {
        let root =
            std::env::temp_dir().join(format!("sirensong-migration-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folders = |name: &str| AppDirs {
            config: root.join(name).join("config"),
            state: root.join(name).join("data"),
            cache: root.join(name).join("cache"),
        };
        let (old, new) = (folders("spotifast"), folders("sirensong"));
        for (path, text) in [
            // Omarchy's hook, run before the first start.
            (new.config.join("themes").join("omarchy.json"), "{}"),
            (
                old.config.join("settings.json"),
                "{\"device_name\":\"Spotifast\",\"volume\":37}",
            ),
            (old.config.join("themes").join("mine.json"), "{}"),
            (old.state.join("history.json"), "[]"),
            (old.state.join("instance"), "lock"),
            (old.state.join("spotifast.log"), "log"),
            (old.cache.join("art").join("cover"), "jpeg"),
            // The new copy's own lock, made before its folders are found.
            (new.state.join("instance"), "lock"),
            // Left by an attempt that stopped part way, and a start since.
            (new.state.join(MIGRATING), ""),
            (new.config.join("settings.json"), "{}"),
            (new.state.join("history.json"), "[\"stale\"]"),
        ] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let profile = format!(
            "{:x}",
            <sha2::Sha256 as sha2::Digest>::digest(old.state.to_string_lossy().as_bytes())
        );

        let lines = migrate_folders(&old, &new);
        assert_eq!(lines.len(), 3, "{lines:?}");
        let settings: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(new.config.join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["device_name"], "Sirensong");
        assert_eq!(settings["volume"], 37);
        assert!(new.config.join("themes").join("mine.json").is_file());
        assert_eq!(
            std::fs::read_to_string(new.state.join("history.json")).unwrap(),
            "[]",
            "what a half-done attempt left is replaced"
        );
        assert_eq!(
            std::fs::read_to_string(new.state.join("credential-profile")).unwrap(),
            profile,
            "the sign-in follows the copy"
        );
        assert_eq!(
            std::fs::read_to_string(new.state.join("instance")).unwrap(),
            "lock"
        );
        assert!(!new.state.join("spotifast.log").exists());
        assert!(new.cache.join("art").join("cover").is_file(), "moved");
        assert!(!old.cache.exists());
        assert!(
            old.config.join("settings.json").is_file(),
            "the old copy keeps working"
        );
        assert!(!root.join("sirensong").join("data.partial").exists());
        assert!(!root.join("sirensong").join("config.partial").exists());
        assert!(!new.state.join(MIGRATING).exists());

        std::fs::write(new.config.join("settings.json"), "{\"changed\":1}").unwrap();
        assert!(migrate_folders(&old, &new).is_empty(), "only once");
        assert_eq!(
            std::fs::read_to_string(new.config.join("settings.json")).unwrap(),
            "{\"changed\":1}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Sirensong's own folders are never replaced: a first start with no
    /// Spotifast notes that, and Spotifast's folders appearing later, or
    /// found beside Sirensong's own data, bring nothing over.
    #[test]
    fn sirensongs_own_folders_are_never_replaced() {
        let root = std::env::temp_dir().join(format!(
            "sirensong-migration-own-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let folders = |name: &str| AppDirs {
            config: root.join(name).join("config"),
            state: root.join(name).join("data"),
            cache: root.join(name).join("cache"),
        };
        let (old, new) = (folders("spotifast"), folders("sirensong"));
        assert!(migrate_folders(&old, &new).is_empty());
        assert!(new.state.join(MIGRATED).is_file(), "nothing to bring over");
        std::fs::create_dir_all(&old.config).unwrap();
        std::fs::write(old.config.join("settings.json"), "{\"old\":1}").unwrap();
        std::fs::create_dir_all(&new.config).unwrap();
        std::fs::write(new.config.join("settings.json"), "{\"mine\":1}").unwrap();
        assert!(migrate_folders(&old, &new).is_empty(), "appeared later");

        std::fs::remove_file(new.state.join(MIGRATED)).unwrap();
        assert!(
            migrate_folders(&old, &new).is_empty(),
            "beside its own data"
        );
        assert!(new.state.join(MIGRATED).is_file());
        assert_eq!(
            std::fs::read_to_string(new.config.join("settings.json")).unwrap(),
            "{\"mine\":1}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// macOS keeps settings and state in one folder: it is brought over
    /// once, without a warning, and a name of your own stays.
    #[test]
    fn one_folder_for_settings_and_state_is_brought_over_once() {
        let root = std::env::temp_dir().join(format!(
            "sirensong-migration-shared-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let folders = |name: &str| AppDirs {
            config: root.join(name).join("support"),
            state: root.join(name).join("support"),
            cache: root.join(name).join("cache"),
        };
        let (old, new) = (folders("spotifast"), folders("sirensong"));
        std::fs::create_dir_all(&old.config).unwrap();
        let mine = "{\"device_name\":\"Thalia Spotifast\"}";
        std::fs::write(old.config.join("settings.json"), mine).unwrap();
        let lines = migrate_folders(&old, &new);
        assert!(
            lines.iter().all(|(level, _)| *level == log::Level::Info),
            "{lines:?}"
        );
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert_eq!(
            std::fs::read_to_string(new.config.join("settings.json")).unwrap(),
            mine
        );
        assert!(new.state.join(MIGRATED).is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_log_says_which_folders_a_portable_copy_uses_and_why() {
        let installed = AppDirs::from(fastframe_dirs::Dirs::inside(Path::new("/user")));
        assert!(describe(&installed, &fastframe_dirs::Mode::Installed).is_none());
        let folder = PathBuf::from("/usb/Spotifast/spotifast-data");
        let portable = AppDirs::from(fastframe_dirs::Dirs::inside(&folder));
        let (level, line) = describe(
            &portable,
            &fastframe_dirs::Mode::Portable {
                folder: folder.clone(),
                copied_from: Some(fastframe_dirs::Dirs::inside(Path::new("/user"))),
            },
        )
        .unwrap();
        assert_eq!(level, log::Level::Info);
        assert!(
            line.contains("spotifast-data") && line.contains("/user"),
            "{line}"
        );
        let (level, line) = describe(
            &installed,
            &fastframe_dirs::Mode::Fallback {
                folder,
                error: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            },
        )
        .unwrap();
        assert_eq!(level, log::Level::Warn);
        assert!(
            line.contains("not usable") && line.contains("/user"),
            "{line}"
        );
    }

    /// Installed copies, and every copy on Linux and macOS, keep the folders
    /// they always had.
    #[test]
    fn the_per_user_folders_are_sirensongs() {
        let Some(project) = directories::ProjectDirs::from("me", "paolino", "sirensong") else {
            return;
        };
        let dirs = AppDirs::installed();
        assert_eq!(dirs.config, project.config_dir());
        assert_eq!(
            dirs.state,
            project.state_dir().unwrap_or(project.data_local_dir())
        );
        assert_eq!(dirs.cache, project.cache_dir());
    }
}
