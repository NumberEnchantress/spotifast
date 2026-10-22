//! The folder edits against a stand-in for Spotify's rootlist service.
//!
//! [`Server`] applies a `ListChanges` the way the playlist4 service does for
//! the ops these edits use: an add lands first, last, or before the item its
//! anchor names; a keyed removal or move finds each item by its URI, a
//! folder's start marker by its id alone; an index range counts rows of the
//! revision the request was made against, and a move's `to_index` counts
//! them before anything moves. Every edit is checked three ways: the
//! request has the shape Spotify's web player sends, the stand-in accepts
//! it, and the tree it leaves is exactly the one [`apply`] showed the user.

use super::*;
use protobuf::Message as _;

// ---- the stand-in ----------------------------------------------------------

#[derive(Clone, Debug)]
struct Server {
    uris: Vec<String>,
    counter: u32,
}

/// Whether `key` names `uri`: the same URI, or start markers of one folder.
fn names(key: &str, uri: &str) -> bool {
    key == uri || matches!((start_id(key), start_id(uri)), (Some(a), Some(b)) if a == b)
}

impl Server {
    fn new(uris: Vec<String>) -> Self {
        Self { uris, counter: 7 }
    }

    fn revision(&self) -> Vec<u8> {
        let mut revision = self.counter.to_be_bytes().to_vec();
        revision.extend([0xab; 20]);
        revision
    }

    fn raw(&self) -> RawRootlist {
        RawRootlist {
            uris: self.uris.clone(),
            revision: self.revision(),
        }
    }

    fn find(uris: &[String], key: &str) -> Result<usize, String> {
        uris.iter()
            .position(|uri| names(key, uri))
            .ok_or_else(|| format!("no item {key}"))
    }

    fn insert_at(
        uris: &[String],
        first: bool,
        last: bool,
        before: Option<&pb::Item>,
        after: Option<&pb::Item>,
        from_index: Option<i32>,
    ) -> Result<usize, String> {
        let chosen = usize::from(first)
            + usize::from(last)
            + usize::from(before.is_some())
            + usize::from(after.is_some())
            + usize::from(from_index.is_some());
        if chosen != 1 {
            return Err(format!("{chosen} positions given"));
        }
        if first {
            Ok(0)
        } else if last {
            Ok(uris.len())
        } else if let Some(anchor) = before {
            Self::find(uris, anchor.uri())
        } else if let Some(anchor) = after {
            Ok(Self::find(uris, anchor.uri())? + 1)
        } else {
            let at = from_index.unwrap() as usize;
            (at <= uris.len())
                .then_some(at)
                .ok_or_else(|| "add past the end".into())
        }
    }

    /// Applies a request as one transaction: on any error nothing changes.
    fn post(&mut self, changes: &pb::ListChanges) -> Result<(), String> {
        // Over the wire and back, as the service would read it.
        let changes = pb::ListChanges::parse_from_bytes(
            &changes
                .write_to_bytes()
                .map_err(|error| error.to_string())?,
        )
        .map_err(|error| error.to_string())?;
        let mut uris = self.uris.clone();
        for delta in &changes.deltas {
            for op in &delta.ops {
                let indexed = match op.kind() {
                    pb::op::Kind::ADD => op.add.from_index.is_some(),
                    pb::op::Kind::REM => !op.rem.items_as_key(),
                    pb::op::Kind::MOV => op.mov.items.is_empty(),
                    kind => return Err(format!("unexpected op {kind:?}")),
                };
                if indexed && changes.base_revision.as_deref() != Some(&self.revision()[..]) {
                    return Err("an index op against another revision".into());
                }
                match op.kind() {
                    pb::op::Kind::ADD => {
                        let add = &op.add;
                        let at = Self::insert_at(
                            &uris,
                            add.add_first(),
                            add.add_last(),
                            add.add_before_item.as_ref(),
                            add.add_after_item.as_ref(),
                            add.from_index,
                        )?;
                        for (offset, item) in add.items.iter().enumerate() {
                            if !item.attributes.timestamp.is_some_and(|time| time > 0) {
                                return Err(format!("{} added without a time", item.uri()));
                            }
                            uris.insert(at + offset, item.uri().to_string());
                        }
                    }
                    pb::op::Kind::REM => {
                        let rem = &op.rem;
                        if rem.items_as_key() {
                            for item in &rem.items {
                                let at = Self::find(&uris, item.uri())?;
                                uris.remove(at);
                            }
                        } else {
                            let from = rem.from_index() as usize;
                            let length = rem.length() as usize;
                            if length == 0 || from + length > uris.len() {
                                return Err("removal out of range".into());
                            }
                            uris.drain(from..from + length);
                        }
                    }
                    pb::op::Kind::MOV => {
                        let mov = &op.mov;
                        if mov.items.is_empty() {
                            let from = mov.from_index() as usize;
                            let length = mov.length() as usize;
                            let to = mov.to_index() as usize;
                            if length == 0 || from + length > uris.len() || to > uris.len() {
                                return Err("move out of range".into());
                            }
                            if to > from && to < from + length {
                                return Err("move into itself".into());
                            }
                            let block: Vec<_> = uris.drain(from..from + length).collect();
                            let at = if to > from { to - length } else { to };
                            uris.splice(at..at, block);
                        } else {
                            let mut block = Vec::new();
                            for item in &mov.items {
                                let at = Self::find(&uris, item.uri())?;
                                block.push(uris.remove(at));
                            }
                            let at = Self::insert_at(
                                &uris,
                                mov.add_first(),
                                mov.add_last(),
                                mov.add_before_item.as_ref(),
                                mov.add_after_item.as_ref(),
                                None,
                            )?;
                            uris.splice(at..at, block);
                        }
                    }
                    _ => unreachable!(),
                }
            }
        }
        self.uris = uris;
        self.counter += 1;
        Ok(())
    }
}

