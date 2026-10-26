//! The equalizer presets saved in Spotify's desktop app on this computer.
//!
//! Spotify's app keeps the presets its users name and save in its own
//! browser storage, a LevelDB database, under `{user}:eq_custom_presets`:
//! a JSON list of names, each with the six filters of its equalizer.
//! Nothing of them is on the account, so presets made on a phone or on
//! another computer are not here. The database is only read, and only
//! when asked; Spotify's app may hold it open meanwhile, which a reader
//! of its files does not disturb.

use std::path::{Path, PathBuf};

use crate::eq::ForeignFilter;
use crate::settings::UserEqPreset;

/// The page origin Spotify's app stores its settings under.
const ORIGIN: &[u8] = b"_https://xpui.app.spotify.com\0\x01";
/// The largest file or block this reads, well past any settings store.
const MAX_BYTES: usize = 64 << 20;

/// Why no presets came from Spotify's app.
#[derive(Debug, PartialEq)]
pub enum ImportError {
    /// Spotify's desktop app keeps no storage on this computer.
    NoApp,
    /// It does, and holds no saved preset for this account.
    NoPresets,
    /// The storage or the presets in it could not be read.
    Unreadable(String),
}

/// The presets `user_id` saved in Spotify's app on this computer, as this
/// equalizer's sliders, in the app's order.
pub fn read_presets(user_id: &str) -> Result<Vec<UserEqPreset>, ImportError> {
    let dirs: Vec<PathBuf> = storage_dirs()
        .into_iter()
        .filter(|dir| dir.is_dir())
        .collect();
    if dirs.is_empty() {
        return Err(ImportError::NoApp);
    }
    let mut key = ORIGIN.to_vec();
    key.extend_from_slice(user_id.as_bytes());
    key.extend_from_slice(b":eq_custom_presets");
    let mut unreadable = None;
    for dir in dirs {
        match latest_value(&dir, &key) {
            Ok(Some(value)) => {
                let presets = parse_presets(&value).map_err(ImportError::Unreadable)?;
                return if presets.is_empty() {
                    Err(ImportError::NoPresets)
                } else {
                    Ok(presets)
                };
            }
            Ok(None) => {}
            Err(error) => unreadable = Some(error),
        }
    }
    Err(unreadable.map_or(ImportError::NoPresets, ImportError::Unreadable))
}

/// Where Spotify's desktop app keeps its browser storage, newest profile
/// first. Only the Windows places have been seen; the others are where
/// the app keeps its other files on those systems.
fn storage_dirs() -> Vec<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let mut apps: Vec<PathBuf> = Vec::new();
    #[cfg(target_os = "windows")]
    if let Some(local) = var("LOCALAPPDATA") {
        apps.push(local.join("Spotify"));
        for kept in ["LocalCache", "LocalState"] {
            apps.push(
                local
                    .join("Packages")
                    .join("SpotifyAB.SpotifyMusic_zpdnekdrzrea0")
                    .join(kept)
                    .join("Spotify"),
            );
        }
    }
    #[cfg(target_os = "macos")]
    if let Some(home) = var("HOME") {
        let library = home.join("Library");
        apps.push(library.join("Caches").join("com.spotify.client"));
        apps.push(library.join("Application Support").join("Spotify"));
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let home = var("HOME");
        let cache = var("XDG_CACHE_HOME").or_else(|| home.as_ref().map(|home| home.join(".cache")));
        let config =
            var("XDG_CONFIG_HOME").or_else(|| home.as_ref().map(|home| home.join(".config")));
        apps.extend(cache.map(|cache| cache.join("spotify")));
        apps.extend(config.map(|config| config.join("spotify")));
    }
    apps.iter()
        .flat_map(|app| {
            ["Browser", "Default"]
                .map(|profile| app.join(profile).join("Local Storage").join("leveldb"))
        })
        .collect()
}

