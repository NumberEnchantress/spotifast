//! Playlist folders, edited on the account itself.
//!
//! Spotify keeps folders in the account's rootlist, the ordered list of its
//! playlists, as a `spotify:start-group:{id}:{name}` row and a
//! `spotify:end-group:{id}` row around what each folder holds. The Web API
//! has no folders, so these edits go to the rootlist's `changes` endpoint
//! over the playback session, shaped the way Spotify's web player shapes
//! them:
//!
//! - a new folder is its two markers, added before a row, at an end, or
//!   just after another folder's start marker, which puts it first inside;
//! - a rename adds a start marker with the new name, through a temporary
//!   one, so no step ever holds two markers with the same id;
//! - a playlist moves by its URI, to the same places;
//! - a folder moves as one index range against the revision it was read
//!   at, once its markers are seen to bracket that range and every page of
//!   the read is seen to come from that one revision;
//! - removing a folder takes out its two markers by their URIs. What it
//!   held stays in the library, one level up. Spotify's own apps take the
//!   playlists out along with the folder; a removal named by URI can only
//!   ever match a folder marker, so this never takes a playlist out.
//!
//! The ops and anchors are those of the web player, with one exception:
//! removing a folder's end marker by URI, which the web player does by
//! index along with everything inside. The one op that counts rows, a
//! folder's move, never removes a row. A new folder that takes a playlist
//! in goes as two edits, as the web player sends it: the folder, then the
//! move into it, planned against a read that holds the folder.
//!
//! [`apply`] makes an edit to the rows the sidebar shows, so the sidebar
//! changes the moment the user acts. [`plan`] turns the same edit into the
//! request, against a fresh read of the rootlist, and [`send`] sends it
//! once and reads the tree back.

use std::fmt::Write as _;

use librespot_protocol::playlist4_external as pb;

use crate::player::{Rootlist, RootlistEntry};

/// A row of the rootlist an edit can name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A playlist, by its URI.
    Playlist(String),
    /// A folder, by its rootlist id.
    Folder(String),
}

/// Where a created or moved row goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Place {
    /// The top of the library.
    Start,
    /// Just before this row, in the folder that row is in.
    Before(Row),
    /// The first row inside this folder, as Spotify's "move to folder"
    /// puts it.
    InFolder(String),
    /// Just after this playlist, in the folder it is in: the end of a
    /// folder whose last row is a playlist.
    AfterPlaylist(String),
    /// The bottom of the library.
    End,
}

/// One change to the account's folders.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderEdit {
    /// A new, empty folder at `place`, with `adopt` moved straight into it.
    Create {
        id: String,
        name: String,
        place: Place,
        adopt: Option<String>,
    },
    Rename {
        id: String,
        name: String,
    },
    /// The folder goes; what it held stays where it was, one level up.
    Remove {
        id: String,
    },
    MovePlaylist {
        uri: String,
        place: Place,
    },
    /// The folder moves with everything inside it.
    MoveFolder {
        id: String,
        place: Place,
    },
}

/// Why an edit was not made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The edit would leave every row where it is.
    Unchanged,
    /// A row the edit names is not in the rootlist, or no longer is.
    Missing,
    /// A folder cannot go inside itself.
    IntoItself,
    /// The rootlist's markers do not pair up around this folder, or its id
    /// is already taken.
    Malformed,
    /// A folder needs a name.
    NoName,
}

impl Refusal {
    /// What the user is told; `Unchanged` is never shown.
    pub fn message(self, locale: crate::i18n::Locale) -> std::borrow::Cow<'static, str> {
        use crate::i18n::gettext;
        match self {
            Self::Unchanged => std::borrow::Cow::Borrowed(""),
            Self::Missing => gettext(
                locale,
                "That folder or playlist has changed on Spotify. Try again.",
            ),
            Self::IntoItself => gettext(locale, "A folder can't go inside itself."),
            Self::Malformed => gettext(
                locale,
                "Spotify's copy of this folder looks damaged, so Spotifast left it alone.",
            ),
            Self::NoName => gettext(locale, "A folder needs a name."),
        }
    }
}