// ---- inputs ------------------------------------------------------------------

/// A small deterministic generator, so a failure names its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len())])
    }
}

const NAMES: &[&str] = &[
    "Mix",
    "Late Night Mix",
    "100% \u{2615}",
    "a:b/c",
    "Rock & Roll",
    "  spaced   out ",
    "\u{65e5}\u{672c}\u{8a9e}",
    "x+y=z",
    "it's (fine)!",
];

fn tree(rng: &mut Rng, depth: usize, next: &mut usize, out: &mut Vec<String>) {
    for _ in 0..rng.below(6) {
        *next += 1;
        match rng.below(12) {
            0..=6 => out.push(format!("spotify:playlist:p{next}")),
            7..=9 if depth < 3 => {
                let id = format!("{:016x}", 0xf000 + *next);
                let name = folder_name(rng.pick(NAMES).unwrap()).unwrap();
                // Now and then a marker with no name, as old folders have.
                out.push(start_marker(&id, (rng.below(8) > 0).then_some(&*name)));
                tree(rng, depth + 1, next, out);
                out.push(end_marker(&id));
            }
            // Rows the sidebar does not show must not throw the counts off.
            10 => out.push(format!("spotify:artist:a{next}")),
            _ => out.push(format!("spotify:playlist:p{next}")),
        }
    }
}

fn rootlist(rng: &mut Rng) -> Vec<String> {
    let mut out = Vec::new();
    tree(rng, 0, &mut 0, &mut out);
    out
}

fn playlists(rows: &[RootlistEntry]) -> Vec<String> {
    rows.iter()
        .filter_map(|row| match row {
            RootlistEntry::Playlist(uri) => Some(uri.clone()),
            _ => None,
        })
        .collect()
}

fn place(rng: &mut Rng, rows: &[RootlistEntry]) -> Place {
    let lists = playlists(rows);
    let folders: Vec<String> = folders(rows).into_iter().map(|folder| folder.id).collect();
    match rng.below(9) {
        0 => Place::Start,
        1 => Place::End,
        2 | 3 => match rng.pick(&lists) {
            Some(uri) => Place::Before(Row::Playlist(uri.clone())),
            None => Place::End,
        },
        4 | 5 => match rng.pick(&folders) {
            Some(id) => Place::Before(Row::Folder(id.clone())),
            None => Place::Start,
        },
        6 | 7 => match rng.pick(&folders) {
            Some(id) => Place::InFolder(id.clone()),
            None => Place::End,
        },
        8 if rng.below(2) == 0 => match rng.pick(&lists) {
            Some(uri) => Place::AfterPlaylist(uri.clone()),
            None => Place::End,
        },
        _ => Place::Before(Row::Playlist("spotify:playlist:gone".into())),
    }
}

fn edit(rng: &mut Rng, rows: &[RootlistEntry], serial: usize) -> FolderEdit {
    let lists = playlists(rows);
    let folders: Vec<String> = folders(rows).into_iter().map(|folder| folder.id).collect();
    let folder = |rng: &mut Rng| {
        if rng.below(10) == 0 {
            "0000000000dead00".to_string()
        } else {
            rng.pick(&folders)
                .cloned()
                .unwrap_or_else(|| "0000000000dead00".into())
        }
    };
    let list = |rng: &mut Rng| {
        rng.pick(&lists)
            .cloned()
            .unwrap_or_else(|| "spotify:playlist:gone".into())
    };
    match rng.below(6) {
        0 => FolderEdit::Create {
            id: format!("{:016x}", 0xc0de_0000 + serial),
            name: if rng.below(12) == 0 {
                "   ".into()
            } else {
                rng.pick(NAMES).unwrap().to_string()
            },
            place: place(rng, rows),
            adopt: (rng.below(2) == 0).then(|| list(rng)),
        },
        1 => FolderEdit::Rename {
            id: folder(rng),
            name: rng.pick(NAMES).unwrap().to_string(),
        },
        2 => FolderEdit::Remove { id: folder(rng) },
        3 | 4 => FolderEdit::MovePlaylist {
            uri: list(rng),
            place: place(rng, rows),
        },
        _ => FolderEdit::MoveFolder {
            id: folder(rng),
            place: place(rng, rows),
        },
    }
}

/// Every folder in the raw rows opens and closes in order, and no id is
/// used twice.
fn well_formed(uris: &[String]) -> bool {
    let mut open = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for uri in uris {
        if let Some(id) = start_id(uri) {
            if !seen.insert(id) {
                return false;
            }
            open.push(id);
        } else if let Some(id) = end_id(uri)
            && open.pop() != Some(id)
        {
            return false;
        }
    }
    open.is_empty()
}

fn sorted(mut list: Vec<String>) -> Vec<String> {
    list.sort();
    list
}