/// The newest value the database in `dir` holds for `key`, from its tables
/// and its logs; `None` when it holds none or deleted it last.
fn latest_value(dir: &Path, key: &[u8]) -> Result<Option<Vec<u8>>, String> {
    let mut newest: Option<Record> = None;
    let entries = std::fs::read_dir(dir).map_err(|error| error.to_string())?;
    for entry in entries.flatten() {
        let path = entry.path();
        let kind = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        if !matches!(kind, "ldb" | "sst" | "log") {
            continue;
        }
        if entry
            .metadata()
            .is_ok_and(|meta| meta.len() > MAX_BYTES as u64)
        {
            continue;
        }
        let Ok(data) = std::fs::read(&path) else {
            continue;
        };
        // A damaged file is skipped: the others may still hold the key.
        let found = if kind == "log" {
            log_records(&data, key)
        } else {
            table_records(&data, key)
        };
        match found {
            Ok(Some((sequence, value))) => {
                if newest.as_ref().is_none_or(|(held, _)| sequence > *held) {
                    newest = Some((sequence, value));
                }
            }
            Ok(None) => {}
            Err(error) => log::warn!("Skipping {}: {error}", path.display()),
        }
    }
    Ok(newest.and_then(|(_, value)| value))
}

/// Reads the presets JSON Chromium stored: a marker byte, then the text
/// as Latin-1 (1) or UTF-16 (0).
pub(crate) fn parse_presets(value: &[u8]) -> Result<Vec<UserEqPreset>, String> {
    let text = match value.split_first() {
        Some((1, rest)) => rest
            .iter()
            .map(|&byte| char::from(byte))
            .collect::<String>(),
        Some((0, rest)) => {
            let units: Vec<u16> = rest
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| u16::from_le_bytes(*pair))
                .collect();
            String::from_utf16(&units).map_err(|error| error.to_string())?
        }
        _ => return Err("unknown text encoding".into()),
    };
    #[derive(serde::Deserialize)]
    struct Preset {
        name: String,
        #[serde(default)]
        filters: Vec<Filter>,
    }
    #[derive(serde::Deserialize)]
    struct Filter {
        frequency: f32,
        #[serde(rename = "type")]
        kind: String,
        #[serde(default)]
        gain: f32,
    }
    let presets: Vec<Preset> = serde_json::from_str(&text).map_err(|error| error.to_string())?;
    Ok(presets
        .into_iter()
        .filter(|preset| !preset.name.trim().is_empty())
        .map(|preset| {
            let filters: Vec<ForeignFilter> = preset
                .filters
                .iter()
                .filter(|filter| filter.frequency.is_finite() && filter.frequency > 0.0)
                .filter_map(|filter| {
                    let gain_db = if filter.gain.is_finite() {
                        filter.gain.clamp(-crate::eq::RANGE_DB, crate::eq::RANGE_DB)
                    } else {
                        0.0
                    };
                    let hz = filter.frequency;
                    match filter.kind.as_str() {
                        "lowshelf" => Some(ForeignFilter::LowShelf { hz, gain_db }),
                        "highshelf" => Some(ForeignFilter::HighShelf { hz, gain_db }),
                        // Web Audio's own default width, which Spotify's
                        // app does not change.
                        "peaking" => Some(ForeignFilter::Peaking {
                            hz,
                            q: 1.0,
                            gain_db,
                        }),
                        _ => None,
                    }
                })
                .collect();
            UserEqPreset {
                name: preset.name.trim().to_string(),
                preamp_db: 0.0,
                bands_db: crate::eq::sliders_for(&filters),
            }
        })
        .collect())
}

/// A sequence number and the value written then, or `None` for a deletion.
type Record = (u64, Option<Vec<u8>>);

/// The most a table may decompress, all its blocks together: far past any
/// settings store, and a bound on what a damaged or crafted one can cost.
const MAX_DECOMPRESSED: usize = 16 << 20;
/// The most data blocks a table's index may name; a 64 MiB table of 4 KiB
/// blocks has 16,384.
const MAX_BLOCKS: usize = 1 << 16;

/// Keeps `sequence` and its value when it is newer than what `held` has.
fn keep_newer(held: &mut Option<Record>, sequence: u64, value: Option<&[u8]>) {
    if held.as_ref().is_none_or(|(at, _)| sequence > *at) {
        *held = Some((sequence, value.map(<[u8]>::to_vec)));
    }
}