/// A fresh folder id: eight random bytes in hex, as Spotify's apps make them.
pub fn new_id() -> String {
    hex(&rand::random::<[u8; 8]>())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// The name as it will read back from Spotify: runs of white space become
/// one space, and the ends are trimmed. `None` when nothing is left.
pub fn folder_name(name: &str) -> Option<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    (!name.is_empty()).then_some(name)
}

/// The name as it sits in a start marker: `encodeURIComponent`, then `+`
/// for a space, as Spotify's web player writes it. The marker's own colons
/// therefore never appear inside a name.
fn encode_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for byte in name.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            b' ' => out.push('+'),
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// A folder's start marker; without a name it is the form Spotify matches
/// a folder by.
pub fn start_marker(id: &str, name: Option<&str>) -> String {
    match name {
        Some(name) => format!("spotify:start-group:{id}:{}", encode_name(name)),
        None => format!("spotify:start-group:{id}"),
    }
}

pub fn end_marker(id: &str) -> String {
    format!("spotify:end-group:{id}")
}

/// The folder id of a start marker.
pub fn start_id(uri: &str) -> Option<&str> {
    let rest = uri.strip_prefix("spotify:start-group:")?;
    Some(rest.split_once(':').map_or(rest, |(id, _)| id))
}

fn end_id(uri: &str) -> Option<&str> {
    uri.strip_prefix("spotify:end-group:")
}

// ---- the rows the sidebar shows ---------------------------------------------

/// The rows from a folder's start to its end, both included.
pub fn folder_span(rows: &[RootlistEntry], id: &str) -> Option<(usize, usize)> {
    let start = rows.iter().position(
        |row| matches!(row, RootlistEntry::FolderStart { id: found, .. } if found == id),
    )?;
    let mut depth = 0usize;
    for (offset, row) in rows[start..].iter().enumerate() {
        match row {
            RootlistEntry::FolderStart { .. } => depth += 1,
            RootlistEntry::FolderEnd => {
                depth -= 1;
                if depth == 0 {
                    return Some((start, start + offset));
                }
            }
            RootlistEntry::Playlist(_) => {}
        }
    }
    None
}

fn playlist_at(rows: &[RootlistEntry], uri: &str) -> Option<usize> {
    rows.iter()
        .position(|row| matches!(row, RootlistEntry::Playlist(held) if held == uri))
}

/// The folder a row sits directly inside, by id; `None` at the top level.
pub fn parent(rows: &[RootlistEntry], row: &Row) -> Option<String> {
    let at = match row {
        Row::Playlist(uri) => playlist_at(rows, uri)?,
        Row::Folder(id) => folder_span(rows, id)?.0,
    };
    let mut open: Vec<&str> = Vec::new();
    for row in &rows[..at] {
        match row {
            RootlistEntry::FolderStart { id, .. } => open.push(id),
            RootlistEntry::FolderEnd => {
                open.pop();
            }
            RootlistEntry::Playlist(_) => {}
        }
    }
    open.last().map(|id| (*id).to_string())
}

/// A folder of the tree, as a menu lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub id: String,
    pub name: String,
    /// How many folders it sits inside.
    pub depth: usize,
}

/// Every folder with how deep it sits, in rootlist order.
pub fn folders(rows: &[RootlistEntry]) -> Vec<Folder> {
    let mut depth = 0usize;
    let mut found = Vec::new();
    for row in rows {
        match row {
            RootlistEntry::FolderStart { id, name } => {
                found.push(Folder {
                    id: id.clone(),
                    name: name.clone(),
                    depth,
                });
                depth += 1;
            }
            RootlistEntry::FolderEnd => depth = depth.saturating_sub(1),
            RootlistEntry::Playlist(_) => {}
        }
    }
    found
}