/// One edit, checked against the stand-in. Returns whether it was made.
fn check(
    server: &mut Server,
    shown: &mut Vec<RootlistEntry>,
    edit: &FolderEdit,
    seed: u64,
) -> bool {
    let raw = server.raw();
    let before = server.uris.clone();
    let context = || format!("seed {seed}\nedit {edit:?}\nrows {before:#?}");
    assert_eq!(
        &crate::player::parse_rootlist(&raw.uris),
        shown,
        "{}",
        context()
    );
    let local = apply(shown, edit);
    let planned = plan(&raw, edit, 1_700_000_000_000, "00000000tempf01d");
    match local {
        Ok(rows) => {
            let changes = planned
                .unwrap_or_else(|refusal| panic!("shown but refused {refusal:?}\n{}", context()));
            server
                .post(&changes)
                .unwrap_or_else(|error| panic!("the service refused: {error}\n{}", context()));
            assert_eq!(
                crate::player::parse_rootlist(&server.uris),
                rows,
                "Spotify would not hold what was shown\n{}\nafter {:#?}",
                context(),
                server.uris
            );
            assert!(well_formed(&server.uris), "{}", context());
            assert!(
                !server.uris.iter().any(|uri| uri.contains("tempf01d")),
                "a passing marker stayed\n{}",
                context()
            );
            // No edit ever takes a playlist out of the library or adds one.
            let kept = |uris: &[String]| {
                sorted(
                    uris.iter()
                        .filter(|uri| start_id(uri).is_none() && end_id(uri).is_none())
                        .cloned()
                        .collect(),
                )
            };
            assert_eq!(kept(&before), kept(&server.uris), "{}", context());
            *shown = rows;
            true
        }
        Err(Refusal::Unchanged) => {
            // Nothing is sent, but were it sent, nothing visible may move.
            if let Ok(changes) = planned {
                let mut copy = server.clone();
                if copy.post(&changes).is_ok() {
                    assert_eq!(
                        &crate::player::parse_rootlist(&copy.uris),
                        shown,
                        "{}",
                        context()
                    );
                }
            }
            false
        }
        Err(refusal) => {
            assert!(
                planned.is_err(),
                "the sidebar refused {refusal:?} but a request was planned\n{}",
                context()
            );
            false
        }
    }
}

// ---- the request's shape -----------------------------------------------------

fn sample() -> RawRootlist {
    RawRootlist {
        uris: [
            "spotify:playlist:a",
            "spotify:start-group:00000000000000f1:Late+Night",
            "spotify:playlist:b",
            "spotify:playlist:c",
            "spotify:end-group:00000000000000f1",
            "spotify:playlist:d",
        ]
        .map(String::from)
        .to_vec(),
        revision: vec![0, 0, 0, 9, 1, 2, 3],
    }
}

fn ops(changes: &pb::ListChanges) -> &[pb::Op] {
    assert_eq!(changes.deltas.len(), 1);
    &changes.deltas[0].ops
}

#[test]
fn every_request_is_one_delta_from_a_client_and_round_trips() {
    let raw = sample();
    for edit in [
        FolderEdit::Create {
            id: "00000000000000f2".into(),
            name: "New".into(),
            place: Place::Start,
            adopt: Some("spotify:playlist:d".into()),
        },
        FolderEdit::Rename {
            id: "00000000000000f1".into(),
            name: "Early".into(),
        },
        FolderEdit::Remove {
            id: "00000000000000f1".into(),
        },
        FolderEdit::MovePlaylist {
            uri: "spotify:playlist:a".into(),
            place: Place::InFolder("00000000000000f1".into()),
        },
        FolderEdit::MoveFolder {
            id: "00000000000000f1".into(),
            place: Place::End,
        },
    ] {
        let changes = plan(&raw, &edit, 5, "00000000000000ee").unwrap();
        let bytes = changes.write_to_bytes().unwrap();
        assert_eq!(pb::ListChanges::parse_from_bytes(&bytes).unwrap(), changes);
        let delta = &changes.deltas[0];
        assert_eq!(
            delta.info.source.client(),
            pb::source_info::Client::CLIENT,
            "{edit:?}"
        );
        assert!(!changes.want_resulting_revisions());
        assert!(!changes.want_sync_result());
    }
}

#[test]
fn a_new_folder_is_its_two_markers_with_the_name_encoded() {
    let changes = plan(
        &sample(),
        &FolderEdit::Create {
            id: "00000000000000f2".into(),
            name: " Road   trip: 2026 ".into(),
            place: Place::Start,
            adopt: None,
        },
        1234,
        "00000000000000ee",
    )
    .unwrap();
    // Names travel as URIs, so nothing counts rows and no revision is sent.
    assert!(changes.base_revision.is_none());
    let [op] = ops(&changes) else { panic!() };
    assert_eq!(op.kind(), pb::op::Kind::ADD);
    assert!(op.add.add_first());
    let uris: Vec<_> = op.add.items.iter().map(|item| item.uri()).collect();
    assert_eq!(
        uris,
        [
            "spotify:start-group:00000000000000f2:Road+trip%3A+2026",
            "spotify:end-group:00000000000000f2",
        ]
    );
    assert!(
        op.add
            .items
            .iter()
            .all(|item| item.attributes.timestamp() == 1234)
    );
}