/// Bytes read in order, every read checked against the end.
struct Cursor<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, at: 0 }
    }

    fn done(&self) -> bool {
        self.at >= self.data.len()
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.data.len());
        let end = end.ok_or("truncated")?;
        let taken = &self.data[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn byte(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn varint(&mut self) -> Result<u64, String> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.byte()?;
            value |= u64::from(byte & 0x7f) << shift;
            if byte < 0x80 {
                return Ok(value);
            }
        }
        Err("varint too long".into())
    }

    fn length(&mut self) -> Result<usize, String> {
        usize::try_from(self.varint()?)
            .ok()
            .filter(|length| *length <= MAX_BYTES)
            .ok_or_else(|| "length too large".into())
    }
}

/// The newest record for `key` in a LevelDB write-ahead log: 32 KiB
/// blocks of fragments that join into write batches. Fragments with no
/// first one before them belong to a write the log no longer holds.
fn log_records(data: &[u8], key: &[u8]) -> Result<Option<Record>, String> {
    const BLOCK: usize = 32 * 1024;
    const HEADER: usize = 7;
    let mut found = None;
    let mut batch: Option<Vec<u8>> = None;
    let mut at = 0;
    while at + HEADER <= data.len() {
        let left = BLOCK - at % BLOCK;
        if left < HEADER {
            at += left;
            continue;
        }
        let length = usize::from(u16::from_le_bytes([data[at + 4], data[at + 5]]));
        let kind = data[at + 6];
        let start = at + HEADER;
        let Some(fragment) = data.get(start..start + length) else {
            // A write cut short at the end of the log.
            break;
        };
        at = start + length;
        match kind {
            // Zero-filled space a writer left at the end of a block.
            0 => {}
            1 => {
                batch = None;
                batch_records(fragment, key, &mut found)?;
            }
            2 => batch = Some(fragment.to_vec()),
            3 => {
                if let Some(batch) = &mut batch {
                    batch.extend_from_slice(fragment);
                }
            }
            4 => {
                if let Some(mut whole) = batch.take() {
                    whole.extend_from_slice(fragment);
                    batch_records(&whole, key, &mut found)?;
                }
            }
            _ => return Err(format!("unknown log record type {kind}")),
        }
    }
    Ok(found)
}

fn batch_records(batch: &[u8], key: &[u8], found: &mut Option<Record>) -> Result<(), String> {
    let mut cursor = Cursor::new(batch);
    let sequence = u64::from_le_bytes(cursor.take(8)?.try_into().map_err(|_| "batch")?);
    let count = u32::from_le_bytes(cursor.take(4)?.try_into().map_err(|_| "batch")?);
    for index in 0..u64::from(count) {
        let tag = cursor.byte()?;
        let length = cursor.length()?;
        let held = cursor.take(length)?;
        let value = match tag {
            1 => {
                let length = cursor.length()?;
                Some(cursor.take(length)?)
            }
            0 => None,
            _ => return Err(format!("unknown batch entry {tag}")),
        };
        if held == key {
            let sequence = sequence.checked_add(index).ok_or("sequence past the end")?;
            keep_newer(found, sequence, value);
        }
    }
    Ok(())
}

/// The newest record for `key` in a LevelDB table: its index names the
/// data blocks, each a run of prefix-shared keys, plain or
/// Snappy-compressed. Each block is read once, and all of them together
/// decompress to no more than `MAX_DECOMPRESSED`.
fn table_records(data: &[u8], key: &[u8]) -> Result<Option<Record>, String> {
    const FOOTER: usize = 48;
    const MAGIC: u64 = 0xdb47_7524_8b80_fb57;
    if data.len() < FOOTER {
        return Err("too short for a table".into());
    }
    let footer = &data[data.len() - FOOTER..];
    let magic = u64::from_le_bytes(footer[40..].try_into().map_err(|_| "footer")?);
    if magic != MAGIC {
        return Err("not a table".into());
    }
    let mut budget = MAX_DECOMPRESSED;
    let mut cursor = Cursor::new(&footer[..40]);
    let _meta = (cursor.varint()?, cursor.varint()?);
    let index = block(data, cursor.varint()?, cursor.varint()?, &mut budget)?;
    let mut handles: Vec<(u64, u64)> = Vec::new();
    each_entry(&index, |_, handle| {
        if handles.len() == MAX_BLOCKS {
            return Err("too many blocks".into());
        }
        let mut handle = Cursor::new(handle);
        handles.push((handle.varint()?, handle.varint()?));
        Ok(())
    })?;
    handles.sort_unstable();
    handles.dedup();
    let mut found = None;
    for (offset, size) in handles {
        let contents = block(data, offset, size, &mut budget)?;
        each_entry(&contents, |internal, value| {
            if internal.len() < 8 {
                return Err("short key".into());
            }
            let (held, trailer) = internal.split_at(internal.len() - 8);
            if held == key {
                let trailer = u64::from_le_bytes(trailer.try_into().map_err(|_| "key")?);
                keep_newer(
                    &mut found,
                    trailer >> 8,
                    (trailer & 0xff == 1).then_some(value),
                );
            }
            Ok(())
        })?;
    }
    Ok(found)
}