/// Whether `inner` is `outer` or sits anywhere inside it.
pub fn within(rows: &[RootlistEntry], inner: &str, outer: &str) -> bool {
    let (Some((start, end)), Some((at, _))) = (folder_span(rows, outer), folder_span(rows, inner))
    else {
        return false;
    };
    (start..=end).contains(&at)
}

/// Where a row taken out of its folder goes: just above that folder, one
/// level up. `None` for a row at the top level.
pub fn out_of_folder(rows: &[RootlistEntry], row: &Row) -> Option<Place> {
    parent(rows, row).map(|id| Place::Before(Row::Folder(id)))
}

/// A folder's span, refused when the folder is missing or its id is held
/// by more than one folder: Spotify finds rows by id, so which one it would
/// pick cannot be known.
fn checked_span(rows: &[RootlistEntry], id: &str) -> Result<(usize, usize), Refusal> {
    let holders = rows
        .iter()
        .filter(|row| matches!(row, RootlistEntry::FolderStart { id: found, .. } if found == id))
        .count();
    if holders > 1 {
        return Err(Refusal::Malformed);
    }
    folder_span(rows, id).ok_or(Refusal::Missing)
}

fn place_at(rows: &[RootlistEntry], place: &Place) -> Result<usize, Refusal> {
    match place {
        Place::Start => Ok(0),
        Place::End => Ok(rows.len()),
        Place::Before(Row::Playlist(uri)) => playlist_at(rows, uri).ok_or(Refusal::Missing),
        Place::AfterPlaylist(uri) => playlist_at(rows, uri)
            .map(|at| at + 1)
            .ok_or(Refusal::Missing),
        Place::Before(Row::Folder(id)) => Ok(checked_span(rows, id)?.0),
        Place::InFolder(id) => Ok(checked_span(rows, id)?.0 + 1),
    }
}

/// The rows after `edit`, as Spotify will hold them once it is made.
pub fn apply(rows: &[RootlistEntry], edit: &FolderEdit) -> Result<Vec<RootlistEntry>, Refusal> {
    let mut rows = rows.to_vec();
    match edit {
        FolderEdit::Create {
            id,
            name,
            place,
            adopt,
        } => {
            let name = folder_name(name).ok_or(Refusal::NoName)?;
            if rows.iter().any(
                |row| matches!(row, RootlistEntry::FolderStart { id: found, .. } if found == id),
            ) {
                return Err(Refusal::Malformed);
            }
            if adopt
                .as_ref()
                .is_some_and(|uri| playlist_at(&rows, uri).is_none())
            {
                return Err(Refusal::Missing);
            }
            let at = place_at(&rows, place)?;
            rows.splice(
                at..at,
                [
                    RootlistEntry::FolderStart {
                        id: id.clone(),
                        name,
                    },
                    RootlistEntry::FolderEnd,
                ],
            );
            if let Some(uri) = adopt {
                let from = playlist_at(&rows, uri).ok_or(Refusal::Missing)?;
                let row = rows.remove(from);
                let (start, _) = checked_span(&rows, id)?;
                rows.insert(start + 1, row);
            }
        }
        FolderEdit::Rename { id, name } => {
            let name = folder_name(name).ok_or(Refusal::NoName)?;
            let (start, _) = checked_span(&rows, id)?;
            if let RootlistEntry::FolderStart { name: held, .. } = &mut rows[start] {
                if *held == name {
                    return Err(Refusal::Unchanged);
                }
                *held = name;
            }
        }
        FolderEdit::Remove { id } => {
            let (start, end) = checked_span(&rows, id)?;
            rows.remove(end);
            rows.remove(start);
        }
        FolderEdit::MovePlaylist { uri, place } => {
            if *place == Place::Before(Row::Playlist(uri.clone()))
                || *place == Place::AfterPlaylist(uri.clone())
            {
                return Err(Refusal::Unchanged);
            }
            let from = playlist_at(&rows, uri).ok_or(Refusal::Missing)?;
            let row = rows.remove(from);
            let at = place_at(&rows, place)?;
            if at == from {
                return Err(Refusal::Unchanged);
            }
            rows.insert(at, row);
        }
        FolderEdit::MoveFolder { id, place } => {
            let (start, end) = checked_span(&rows, id)?;
            let to = place_at(&rows, place)?;
            if to > start && to <= end {
                return Err(Refusal::IntoItself);
            }
            if to == start || to == end + 1 {
                return Err(Refusal::Unchanged);
            }
            let span: Vec<_> = rows.drain(start..=end).collect();
            let at = if to > end { to - span.len() } else { to };
            rows.splice(at..at, span);
        }
    }
    Ok(rows)
}