#[test]
fn a_new_folder_can_take_a_playlist_with_it() {
    let changes = plan(
        &sample(),
        &FolderEdit::Create {
            id: "00000000000000f2".into(),
            name: "Gym".into(),
            place: Place::Before(Row::Playlist("spotify:playlist:d".into())),
            adopt: Some("spotify:playlist:d".into()),
        },
        1,
        "00000000000000ee",
    )
    .unwrap();
    let [add, mov] = ops(&changes) else { panic!() };
    assert_eq!(add.add.add_before_item.uri(), "spotify:playlist:d");
    assert_eq!(mov.kind(), pb::op::Kind::MOV);
    assert_eq!(mov.mov.items[0].uri(), "spotify:playlist:d");
    // First inside the new folder, as the web player's "move to folder"
    // anchors after the folder's start marker; never on an end marker.
    assert!(mov.mov.add_before_item.is_none());
    assert_eq!(
        mov.mov.add_after_item.uri(),
        "spotify:start-group:00000000000000f2"
    );
}

/// The web player's rename: a passing marker, then the real one, so the
/// folder's id is never held twice and the end marker still matches.
#[test]
fn a_rename_goes_through_a_passing_marker() {
    let changes = plan(
        &sample(),
        &FolderEdit::Rename {
            id: "00000000000000f1".into(),
            name: "After Hours".into(),
        },
        1,
        "00000000000000ee",
    )
    .unwrap();
    assert!(changes.base_revision.is_none());
    let steps: Vec<(pb::op::Kind, String, String)> = ops(&changes)
        .iter()
        .map(|op| match op.kind() {
            pb::op::Kind::ADD => (
                op.kind(),
                op.add.items[0].uri().to_string(),
                op.add.add_before_item.uri().to_string(),
            ),
            pb::op::Kind::REM => {
                assert!(op.rem.items_as_key());
                (op.kind(), op.rem.items[0].uri().to_string(), String::new())
            }
            kind => panic!("{kind:?}"),
        })
        .collect();
    let passing = "spotify:start-group:00000000000000ee:After+Hours";
    let current = "spotify:start-group:00000000000000f1";
    assert_eq!(
        steps,
        [
            (pb::op::Kind::ADD, passing.into(), current.into()),
            (pb::op::Kind::REM, current.into(), String::new()),
            (
                pb::op::Kind::ADD,
                "spotify:start-group:00000000000000f1:After+Hours".into(),
                passing.into()
            ),
            (pb::op::Kind::REM, passing.into(), String::new()),
        ]
    );
}

#[test]
fn a_folder_moves_as_one_range_counted_before_the_move() {
    let raw = sample();
    let changes = plan(
        &raw,
        &FolderEdit::MoveFolder {
            id: "00000000000000f1".into(),
            place: Place::End,
        },
        1,
        "00000000000000ee",
    )
    .unwrap();
    assert_eq!(changes.base_revision(), &raw.revision[..]);
    let [op] = ops(&changes) else { panic!() };
    assert_eq!(
        (op.mov.from_index(), op.mov.length(), op.mov.to_index()),
        (1, 4, 6)
    );
    assert!(op.mov.items.is_empty());
}

#[test]
fn a_playlist_moves_by_its_uri() {
    let changes = plan(
        &sample(),
        &FolderEdit::MovePlaylist {
            uri: "spotify:playlist:c".into(),
            place: Place::Before(Row::Folder("00000000000000f1".into())),
        },
        1,
        "00000000000000ee",
    )
    .unwrap();
    assert!(changes.base_revision.is_none());
    let [op] = ops(&changes) else { panic!() };
    assert_eq!(op.mov.items.len(), 1);
    assert_eq!(op.mov.items[0].uri(), "spotify:playlist:c");
    assert_eq!(
        op.mov.add_before_item.uri(),
        "spotify:start-group:00000000000000f1"
    );
}

// ---- refusals ----------------------------------------------------------------

#[test]
fn a_folder_cannot_move_into_itself_or_its_own_folders() {
    let mut raw = sample();
    raw.uris.splice(
        3..3,
        [
            "spotify:start-group:00000000000000f3:Inner".to_string(),
            "spotify:end-group:00000000000000f3".to_string(),
        ],
    );
    let shown = crate::player::parse_rootlist(&raw.uris);
    for place in [
        Place::InFolder("00000000000000f1".into()),
        Place::InFolder("00000000000000f3".into()),
        Place::Before(Row::Folder("00000000000000f3".into())),
        Place::Before(Row::Playlist("spotify:playlist:c".into())),
    ] {
        let edit = FolderEdit::MoveFolder {
            id: "00000000000000f1".into(),
            place,
        };
        assert_eq!(apply(&shown, &edit), Err(Refusal::IntoItself), "{edit:?}");
        assert_eq!(plan(&raw, &edit, 1, "ee").unwrap_err(), Refusal::IntoItself);
    }
}

#[test]
fn damaged_markers_are_left_alone() {
    let raw = RawRootlist {
        uris: [
            "spotify:start-group:00000000000000f1:A",
            "spotify:start-group:00000000000000f2:B",
            "spotify:end-group:00000000000000f1",
            "spotify:end-group:00000000000000f2",
        ]
        .map(String::from)
        .to_vec(),
        revision: vec![0, 0, 0, 1],
    };
    for edit in [
        FolderEdit::Remove {
            id: "00000000000000f1".into(),
        },
        FolderEdit::MoveFolder {
            id: "00000000000000f2".into(),
            place: Place::Start,
        },
    ] {
        assert_eq!(plan(&raw, &edit, 1, "ee").unwrap_err(), Refusal::Malformed);
    }
}