/// One block of a table, decompressed, by its handle, taken from `budget`.
fn block(data: &[u8], offset: u64, size: u64, budget: &mut usize) -> Result<Vec<u8>, String> {
    let offset = usize::try_from(offset).map_err(|_| "offset")?;
    let size = usize::try_from(size).map_err(|_| "size")?;
    let end = offset
        .checked_add(size)
        .filter(|end| *end < data.len())
        .ok_or("block past the end")?;
    let contents = &data[offset..end];
    let block = match data[end] {
        0 => {
            if size > *budget {
                return Err("blocks too large".into());
            }
            contents.to_vec()
        }
        1 => snappy(contents, *budget)?,
        kind => return Err(format!("unknown compression {kind}")),
    };
    *budget -= block.len();
    Ok(block)
}

/// Hands each key and value of a block to `each`, the key completed from
/// the one before in one buffer, so a block costs no more than itself.
fn each_entry(
    block: &[u8],
    mut each: impl FnMut(&[u8], &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    if block.len() < 4 {
        return Err("short block".into());
    }
    let restarts = u32::from_le_bytes(block[block.len() - 4..].try_into().map_err(|_| "block")?);
    let restarts = usize::try_from(restarts).map_err(|_| "restarts")?;
    let end = restarts
        .checked_mul(4)
        .and_then(|bytes| (block.len() - 4).checked_sub(bytes))
        .ok_or("restarts past the block")?;
    let mut cursor = Cursor::new(&block[..end]);
    let mut key: Vec<u8> = Vec::new();
    while !cursor.done() {
        let shared = cursor.length()?;
        let unshared = cursor.length()?;
        let length = cursor.length()?;
        if shared > key.len() {
            return Err("shared prefix past the key".into());
        }
        key.truncate(shared);
        key.extend_from_slice(cursor.take(unshared)?);
        each(&key, cursor.take(length)?)?;
    }
    Ok(())
}

/// Decompresses a Snappy block of no more than `limit` bytes: literals
/// and copies of what came before.
fn snappy(data: &[u8], limit: usize) -> Result<Vec<u8>, String> {
    let mut cursor = Cursor::new(data);
    let expected = cursor.length()?;
    if expected > limit {
        return Err("blocks too large".into());
    }
    let mut out: Vec<u8> = Vec::with_capacity(expected);
    while !cursor.done() {
        let tag = cursor.byte()?;
        let (length, offset) = match tag & 3 {
            0 => {
                let mut length = usize::from(tag >> 2);
                if length >= 60 {
                    let bytes = cursor.take(length - 59)?;
                    length = bytes
                        .iter()
                        .rev()
                        .fold(0usize, |value, byte| (value << 8) | usize::from(*byte));
                }
                let literal = cursor.take(length + 1)?;
                out.extend_from_slice(literal);
                continue;
            }
            1 => {
                let low = usize::from(cursor.byte()?);
                (
                    usize::from((tag >> 2) & 7) + 4,
                    (usize::from(tag >> 5) << 8) | low,
                )
            }
            2 => {
                let bytes = cursor.take(2)?;
                (
                    usize::from(tag >> 2) + 1,
                    usize::from(u16::from_le_bytes([bytes[0], bytes[1]])),
                )
            }
            _ => {
                let bytes = cursor.take(4)?;
                let offset = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                (
                    usize::from(tag >> 2) + 1,
                    usize::try_from(offset).map_err(|_| "offset")?,
                )
            }
        };
        if offset == 0 || offset > out.len() {
            return Err("copy before the start".into());
        }
        if out.len() + length > expected {
            return Err("longer than it said".into());
        }
        let from = out.len() - offset;
        for index in 0..length {
            out.push(out[from + index]);
        }
    }
    if out.len() != expected {
        return Err("shorter than it said".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests;