// ---- the request ------------------------------------------------------------

/// The rootlist as Spotify keeps it: every row's URI, folder markers and
/// all, and the revision they were read at.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RawRootlist {
    pub uris: Vec<String>,
    /// Empty when the read came in pages that did not all carry the same
    /// revision: rows counted across them could be off by any amount, so
    /// nothing that counts rows is planned against such a read.
    pub revision: Vec<u8>,
}

/// The revision a paged read was made at: the one every page carries, or
/// empty when any page carries another or none.
pub fn one_revision(pages: Vec<Option<Vec<u8>>>) -> Vec<u8> {
    let mut pages = pages.into_iter();
    let Some(Some(first)) = pages.next() else {
        return Vec::new();
    };
    if pages.all(|page| page.as_ref() == Some(&first)) {
        first
    } else {
        Vec::new()
    }
}

/// The counter that leads a revision; it grows with every change.
pub fn revision_counter(revision: &[u8]) -> Option<u32> {
    let bytes: [u8; 4] = revision.get(..4)?.try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

/// Where an added or moved item lands, by what Spotify's ops can say.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Location {
    First,
    Last,
    Before(String),
    /// Just after this item: after a folder's start marker is first inside
    /// the folder, as the web player's "move to folder" sends it.
    After(String),
}

impl RawRootlist {
    fn find(&self, uri: &str) -> Option<usize> {
        self.uris.iter().position(|held| held == uri)
    }

    fn start(&self, id: &str) -> Option<usize> {
        self.uris.iter().position(|uri| start_id(uri) == Some(id))
    }

    /// A folder's markers, checked to pair up: each is the only one with
    /// its id, everything between them opens and closes inside, and the
    /// end marker carries the same id. Spotify finds markers by id, so a
    /// doubled id would leave which one it picks unknown.
    fn span(&self, id: &str) -> Result<(usize, usize), Refusal> {
        let start = self.start(id).ok_or(Refusal::Missing)?;
        let starts = self.uris.iter().filter(|uri| start_id(uri) == Some(id));
        let ends = self.uris.iter().filter(|uri| end_id(uri) == Some(id));
        if starts.count() != 1 || ends.count() != 1 {
            return Err(Refusal::Malformed);
        }
        let mut open: Vec<&str> = Vec::new();
        for (at, uri) in self.uris.iter().enumerate().skip(start) {
            if let Some(found) = start_id(uri) {
                open.push(found);
            } else if let Some(found) = end_id(uri) {
                if open.pop() != Some(found) {
                    return Err(Refusal::Malformed);
                }
                if open.is_empty() {
                    return Ok((start, at));
                }
            }
        }
        Err(Refusal::Malformed)
    }