#[test]
fn names_need_more_than_space() {
    let shown = crate::player::parse_rootlist(&sample().uris);
    let edit = FolderEdit::Rename {
        id: "00000000000000f1".into(),
        name: " \t ".into(),
    };
    assert_eq!(apply(&shown, &edit), Err(Refusal::NoName));
    assert_eq!(
        plan(&sample(), &edit, 1, "ee").unwrap_err(),
        Refusal::NoName
    );
}

#[test]
fn removing_a_folder_keeps_what_it_held_in_place() {
    let shown = crate::player::parse_rootlist(&sample().uris);
    let rows = apply(
        &shown,
        &FolderEdit::Remove {
            id: "00000000000000f1".into(),
        },
    )
    .unwrap();
    assert_eq!(
        rows,
        ["a", "b", "c", "d"]
            .map(|id| RootlistEntry::Playlist(format!("spotify:playlist:{id}")))
            .to_vec()
    );
}

/// Removing a folder names its two markers by URI, so it can only ever
/// match them: however the rows have moved since the read, no playlist is
/// counted into the removal.
#[test]
fn removing_a_folder_names_only_its_markers() {
    let changes = plan(
        &sample(),
        &FolderEdit::Remove {
            id: "00000000000000f1".into(),
        },
        1,
        "00000000000000ee",
    )
    .unwrap();
    assert!(changes.base_revision.is_none());
    let keys: Vec<_> = ops(&changes)
        .iter()
        .map(|op| {
            assert_eq!(op.kind(), pb::op::Kind::REM);
            assert!(op.rem.items_as_key());
            assert!(op.rem.from_index.is_none() && op.rem.length.is_none());
            let [item] = &op.rem.items[..] else { panic!() };
            item.uri().to_string()
        })
        .collect();
    assert_eq!(
        keys,
        [
            "spotify:end-group:00000000000000f1",
            "spotify:start-group:00000000000000f1",
        ]
    );
    // Even against a tree that has changed since the read, Spotify is left
    // with every playlist it had.
    let mut server = Server::new(
        [
            "spotify:playlist:new",
            "spotify:playlist:a",
            "spotify:start-group:00000000000000f1:Late+Night",
            "spotify:playlist:b",
            "spotify:end-group:00000000000000f1",
        ]
        .map(String::from)
        .to_vec(),
    );
    server.post(&changes).unwrap();
    assert_eq!(
        server.uris,
        [
            "spotify:playlist:new",
            "spotify:playlist:a",
            "spotify:playlist:b"
        ]
        .map(String::from)
    );
}

#[test]
fn taking_a_row_out_of_a_folder_puts_it_just_above_that_folder() {
    use RootlistEntry::{FolderEnd, FolderStart, Playlist};
    let folder = |id: &str| FolderStart {
        id: id.into(),
        name: id.into(),
    };
    let rows = vec![
        folder("outer"),
        folder("inner"),
        Playlist("spotify:playlist:a".into()),
        FolderEnd,
        FolderEnd,
        Playlist("spotify:playlist:b".into()),
    ];
    let out = out_of_folder(&rows, &Row::Playlist("spotify:playlist:a".into())).unwrap();
    assert_eq!(out, Place::Before(Row::Folder("inner".into())));
    let moved = apply(
        &rows,
        &FolderEdit::MovePlaylist {
            uri: "spotify:playlist:a".into(),
            place: out,
        },
    )
    .unwrap();
    assert_eq!(
        parent(&moved, &Row::Playlist("spotify:playlist:a".into())),
        Some("outer".into())
    );
    assert_eq!(
        out_of_folder(&rows, &Row::Folder("inner".into())),
        Some(Place::Before(Row::Folder("outer".into())))
    );
    assert_eq!(
        out_of_folder(&rows, &Row::Playlist("spotify:playlist:b".into())),
        None
    );
}

#[test]
fn into_a_folder_is_first_inside_it() {
    let edit = FolderEdit::MovePlaylist {
        uri: "spotify:playlist:d".into(),
        place: Place::InFolder("00000000000000f1".into()),
    };
    let shown = crate::player::parse_rootlist(&sample().uris);
    let rows = apply(&shown, &edit).unwrap();
    assert_eq!(
        rows[2],
        RootlistEntry::Playlist("spotify:playlist:d".into())
    );
    let changes = plan(&sample(), &edit, 1, "ee").unwrap();
    let [op] = ops(&changes) else { panic!() };
    assert!(op.mov.add_before_item.is_none());
    assert_eq!(
        op.mov.add_after_item.uri(),
        "spotify:start-group:00000000000000f1"
    );
}