    fn location(&self, place: &Place) -> Result<Location, Refusal> {
        Ok(match place {
            Place::Start => Location::First,
            Place::End => Location::Last,
            Place::Before(Row::Playlist(uri)) => {
                self.find(uri).ok_or(Refusal::Missing)?;
                Location::Before(uri.clone())
            }
            Place::AfterPlaylist(uri) => {
                self.find(uri).ok_or(Refusal::Missing)?;
                Location::After(uri.clone())
            }
            Place::Before(Row::Folder(id)) => {
                self.span(id)?;
                Location::Before(start_marker(id, None))
            }
            Place::InFolder(id) => {
                self.span(id)?;
                Location::After(start_marker(id, None))
            }
        })
    }

    /// The index a place stands at before anything moves.
    fn index(&self, place: &Place) -> Result<usize, Refusal> {
        match place {
            Place::Start => Ok(0),
            Place::End => Ok(self.uris.len()),
            Place::Before(Row::Playlist(uri)) => self.find(uri).ok_or(Refusal::Missing),
            Place::AfterPlaylist(uri) => self.find(uri).map(|at| at + 1).ok_or(Refusal::Missing),
            Place::Before(Row::Folder(id)) => Ok(self.span(id)?.0),
            Place::InFolder(id) => Ok(self.span(id)?.0 + 1),
        }
    }
}

fn item(uri: &str) -> pb::Item {
    let mut item = pb::Item::new();
    item.set_uri(uri.to_string());
    item
}

/// An item as a move or an anchor names it: the URI and empty attributes.
fn keyed(uri: &str) -> pb::Item {
    let mut item = item(uri);
    item.attributes = protobuf::MessageField::some(pb::ItemAttributes::new());
    item
}

fn added(uri: &str, now_ms: i64) -> pb::Item {
    let mut attributes = pb::ItemAttributes::new();
    attributes.set_timestamp(now_ms);
    let mut item = item(uri);
    item.attributes = protobuf::MessageField::some(attributes);
    item
}

fn add_op(items: Vec<pb::Item>, location: &Location) -> pb::Op {
    let mut add = pb::Add::new();
    match location {
        Location::First => add.set_add_first(true),
        Location::Last => add.set_add_last(true),
        Location::Before(uri) => add.add_before_item = protobuf::MessageField::some(keyed(uri)),
        Location::After(uri) => add.add_after_item = protobuf::MessageField::some(keyed(uri)),
    }
    add.items = items;
    let mut op = pb::Op::new();
    op.set_kind(pb::op::Kind::ADD);
    op.add = protobuf::MessageField::some(add);
    op
}

fn remove_keyed(uri: &str) -> pb::Op {
    let mut rem = pb::Rem::new();
    rem.items = vec![item(uri)];
    rem.set_items_as_key(true);
    let mut op = pb::Op::new();
    op.set_kind(pb::op::Kind::REM);
    op.rem = protobuf::MessageField::some(rem);
    op
}

fn move_keyed(uri: &str, location: &Location) -> pb::Op {
    let mut mov = pb::Mov::new();
    mov.items = vec![keyed(uri)];
    match location {
        Location::First => mov.set_add_first(true),
        Location::Last => mov.set_add_last(true),
        Location::Before(anchor) => {
            mov.add_before_item = protobuf::MessageField::some(keyed(anchor));
        }
        Location::After(anchor) => {
            mov.add_after_item = protobuf::MessageField::some(keyed(anchor));
        }
    }
    let mut op = pb::Op::new();
    op.set_kind(pb::op::Kind::MOV);
    op.mov = protobuf::MessageField::some(mov);
    op
}

fn move_range(from: usize, length: usize, to: usize) -> pb::Op {
    let mut mov = pb::Mov::new();
    mov.set_from_index(from as i32);
    mov.set_length(length as i32);
    mov.set_to_index(to as i32);
    let mut op = pb::Op::new();
    op.set_kind(pb::op::Kind::MOV);
    op.mov = protobuf::MessageField::some(mov);
    op
}

/// The request that makes `edit` on the rootlist as `raw` holds it.
/// `temp_id` names the passing marker a rename goes through; it must not be
/// any folder's id.
pub fn plan(
    raw: &RawRootlist,
    edit: &FolderEdit,
    now_ms: i64,
    temp_id: &str,
) -> Result<pb::ListChanges, Refusal> {
    let mut ops = Vec::new();
    // Ops that count rows by index only hold for the revision they were
    // counted against; ops that name rows by URI hold for any.
    let mut by_index = false;
    match edit {
        FolderEdit::Create {
            id,
            name,
            place,
            adopt,
        } => {
            let name = folder_name(name).ok_or(Refusal::NoName)?;
            if raw.start(id).is_some() || raw.uris.iter().any(|uri| end_id(uri) == Some(id)) {
                return Err(Refusal::Malformed);
            }
            if let Some(uri) = adopt {
                raw.find(uri).ok_or(Refusal::Missing)?;
            }
            let location = raw.location(place)?;
            ops.push(add_op(
                vec![
                    added(&start_marker(id, Some(&name)), now_ms),
                    added(&end_marker(id), now_ms),
                ],
                &location,
            ));
            if let Some(uri) = adopt {
                ops.push(move_keyed(uri, &Location::After(start_marker(id, None))));
            }
        }
        FolderEdit::Rename { id, name } => {
            let name = folder_name(name).ok_or(Refusal::NoName)?;
            let (start, _) = raw.span(id)?;
            if raw.start(temp_id).is_some() {
                return Err(Refusal::Malformed);
            }
            if raw.uris[start] == start_marker(id, Some(&name)) {
                return Err(Refusal::Unchanged);
            }
            let passing = start_marker(temp_id, Some(&name));
            let current = start_marker(id, None);
            ops.push(add_op(
                vec![added(&passing, now_ms)],
                &Location::Before(current.clone()),
            ));
            ops.push(remove_keyed(&current));
            ops.push(add_op(
                vec![added(&start_marker(id, Some(&name)), now_ms)],
                &Location::Before(passing.clone()),
            ));
            ops.push(remove_keyed(&passing));
        }
        FolderEdit::Remove { id } => {
            // Named by URI, as the web player's rename removes a start
            // marker: whatever else has moved meanwhile, these can only
            // match the folder's own markers, never a playlist.
            raw.span(id)?;
            // The end marker first: no web player op removes one by URI, so
            // should Spotify refuse it, it does so before the start marker,
            // whose removal by URI the rename shows, has been touched.
            ops.push(remove_keyed(&end_marker(id)));
            ops.push(remove_keyed(&start_marker(id, None)));
        }
        FolderEdit::MovePlaylist { uri, place } => {
            if *place == Place::Before(Row::Playlist(uri.clone()))
                || *place == Place::AfterPlaylist(uri.clone())
            {
                return Err(Refusal::Unchanged);
            }
            raw.find(uri).ok_or(Refusal::Missing)?;
            let location = raw.location(place)?;
            ops.push(move_keyed(uri, &location));
        }
        FolderEdit::MoveFolder { id, place } => {
            if raw.revision.is_empty() {
                // The read's pages disagree, so its counts cannot be trusted.
                return Err(Refusal::Missing);
            }
            let (start, end) = raw.span(id)?;
            let to = raw.index(place)?;
            if to > start && to <= end {
                return Err(Refusal::IntoItself);
            }
            if to == start || to == end + 1 {
                return Err(Refusal::Unchanged);
            }
            by_index = true;
            ops.push(move_range(start, end - start + 1, to));
        }
    }
    let mut source = pb::SourceInfo::new();
    source.set_client(pb::source_info::Client::CLIENT);
    let mut info = pb::ChangeInfo::new();
    info.source = protobuf::MessageField::some(source);
    let mut delta = pb::Delta::new();
    delta.ops = ops;
    delta.info = protobuf::MessageField::some(info);
    let mut changes = pb::ListChanges::new();
    if by_index {
        changes.set_base_revision(raw.revision.clone());
    }
    changes.deltas = vec![delta];
    changes.set_want_resulting_revisions(false);
    changes.set_want_sync_result(false);
    Ok(changes)
}