/// Spotify finds a folder's markers by id, so an id held twice leaves which
/// one it would pick unknown: every edit that names one is refused, and a
/// new folder never takes an id already in the tree.
#[test]
fn a_doubled_folder_id_is_never_edited_or_made() {
    let mut raw = sample();
    raw.uris.extend(
        [
            "spotify:start-group:00000000000000f1:Copy",
            "spotify:end-group:00000000000000f1",
        ]
        .map(String::from),
    );
    let shown = crate::player::parse_rootlist(&raw.uris);
    let id = || "00000000000000f1".to_string();
    for edit in [
        FolderEdit::Rename {
            id: id(),
            name: "New".into(),
        },
        FolderEdit::Remove { id: id() },
        FolderEdit::MoveFolder {
            id: id(),
            place: Place::Start,
        },
        FolderEdit::MovePlaylist {
            uri: "spotify:playlist:a".into(),
            place: Place::InFolder(id()),
        },
        FolderEdit::Create {
            id: id(),
            name: "Again".into(),
            place: Place::End,
            adopt: None,
        },
    ] {
        assert_eq!(apply(&shown, &edit), Err(Refusal::Malformed), "{edit:?}");
        assert_eq!(
            plan(&raw, &edit, 1, "ee").map(|_| ()),
            Err(Refusal::Malformed),
            "{edit:?}"
        );
    }
    // A stray end marker alone still holds its id.
    let mut raw = sample();
    raw.uris.push("spotify:end-group:00000000000000f9".into());
    let create = FolderEdit::Create {
        id: "00000000000000f9".into(),
        name: "New".into(),
        place: Place::End,
        adopt: None,
    };
    assert_eq!(
        plan(&raw, &create, 1, "ee").map(|_| ()),
        Err(Refusal::Malformed)
    );
}

/// The passing marker of a rename must not share an id with any folder.
#[test]
fn a_rename_never_passes_through_a_taken_id() {
    let edit = FolderEdit::Rename {
        id: "00000000000000f1".into(),
        name: "New".into(),
    };
    assert_eq!(
        plan(&sample(), &edit, 1, "00000000000000f1").map(|_| ()),
        Err(Refusal::Malformed)
    );
    assert!(plan(&sample(), &edit, 1, "00000000000000ee").is_ok());
}

/// A folder moves by counting rows, which holds only for one revision; a
/// read whose pages carried different revisions has none, and no move is
/// planned against it.
#[test]
fn rows_are_only_counted_in_a_read_from_one_revision() {
    let a = vec![0, 0, 0, 9];
    let b = vec![0, 0, 0, 10];
    assert_eq!(one_revision(vec![Some(a.clone()), Some(a.clone())]), a);
    assert_eq!(one_revision(vec![Some(a.clone())]), a);
    assert!(one_revision(vec![Some(a.clone()), Some(b)]).is_empty());
    assert!(one_revision(vec![Some(a.clone()), None]).is_empty());
    assert!(one_revision(vec![None]).is_empty());
    assert!(one_revision(vec![]).is_empty());

    let mut raw = sample();
    raw.revision.clear();
    let edit = FolderEdit::MoveFolder {
        id: "00000000000000f1".into(),
        place: Place::End,
    };
    assert_eq!(
        plan(&raw, &edit, 1, "ee").map(|_| ()),
        Err(Refusal::Missing)
    );
    // Edits that name rows by URI count nothing, and still go.
    let remove = FolderEdit::Remove {
        id: "00000000000000f1".into(),
    };
    assert!(plan(&raw, &remove, 1, "ee").is_ok());
}

/// After an edit the tree is read until a read holds it: one at or past
/// the revision Spotify answered with, with its pages in agreement.
#[test]
fn the_tree_is_read_back_until_it_holds_the_edit() {
    assert!(caught_up(Some(10), &[0, 0, 0, 10]));
    assert!(caught_up(Some(10), &[0, 0, 0, 11]));
    assert!(!caught_up(Some(10), &[0, 0, 0, 9]));
    assert!(!caught_up(Some(10), &[]));
    assert!(caught_up(None, &[]));

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let reads = |counters: Vec<u32>| {
        let left = std::cell::RefCell::new(counters.into_iter());
        let made = std::cell::Cell::new(0);
        let found = runtime.block_on(read_until(
            4,
            std::time::Duration::ZERO,
            |raw| caught_up(Some(10), &raw.revision),
            || {
                made.set(made.get() + 1);
                let counter = left.borrow_mut().next().unwrap();
                async move {
                    Ok((
                        RawRootlist {
                            uris: Vec::new(),
                            revision: counter.to_be_bytes().to_vec(),
                        },
                        Default::default(),
                    ))
                }
            },
        ));
        (found.unwrap().0.revision, made.get())
    };
    // A lagging read is followed by another until one has caught up.
    assert_eq!(reads(vec![8, 9, 10, 11]), (10u32.to_be_bytes().to_vec(), 3));
    // One that is current at once is the only read.
    assert_eq!(reads(vec![12]), (12u32.to_be_bytes().to_vec(), 1));
    // It gives up after four, keeping the last.
    assert_eq!(reads(vec![1, 2, 3, 4, 5]), (4u32.to_be_bytes().to_vec(), 4));
}

// ---- names -------------------------------------------------------------------

#[test]
fn every_name_reads_back_as_it_was_written() {
    let mut rng = Rng(0x5eed_1234);
    let alphabet: Vec<char> = "aZ09 -_.!~*'()%+:/?#&=\u{e9}\u{2615}\u{65e5}\u{1f3b5}\t"
        .chars()
        .collect();
    for _ in 0..2_000 {
        let length = 1 + rng.below(12);
        let raw: String = (0..length).map(|_| *rng.pick(&alphabet).unwrap()).collect();
        let Some(name) = folder_name(&raw) else {
            continue;
        };
        let uris = vec![start_marker("00000000000000f1", Some(&name))];
        assert_eq!(
            crate::player::parse_rootlist(&uris)[0],
            RootlistEntry::FolderStart {
                id: "00000000000000f1".into(),
                name: name.clone()
            },
            "{raw:?}"
        );
    }
}

#[test]
fn refusals_are_told_in_the_users_language() {
    use crate::i18n::Locale;
    assert_eq!(
        Refusal::NoName.message(Locale::German),
        "Ein Ordner braucht einen Namen."
    );
    assert_eq!(
        crate::i18n::gettext(Locale::Japanese, "Move to folder"),
        "フォルダに移動"
    );
    assert_eq!(Refusal::Unchanged.message(Locale::English), "");
}

#[test]
fn new_ids_look_like_spotifys() {
    let id = new_id();
    assert_eq!(id.len(), 16);
    assert!(
        id.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
    assert_ne!(id, new_id());
}

#[test]
fn the_revision_counter_leads_the_revision() {
    assert_eq!(revision_counter(&[0, 0, 1, 2, 9, 9]), Some(258));
    assert_eq!(revision_counter(&[1, 2]), None);
}

// ---- the grader ----------------------------------------------------------------

/// Thousands of random trees and edits: whatever the sidebar shows after an
/// edit is what Spotify holds after the request, and no edit the sidebar
/// refuses is ever sent.
#[test]
fn spotify_ends_up_holding_what_the_sidebar_showed() {
    let mut made = 0;
    for seed in 1..=4_000u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let mut server = Server::new(rootlist(&mut rng));
        let mut shown = crate::player::parse_rootlist(&server.uris);
        let edit = edit(&mut rng, &shown, 0);
        if check(&mut server, &mut shown, &edit, seed) {
            made += 1;
        }
    }
    // Most of the random edits must actually be made, or this proves little.
    assert!(made > 1_500, "only {made} edits were made");
}

/// Long runs of edits on one account, each read back before the next, as a
/// user editing for a while would make them.
#[test]
fn a_long_session_of_edits_stays_in_step() {
    for seed in 1..=300u64 {
        let mut rng = Rng(seed.wrapping_mul(0xd1b5_4a32_d192_ed03) | 1);
        let mut server = Server::new(rootlist(&mut rng));
        let mut shown = crate::player::parse_rootlist(&server.uris);
        for serial in 0..40 {
            let edit = edit(&mut rng, &shown, serial);
            check(&mut server, &mut shown, &edit, seed);
        }
    }
}

// ---- sending ---------------------------------------------------------------

/// Reads served in order from a list; the last one repeats.
struct Reads(std::cell::RefCell<Vec<RawRootlist>>);

impl Reads {
    fn new(reads: Vec<RawRootlist>) -> Self {
        Self(std::cell::RefCell::new(reads))
    }

    fn next(&self) -> RawRootlist {
        let mut reads = self.0.borrow_mut();
        if reads.len() > 1 {
            reads.remove(0)
        } else {
            reads[0].clone()
        }
    }
}

fn at(counter: u32) -> RawRootlist {
    RawRootlist {
        revision: counter.to_be_bytes().to_vec(),
        ..sample()
    }
}

fn split_read() -> RawRootlist {
    RawRootlist {
        revision: Vec::new(),
        ..sample()
    }
}

/// Runs `send_with` against served reads and a post that answers `posted`,
/// returning the result and the request that was posted, if one was.
fn send_through(
    edit: &FolderEdit,
    reads: Vec<RawRootlist>,
    posted: Result<Option<u32>, PostError>,
) -> (Result<Rootlist, Failure>, Option<pb::ListChanges>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let reads = Reads::new(reads);
    let sent = std::cell::RefCell::new(None);
    let result = runtime.block_on(send_with(
        edit,
        std::time::Duration::ZERO,
        Limits::default(),
        1,
        "00000000000000ee",
        || {
            let read = reads.next();
            async move { Ok((read, Default::default())) }
        },
        |changes| {
            *sent.borrow_mut() = Some(changes);
            async move { posted }
        },
    ));
    (result, sent.into_inner())
}

fn move_folder() -> FolderEdit {
    FolderEdit::MoveFolder {
        id: "00000000000000f1".into(),
        place: Place::End,
    }
}

/// A folder's move waits for a read whose pages agree and is counted
/// against that read's revision.
#[test]
fn a_move_is_planned_against_a_read_whose_pages_agree() {
    let (result, sent) = send_through(
        &move_folder(),
        vec![split_read(), at(9), at(10)],
        Ok(Some(10)),
    );
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(sent.unwrap().base_revision(), &9u32.to_be_bytes()[..]);
    // With no agreeing read at all, nothing is sent.
    let (result, sent) = send_through(&move_folder(), vec![split_read()], Ok(Some(10)));
    assert_eq!(result.unwrap_err(), Failure::Refused(Refusal::Missing));
    assert!(sent.is_none());
}