// ---- over the wire ----------------------------------------------------------

/// Why an edit did not reach Spotify, or could not be seen to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Spotify's copy of the tree no longer allows the edit; nothing was sent.
    Refused(Refusal),
    /// Sending the edit failed.
    Failed(String),
    /// Spotify took the edit, but reading the tree back afterwards failed.
    Unconfirmed(String),
}

impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(format!("{error:#}"))
    }
}

/// How long to wait between reads that have not yet come out right.
const READ_PAUSE: std::time::Duration = std::time::Duration::from_millis(400);

/// How long the steps of an edit may take. Past `plan` nothing has been
/// sent, so the edit failed; past `confirm` it was sent and only the read
/// back is missing.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub plan: std::time::Duration,
    pub confirm: std::time::Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            plan: std::time::Duration::from_secs(30),
            confirm: std::time::Duration::from_secs(30),
        }
    }
}

/// A folder edit waiting its turn.
pub type Job = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

/// Starts a task that runs jobs one after another, in the order they were
/// queued, so each edit is planned against the tree the one before it left.
/// Called within the Tokio runtime.
pub fn queue() -> tokio::sync::mpsc::UnboundedSender<Job> {
    let (jobs, mut waiting) = tokio::sync::mpsc::unbounded_channel::<Job>();
    tokio::spawn(async move {
        while let Some(job) = waiting.recv().await {
            job.await;
        }
    });
    jobs
}

/// Reads the tree until `done` says a read came out right, at most `tries`
/// times, `pause` apart. The last read stands if none did.
async fn read_until<F, Fut>(
    tries: usize,
    pause: std::time::Duration,
    done: impl Fn(&RawRootlist) -> bool,
    mut read: F,
) -> anyhow::Result<(RawRootlist, std::collections::BTreeSet<String>)>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<
            Output = anyhow::Result<(RawRootlist, std::collections::BTreeSet<String>)>,
        >,
{
    let mut attempt = 0;
    loop {
        let found = read().await?;
        attempt += 1;
        if done(&found.0) || attempt >= tries {
            return Ok(found);
        }
        tokio::time::sleep(pause).await;
    }
}