/// Only an answer that says the change was not taken is a failure; a
/// request whose fate is unknown, or a change that cannot be read back, is
/// unconfirmed, so the app reads the tree rather than invite a second try.
#[test]
fn only_a_refused_post_is_reported_as_failed() {
    let remove = FolderEdit::Remove {
        id: "00000000000000f1".into(),
    };
    let (result, _) = send_through(
        &remove,
        vec![at(9)],
        Err(PostError::Failed("HTTP 403".into())),
    );
    assert_eq!(result.unwrap_err(), Failure::Failed("HTTP 403".into()));

    let (result, _) = send_through(
        &remove,
        vec![at(9)],
        Err(PostError::Unknown("connection reset".into())),
    );
    assert_eq!(
        result.unwrap_err(),
        Failure::Unconfirmed("connection reset".into())
    );

    // Spotify answered with revision 10, but every read stays at 9.
    let (result, sent) = send_through(&remove, vec![at(9)], Ok(Some(10)));
    assert!(sent.is_some());
    assert!(matches!(result, Err(Failure::Unconfirmed(_))), "{result:?}");

    // A read that has caught up is the tree from then on.
    let (result, _) = send_through(&remove, vec![at(9), at(10)], Ok(Some(10)));
    assert_eq!(
        result.unwrap().entries,
        crate::player::parse_rootlist(&sample().uris)
    );
}

/// Once Spotify has taken the change, a read that fails says only that the
/// change could not be seen, not that it failed.
#[test]
fn a_failed_read_after_the_post_is_unconfirmed() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let reads = std::cell::Cell::new(0);
    let result = runtime.block_on(send_with(
        &FolderEdit::Remove {
            id: "00000000000000f1".into(),
        },
        std::time::Duration::ZERO,
        Limits::default(),
        1,
        "00000000000000ee",
        || {
            reads.set(reads.get() + 1);
            let first = reads.get() == 1;
            async move {
                if first {
                    Ok((at(9), Default::default()))
                } else {
                    Err(anyhow::anyhow!("the session dropped"))
                }
            }
        },
        |_| async { Ok(Some(10)) },
    ));
    assert!(
        matches!(&result, Err(Failure::Unconfirmed(error)) if error.contains("the session dropped")),
        "{result:?}"
    );
}

/// Running out of time before the post means nothing was sent: the edit
/// failed. Running out of time after it means only the read back is
/// missing.
#[test]
fn a_timeout_is_a_failure_only_before_anything_is_sent() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let short = Limits {
        plan: std::time::Duration::from_millis(20),
        confirm: std::time::Duration::from_millis(20),
    };
    let remove = FolderEdit::Remove {
        id: "00000000000000f1".into(),
    };
    let posted = std::cell::Cell::new(false);
    // The first read never answers.
    let result =
        runtime.block_on(send_with(
            &remove,
            std::time::Duration::ZERO,
            short,
            1,
            "00000000000000ee",
            || {
                std::future::pending::<
                    anyhow::Result<(RawRootlist, std::collections::BTreeSet<String>)>,
                >()
            },
            |_| {
                posted.set(true);
                async { Ok(Some(10)) }
            },
        ));
    assert!(matches!(result, Err(Failure::Failed(_))), "{result:?}");
    assert!(!posted.get());

    // The first read answers; every read after the post hangs.
    let reads = std::cell::Cell::new(0);
    let result = runtime.block_on(send_with(
        &remove,
        std::time::Duration::ZERO,
        short,
        1,
        "00000000000000ee",
        || {
            reads.set(reads.get() + 1);
            let first = reads.get() == 1;
            async move {
                if first {
                    Ok((at(9), Default::default()))
                } else {
                    std::future::pending().await
                }
            }
        },
        |_| async { Ok(Some(10)) },
    ));
    assert!(matches!(result, Err(Failure::Unconfirmed(_))), "{result:?}");
}

/// Edits run strictly in the order they were queued, however long each
/// takes, so the second of a new folder and the move into it is always
/// planned against the folder.
#[test]
fn queued_edits_run_in_order() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
        .unwrap();
    let order = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let (done, finished) = std::sync::mpsc::channel();
    runtime.block_on(async {
        let jobs = queue();
        for (step, wait) in [(1, 40), (2, 0), (3, 10), (4, 0)] {
            let order = std::sync::Arc::clone(&order);
            let done = done.clone();
            jobs.send(Box::pin(async move {
                tokio::time::sleep(std::time::Duration::from_millis(wait)).await;
                order.lock().unwrap().push(step);
                let _ = done.send(());
            }))
            .unwrap();
        }
    });
    for _ in 0..4 {
        finished
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
    }
    assert_eq!(*order.lock().unwrap(), [1, 2, 3, 4]);
}

/// The end of a folder whose last row is a playlist is just after that
/// playlist, named by URI as the web player names an "after" place.
#[test]
fn after_a_playlist_is_beside_it_in_its_folder() {
    let edit = FolderEdit::MovePlaylist {
        uri: "spotify:playlist:a".into(),
        place: Place::AfterPlaylist("spotify:playlist:c".into()),
    };
    let shown = crate::player::parse_rootlist(&sample().uris);
    let rows = apply(&shown, &edit).unwrap();
    assert_eq!(
        parent(&rows, &Row::Playlist("spotify:playlist:a".into())),
        Some("00000000000000f1".into())
    );
    assert_eq!(
        rows[3],
        RootlistEntry::Playlist("spotify:playlist:a".into())
    );
    let changes = plan(&sample(), &edit, 1, "ee").unwrap();
    let [op] = ops(&changes) else { panic!() };
    assert!(op.mov.add_before_item.is_none());
    assert_eq!(op.mov.add_after_item.uri(), "spotify:playlist:c");
    let beside_itself = FolderEdit::MovePlaylist {
        uri: "spotify:playlist:c".into(),
        place: Place::AfterPlaylist("spotify:playlist:c".into()),
    };
    assert_eq!(apply(&shown, &beside_itself), Err(Refusal::Unchanged));
}