/// Whether a read made at revision `read` already holds the change Spotify
/// answered with revision `written`. A read whose pages disagree, with no
/// revision of its own, has not.
fn caught_up(written: Option<u32>, read: &[u8]) -> bool {
    match (written, revision_counter(read)) {
        (Some(written), Some(read)) => read >= written,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

/// Makes `edit` on Spotify and returns the tree as Spotify then holds it.
/// The change is planned against a read made just before it whose pages
/// all agree, sent once (never retried, since a repeated add would make a
/// second folder), and read back until the read has caught up with it.
pub async fn send(
    engine: &crate::player::Engine,
    http: &reqwest::Client,
    edit: &FolderEdit,
) -> Result<Rootlist, Failure> {
    send_with(
        edit,
        READ_PAUSE,
        Limits::default(),
        jiff::Timestamp::now().as_millisecond(),
        &new_id(),
        || engine.read_rootlist(),
        |changes| post(engine, http, changes),
    )
    .await
}

/// How posting a change went wrong.
#[derive(Debug)]
enum PostError {
    /// The change never reached Spotify, or Spotify answered that it did
    /// not take it.
    Failed(String),
    /// The request went out but no answer came back: Spotify may hold the
    /// change or may not.
    Unknown(String),
}

/// [`send`], with the reads and the post passed in.
#[allow(clippy::too_many_arguments)] // Each is a seam the tests drive.
async fn send_with<R, RF, P, PF>(
    edit: &FolderEdit,
    pause: std::time::Duration,
    limits: Limits,
    now_ms: i64,
    temp_id: &str,
    read: R,
    post: P,
) -> Result<Rootlist, Failure>
where
    R: Fn() -> RF,
    RF: std::future::Future<
            Output = anyhow::Result<(RawRootlist, std::collections::BTreeSet<String>)>,
        >,
    P: FnOnce(pb::ListChanges) -> PF,
    PF: std::future::Future<Output = Result<Option<u32>, PostError>>,
{
    // Nothing has been sent until the post: running out of time before it
    // means the edit failed.
    let (raw, _) = tokio::time::timeout(
        limits.plan,
        read_until(3, pause, |raw| !raw.revision.is_empty(), &read),
    )
    .await
    .map_err(|_| Failure::Failed("Spotify took too long to answer.".into()))??;
    let changes = plan(&raw, edit, now_ms, temp_id).map_err(Failure::Refused)?;
    let written = post(changes).await.map_err(|error| match error {
        PostError::Failed(error) => Failure::Failed(error),
        PostError::Unknown(error) => Failure::Unconfirmed(error),
    })?;
    // From here on Spotify holds the change; a read that fails, never
    // catches up with it, or runs out of time, says only that it could not
    // be seen yet.
    let (raw, editable) = tokio::time::timeout(
        limits.confirm,
        read_until(4, pause, |raw| caught_up(written, &raw.revision), &read),
    )
    .await
    .map_err(|_| Failure::Unconfirmed("Spotify took too long to show the change.".into()))?
    .map_err(|error| Failure::Unconfirmed(format!("{error:#}")))?;
    if !caught_up(written, &raw.revision) {
        return Err(Failure::Unconfirmed(
            "Spotify has not shown the change yet".into(),
        ));
    }
    Ok(Rootlist {
        entries: crate::player::parse_rootlist(&raw.uris),
        editable,
    })
}

/// Posts the change once and returns the revision counter Spotify answered
/// with, when it gave one.
async fn post(
    engine: &crate::player::Engine,
    http: &reqwest::Client,
    changes: pb::ListChanges,
) -> Result<Option<u32>, PostError> {
    use protobuf::Message as _;

    let failed = |error: anyhow::Error| PostError::Failed(format!("{error:#}"));
    let body = changes
        .write_to_bytes()
        .map_err(|error| failed(error.into()))?;
    let session = engine.session();
    let base = session
        .spclient()
        .base_url()
        .await
        .map_err(|error| failed(error.into()))?;
    let endpoint = format!(
        "{base}/playlist/v2/user/{}/rootlist/changes",
        urlencoding::encode(&session.username()),
    );
    let login = session
        .login5()
        .auth_token()
        .await
        .map_err(|error| failed(error.into()))?;
    let client_token = session
        .spclient()
        .client_token()
        .await
        .map_err(|error| failed(error.into()))?;
    let response = http
        .post(endpoint)
        .bearer_auth(&login.access_token)
        .header("client-token", client_token)
        .header(reqwest::header::CONTENT_TYPE, "application/x-protobuf")
        .header(reqwest::header::ACCEPT, "application/x-protobuf")
        .body(body)
        .send()
        .await
        .map_err(|error| {
            // A connection that never opened, or a request that could not
            // be built, sent nothing; anything later may have reached it.
            let sent_nothing = error.is_connect() || error.is_builder();
            let error = error.without_url().to_string();
            if sent_nothing {
                PostError::Failed(error)
            } else {
                PostError::Unknown(error)
            }
        })?;
    let status = response.status();
    if !status.is_success() {
        return Err(PostError::Failed(format!(
            "Spotify did not accept the folder change (HTTP {})",
            status.as_u16()
        )));
    }
    Ok(response
        .bytes()
        .await
        .ok()
        .and_then(|bytes| pb::SelectedListContent::parse_from_bytes(&bytes).ok())
        .and_then(|content| content.revision)
        .and_then(|revision| revision_counter(&revision)))
}

#[cfg(test)]
mod tests;
