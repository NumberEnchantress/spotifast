//! The left panel: navigation and Your Library.

use egui::{Align, CornerRadius, Frame, Layout, Margin, Rect, Sense, Vec2, pos2, vec2};

use crate::api::models::pick_image;
use crate::app::App;
use crate::folders::{FolderEdit, Place, Row};
use crate::i18n::{Locale, gettext};
use crate::model::{Action, Dialog, DragEntry, DragTrack, Loadable, Page};
use crate::player::RootlistEntry;
use crate::settings::{LIKED_SONGS_KEY, LibraryShelf as Filter, LibrarySort};
use crate::theme::{self, Icon, Palette};

const DEFAULT_ROW_HEIGHT: f32 = 60.0;
const COMPACT_ROW_HEIGHT: f32 = 32.0;

#[derive(Clone, Debug)]
pub(crate) struct Entry {
    image: Option<String>,
    grid_image: Option<String>,
    name: String,
    subtitle: String,
    grid_subtitle: String,
    page: Page,
    uri: String,
    round: bool,
    liked: bool,
    /// The account's own playlist: the one it may rename and delete.
    owned: bool,
    /// A playlist the account may drop songs on.
    editable: bool,
    playlist_index: Option<usize>,
    /// A folder row: its rootlist id, whether it is rolled up, and how
    /// many playlists it holds.
    folder: Option<(String, bool, usize)>,
    /// How deep inside folders the row sits, for the indent.
    depth: u8,
    added_at: Option<i64>,
    /// The key a row without a URI of its own is pinned by: a folder's
    /// library URI.
    pin_key: Option<String>,
}

impl Entry {
    /// What the row opens and plays; empty for Liked Songs and a folder.
    pub(crate) fn uri(&self) -> &str {
        &self.uri
    }

    fn ordering_key(&self) -> &str {
        if self.liked {
            LIKED_SONGS_KEY
        } else if let Some(key) = &self.pin_key {
            key
        } else {
            &self.uri
        }
    }
}

/// The context a library row plays, matching the cover play button and the
/// right-click menu. Liked Songs has no URI of its own and plays the
/// account's collection instead.
fn entry_play_uri(app: &App, entry: &Entry) -> Option<String> {
    if entry.liked {
        app.user
            .as_ref()
            .map(|user| format!("spotify:user:{}:collection", user.id))
    } else if entry.uri.is_empty() {
        None
    } else {
        Some(entry.uri.clone())
    }
}

fn entry_is_playing_context(entry: &Entry, context: Option<&str>) -> bool {
    if entry.liked {
        context.is_some_and(|context| context.ends_with(":collection"))
    } else {
        !entry.uri.is_empty() && context == Some(entry.uri.as_str())
    }
}

fn cover_play_button(
    app: &mut App,
    ui: &mut egui::Ui,
    entry: &Entry,
    index: usize,
    cover_rect: Rect,
    parent: &egui::Response,
    current_context: bool,
) -> bool {
    let Some(uri) = entry_play_uri(app, entry) else {
        return false;
    };
    // The control pauses only while this context is actually playing; a
    // paused one keeps the play glyph, but pressing it resumes in place
    // instead of restarting the context.
    let playing = current_context && app.believed_playing();
    let play = ui.interact(
        cover_rect,
        ui.id().with(("library-cover-play", index)),
        Sense::click(),
    );
    let play_hover = play.hovered();
    if play_hover || parent.hovered() {
        let radius = if entry.round {
            (cover_rect.width() / 2.0).min(127.0) as u8
        } else {
            6
        };
        ui.painter().rect_filled(
            cover_rect,
            CornerRadius::same(radius),
            egui::Color32::from_black_alpha(120),
        );
        let icon = if playing {
            Icon::PauseFilled
        } else {
            Icon::PlayFilled
        };
        let icon_size = (cover_rect.width() * 0.24).clamp(18.0, 26.0);
        icon.image(
            if play_hover {
                app.palette.accent
            } else {
                egui::Color32::WHITE
            },
            icon_size,
        )
        .paint_at(
            ui,
            Rect::from_center_size(
                cover_rect.center() + theme::play_glyph_offset(icon, icon_size),
                Vec2::splat(icon_size),
            ),
        );
    }
    if !play.clicked() {
        return false;
    }
    // The first click of a double click plays; the second must not play again.
    if !play.double_clicked() {
        if current_context {
            app.actions.push(Action::TogglePlay);
        } else {
            app.actions.push(Action::PlayContext {
                uri,
                offset_uri: None,
                offset_index: None,
            });
        }
    }
    true
}

fn grid_play_rect(cover_rect: Rect) -> Rect {
    let size = (cover_rect.width() * 0.3).clamp(32.0, 44.0);
    let inset = LIBRARY_ITEM_PADDING + size / 2.0;
    Rect::from_center_size(
        pos2(cover_rect.right() - inset, cover_rect.bottom() - inset),
        Vec2::splat(size),
    )
}

fn grid_pin_rect(cover_rect: Rect) -> Rect {
    Rect::from_center_size(cover_rect.left_top() + Vec2::splat(12.0), Vec2::splat(20.0))
}

fn grid_play_button(
    app: &mut App,
    ui: &mut egui::Ui,
    entry: &Entry,
    cover_rect: Rect,
    parent: &egui::Response,
    playing_here: bool,
) -> bool {
    let Some(uri) = entry_play_uri(app, entry) else {
        return false;
    };
    let rect = grid_play_rect(cover_rect);
    let size = rect.width();
    let button = ui.interact(rect, ui.id().with("library-grid-play"), Sense::click());
    let playing = playing_here && app.believed_playing();
    let label = if playing {
        // Translators: The play button on a Library card. {name} is the playlist, album, artist, or podcast.
        gettext(app.locale, "Pause {name}")
    } else {
        // Translators: The play button on a Library card. {name} is the playlist, album, artist, or podcast.
        gettext(app.locale, "Play {name}")
    }
    .replace("{name}", &entry.name);
    button.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label.clone())
    });
    if parent.hovered() || playing_here || button.has_focus() {
        let fill = if button.hovered() {
            app.palette.accent_hover
        } else {
            app.palette.accent
        };
        ui.painter().add(
            egui::epaint::Shadow {
                offset: [0, 4],
                blur: 12,
                spread: 0,
                color: egui::Color32::from_black_alpha(90),
            }
            .as_shape(rect, egui::CornerRadius::same(127)),
        );
        ui.painter().circle_filled(rect.center(), size / 2.0, fill);
        let icon = if playing {
            Icon::PauseFilled
        } else {
            Icon::PlayFilled
        };
        let icon_size = size * 0.42;
        icon.image(app.palette.on_accent, icon_size).paint_at(
            ui,
            Rect::from_center_size(
                rect.center() + theme::play_glyph_offset(icon, icon_size),
                Vec2::splat(icon_size),
            ),
        );
    }
    if !button.clicked() {
        return false;
    }
    // The second click of a double click plays nothing: either the first
    // played, or it opened a folder and this card slid under the pointer.
    if button.double_clicked() {
        return true;
    }
    if playing_here {
        app.actions.push(Action::TogglePlay);
    } else {
        app.actions.push(Action::PlayContext {
            uri,
            offset_uri: None,
            offset_index: None,
        });
    }
    true
}

fn finish_entry_interaction(
    app: &mut App,
    ui: &mut egui::Ui,
    response: &egui::Response,
    entry: &Entry,
    cover_took_click: bool,
    custom_order: bool,
    drop_allowed: bool,
) {
    if response.hovered()
        && let Some(image) = &entry.image
    {
        app.actions.push(Action::PrepareTint(image.clone()));
    }
    // A folder rides as its start marker, and only where folders can move.
    let drag_key = match &entry.folder {
        Some((id, _, _)) => app
            .folders_editable()
            .then(|| crate::folders::start_marker(id, None)),
        None => (!entry.ordering_key().is_empty()).then(|| entry.ordering_key().to_string()),
    };
    if let Some(key) = drag_key
        && response.drag_started_by(egui::PointerButton::Primary)
    {
        egui::DragAndDrop::set_payload(
            ui.ctx(),
            DragEntry {
                uri: key,
                title: entry.name.clone(),
                image: entry.image.clone(),
            },
        );
    }
    if drop_allowed
        && (entry.liked || entry.editable)
        && egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx())
        && let Some(track) = response.dnd_release_payload::<DragTrack>()
    {
        if entry.liked {
            app.actions.push(Action::SetSavedMany {
                uris: track
                    .items
                    .iter()
                    .map(|item| item.uri().to_string())
                    .collect(),
                saved: true,
            });
        } else if let Page::Playlist(id) = &entry.page {
            app.actions.push(Action::AddToPlaylist {
                playlist_id: id.clone(),
                playlist_name: entry.name.clone(),
                items: track.items.clone(),
            });
        }
    }
    theme::focus_ring(ui, response);
    // Opening a folder, or going back, moves other rows under the pointer:
    // the second click of a double click that lands on a row the first did
    // not touch neither opens nor plays it.
    let stray = (response.double_clicked() || response.triple_clicked())
        && ui.data(|data| data.get_temp::<egui::Id>(last_click_id())) != Some(response.id);
    if response.clicked() && !stray {
        ui.data_mut(|data| data.insert_temp(last_click_id(), response.id));
    }
    if response.clicked() && !cover_took_click && !stray {
        if let Some((folder_id, _, _)) = &entry.folder {
            app.actions
                .push(Action::EnterLibraryFolder(Some(folder_id.clone())));
        } else {
            app.actions.push(Action::Open(entry.page.clone()));
        }
    }
    if !app.settings.sidebar_grid
        && response.double_clicked()
        && entry.folder.is_none()
        && !cover_took_click
        && !stray
        && let Some(uri) = entry_play_uri(app, entry)
    {
        app.actions.push(Action::PlayContext {
            uri,
            offset_uri: None,
            offset_index: None,
        });
    }
    entry_menu(app, response, entry, custom_order);
    crate::autoscroll::row(ui, response);
}

/// Where the sidebar remembers the last row or Back button clicked, so
/// the second click of a double click knows whether it hit the same one.
fn last_click_id() -> egui::Id {
    egui::Id::new("sidebar-last-click")
}

/// The Library's Local Files row. It has no Spotify address, so it is
/// neither dragged, pinned nor played from the list; it opens its page.
fn local_files_entry(app: &App) -> Entry {
    let count = app.local_files.as_ref().map(|(_, uris)| uris.len() as u32);
    Entry {
        image: None,
        grid_image: None,
        name: gettext(app.locale, "Local Files").into_owned(),
        subtitle: count.map_or_else(
            || gettext(app.locale, "Playlist").into_owned(),
            |count| app.locale.liked_song_count(count),
        ),
        grid_subtitle: count.map_or_else(String::new, |count| app.locale.song_count(count)),
        page: Page::LocalFiles,
        uri: String::new(),
        round: false,
        liked: false,
        owned: false,
        editable: false,
        playlist_index: None,
        folder: None,
        depth: 0,
        added_at: None,
        pin_key: None,
    }
}

fn liked_entry(app: &App) -> Entry {
    let subtitle = match app.library.liked.total {
        Some(total) => app.locale.liked_song_count(total),
        None => gettext(app.locale, "Playlist").into_owned(),
    };
    let grid_subtitle = app
        .library
        .liked
        .total
        .map_or_else(String::new, |total| app.locale.song_count(total));
    Entry {
        image: None,
        grid_image: None,
        name: gettext(app.locale, "Liked Songs").into_owned(),
        subtitle,
        grid_subtitle,
        page: Page::LikedSongs,
        uri: String::new(),
        round: false,
        liked: true,
        owned: false,
        editable: false,
        playlist_index: None,
        folder: None,
        depth: 0,
        added_at: None,
        pin_key: None,
    }
}

/// The Library's kinds, in the order the filters and the groups list them.
const SHELVES: [Filter; 4] = [
    Filter::Playlists,
    Filter::Albums,
    Filter::Artists,
    Filter::Podcasts,
];

/// The kinds ticked in the Library's filters, each one a checkbox. None
/// ticked lists every kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Shown(u8);

impl Default for Shown {
    fn default() -> Self {
        Self::only(Filter::Playlists)
    }
}

impl Shown {
    fn bit(shelf: Filter) -> u8 {
        1 << SHELVES
            .iter()
            .position(|held| *held == shelf)
            .unwrap_or_default()
    }

    fn only(shelf: Filter) -> Self {
        Self(Self::bit(shelf))
    }

    /// The kinds saved as ticked.
    fn of(ticked: &[Filter]) -> Self {
        Self(
            ticked
                .iter()
                .fold(0, |bits, shelf| bits | Self::bit(*shelf)),
        )
    }

    /// The kinds ticked, none when none is, as the settings keep them.
    fn ticked(self) -> Vec<Filter> {
        SHELVES
            .into_iter()
            .filter(|shelf| self.has(*shelf))
            .collect()
    }

    fn has(self, shelf: Filter) -> bool {
        self.0 & Self::bit(shelf) != 0
    }

    fn toggled(self, shelf: Filter) -> Self {
        Self(self.0 ^ Self::bit(shelf))
    }

    /// The kinds listed: those ticked, or all of them when none is.
    fn shelves(self) -> Vec<Filter> {
        let ticked: Vec<Filter> = SHELVES
            .into_iter()
            .filter(|shelf| self.has(*shelf))
            .collect();
        if ticked.is_empty() {
            SHELVES.to_vec()
        } else {
            ticked
        }
    }
}

/// The kinds the list holds now. A folder opened into lists only what it
/// holds, whatever else is ticked.
fn listed_shelves(app: &App, shown: Shown) -> Vec<Filter> {
    let shelves = shown.shelves();
    if shelves.contains(&Filter::Playlists) && entered_span(app).is_some() {
        vec![Filter::Playlists]
    } else {
        shelves
    }
}

/// The orders every kind in `shelves` can take, for a Library of several.
fn mixed_sorts(shelves: &[Filter]) -> Vec<LibrarySort> {
    [
        LibrarySort::RecentlyPlayed,
        LibrarySort::Name,
        LibrarySort::RecentlyAdded,
    ]
    .into_iter()
    .filter(|sort| shelves.iter().all(|shelf| sort.supports(*shelf)))
    .collect()
}

/// The order of a Library of several kinds: the one chosen for such a
/// Library while they can all take it, else recently played.
fn mixed_sort(app: &App, shelves: &[Filter]) -> LibrarySort {
    app.settings
        .library_mixed_sort
        .filter(|sort| mixed_sorts(shelves).contains(sort))
        .unwrap_or(LibrarySort::RecentlyPlayed)
}

/// What one kind adds to the list, and whether it is still on its way.
#[derive(Default)]
struct ShelfStatus {
    loading: bool,
    error: Option<String>,
    more: Option<Page>,
}

/// The Library's rows for the kinds in `shelves`, in order.
#[derive(Default)]
struct LibraryRows {
    entries: Vec<Entry>,
    /// Where each kind after the first begins, when the kinds are kept
    /// together.
    group_starts: Vec<usize>,
    loading: bool,
    error: Option<String>,
    more_pages: Vec<Page>,
}

/// Rows of one kind, or of several: kept together a kind at a time, each
/// in the shared order, or mixed in one list in it, flat.
fn library_rows(app: &App, shelves: &[Filter], needle: &str) -> LibraryRows {
    let mut rows = LibraryRows::default();
    let mixed = shelves.len() > 1;
    let grouped = !mixed || app.settings.library_group_kinds;
    let sort_of = |shelf: Filter| {
        if mixed {
            mixed_sort(app, shelves)
        } else {
            selected_sort(app, shelf)
        }
    };
    for &shelf in shelves {
        let mut group = Vec::new();
        let status = shelf_rows(
            app,
            shelf,
            sort_of(shelf),
            needle,
            grouped,
            false,
            &mut group,
        );
        rows.loading |= status.loading;
        if rows.error.is_none() {
            rows.error = status.error;
        }
        rows.more_pages.extend(status.more);
        if grouped {
            order_entries(app, shelf, sort_of(shelf), &mut group);
            if !group.is_empty() && !rows.entries.is_empty() {
                rows.group_starts.push(rows.entries.len());
            }
        }
        rows.entries.extend(group);
    }
    if !grouped {
        order_entries(app, shelves[0], mixed_sort(app, shelves), &mut rows.entries);
    }
    rows
}

/// One thing on the Home map, in the order the Library lists it, held by
/// the last row before it that sits one `depth` higher.
pub(crate) struct MapRow {
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) image: Option<String>,
    pub(crate) round: bool,
    /// What a click opens; a folder or a kind's branch has no page.
    pub(crate) page: Option<Page>,
    /// A folder's rootlist id.
    pub(crate) folder: Option<String>,
    /// A kind's own branch, while the map holds more than one kind.
    pub(crate) group: Option<Filter>,
    pub(crate) depth: u8,
    pub(crate) songs: Option<u32>,
    /// Songs dropped on it are added: Liked Songs, and the playlists the
    /// account may change.
    pub(crate) takes_songs: bool,
    /// The Library's own choices list it: a ticked kind, By You and the
    /// search. A folder or branch is listed while anything in it is.
    pub(crate) listed: bool,
    /// The Library's own row, for its menu; a kind's branch has none.
    pub(crate) entry: Option<Entry>,
}

/// Whether a kind has anything to show without loading more.
fn shelf_loaded(app: &App, shelf: Filter) -> bool {
    match shelf {
        Filter::Playlists => true,
        Filter::Albums => !app.library.albums.items.is_empty(),
        Filter::Artists => !app.library.artists.items.is_empty(),
        Filter::Podcasts => !app.library.shows.items.is_empty(),
    }
}

/// The whole Library for the Home map, in the sidebar's order and with
/// every folder open: the kinds ticked, then any other kind already loaded,
/// each on a branch of its own. Ticked kinds kept together get a branch
/// each too, where more than one is ticked; mixed, they share the top in
/// one order.
pub(crate) fn map_rows(app: &App) -> Vec<MapRow> {
    let locale = app.locale;
    let listed = Shown::of(&app.settings.library_kinds).shelves();
    let others: Vec<Filter> = SHELVES
        .into_iter()
        .filter(|shelf| !listed.contains(shelf) && shelf_loaded(app, *shelf))
        .collect();
    let mixed = listed.len() > 1 && !app.settings.library_group_kinds;
    let branches = listed.len() > 1;
    let needle = app.library.filter.trim().to_lowercase();
    let mut rows = Vec::new();
    let block = |rows: &mut Vec<MapRow>,
                 shelves: &[Filter],
                 sort: LibrarySort,
                 ticked: bool,
                 branch: bool| {
        let start = rows.len();
        if branch {
            let shelf = shelves[0];
            rows.push(MapRow {
                key: format!("kind:{shelf:?}"),
                name: match shelf {
                    Filter::Playlists => gettext(locale, "Playlists"),
                    Filter::Albums => gettext(locale, "Albums"),
                    Filter::Artists => gettext(locale, "Artists"),
                    Filter::Podcasts => gettext(locale, "Podcasts"),
                }
                .into_owned(),
                image: None,
                round: false,
                page: None,
                folder: None,
                group: Some(shelf),
                depth: 0,
                songs: None,
                takes_songs: false,
                listed: false,
                entry: None,
            });
        }
        map_block(app, shelves, sort, &needle, ticked, u8::from(branch), rows);
        if branch {
            rows[start].listed = rows[start + 1..].iter().any(|row| row.listed);
        }
    };
    if mixed {
        block(&mut rows, &listed, mixed_sort(app, &listed), true, false);
    } else {
        // Kinds kept together share the order of a Library of several, as
        // the sidebar lists them.
        for &shelf in &listed {
            let sort = if branches {
                mixed_sort(app, &listed)
            } else {
                selected_sort(app, shelf)
            };
            block(&mut rows, &[shelf], sort, true, branches);
        }
    }
    for &shelf in &others {
        block(&mut rows, &[shelf], selected_sort(app, shelf), false, true);
    }
    rows
}

/// The map's rows of `shelves`, `base` levels down, ordered as the sidebar
/// orders them: a tree order as it is, any other with each folder taking
/// the place of the first row it holds.
fn map_block(
    app: &App,
    shelves: &[Filter],
    sort: LibrarySort,
    needle: &str,
    ticked: bool,
    base: u8,
    rows: &mut Vec<MapRow>,
) {
    let mut tree = Vec::new();
    for &shelf in shelves {
        shelf_rows(app, shelf, LibrarySort::Spotify, "", true, true, &mut tree);
    }
    if shelves == [Filter::Playlists] && matches!(sort, LibrarySort::Spotify | LibrarySort::Name) {
        order_entries(app, Filter::Playlists, sort, &mut tree);
    } else {
        let mut flat = Vec::new();
        for &shelf in shelves {
            shelf_rows(app, shelf, sort, "", false, true, &mut flat);
        }
        order_entries(app, shelves[0], sort, &mut flat);
        let ranks = first_ranks(flat.iter().map(Entry::ordering_key));
        let order = tree_order(&tree, |entry| {
            ranks
                .get(entry.ordering_key())
                .copied()
                .unwrap_or(usize::MAX)
        });
        permute(&mut tree, &order);
    }
    // The search matches as the sidebar's does, an album by its artists too.
    let found: Option<std::collections::HashSet<String>> = (!needle.is_empty()).then(|| {
        let mut found = Vec::new();
        for &shelf in shelves {
            shelf_rows(app, shelf, sort, needle, false, true, &mut found);
        }
        found
            .iter()
            .map(|entry| entry.ordering_key().to_string())
            .collect()
    });
    let by_you = app.settings.library_by_you;
    let playlists = app.library.playlists.get();
    let start = rows.len();
    for entry in tree {
        let songs = if entry.liked {
            app.library.liked.total
        } else {
            entry
                .playlist_index
                .and_then(|index| playlists?.get(index))
                .map(crate::api::models::Playlist::track_total)
        };
        let listed = ticked
            && !(by_you && entry.playlist_index.is_some() && !entry.owned)
            && found
                .as_ref()
                .is_none_or(|found| found.contains(entry.ordering_key()));
        let page = entry.folder.is_none().then(|| entry.page.clone());
        rows.push(MapRow {
            key: match (&entry.folder, &entry.page) {
                (Some((id, _, _)), _) => format!("folder:{id}"),
                (None, Page::LocalFiles) => "local-files".to_string(),
                _ => entry.ordering_key().to_string(),
            },
            takes_songs: entry.liked || entry.editable,
            folder: entry.folder.as_ref().map(|(id, _, _)| id.clone()),
            name: entry.name.clone(),
            image: entry.image.clone(),
            round: entry.round,
            page,
            group: None,
            depth: base + entry.depth,
            songs,
            listed,
            entry: Some(entry),
        });
    }
    // A folder is listed while anything inside it is: walking back, each
    // depth remembers whether a listed row lay below it.
    let mut below = vec![false; 258];
    for row in rows[start..].iter_mut().rev() {
        let depth = usize::from(row.depth);
        if row.folder.is_some() {
            row.listed = below[depth + 1];
            below[depth + 1..].iter_mut().for_each(|seen| *seen = false);
        }
        if row.listed {
            below[..=depth].iter_mut().for_each(|seen| *seen = true);
        }
    }
}

/// The order that puts each folder's rows by `rank`, a folder ranked by the
/// first of what it holds, keeping every row under its folder.
fn tree_order(entries: &[Entry], rank: impl Fn(&Entry) -> usize) -> Vec<usize> {
    let top = entries.len();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); top + 1];
    let mut open: Vec<(u8, usize)> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        while open.last().is_some_and(|(depth, _)| *depth >= entry.depth) {
            open.pop();
        }
        children[open.last().map_or(top, |(_, folder)| *folder)].push(index);
        if entry.folder.is_some() {
            open.push((entry.depth, index));
        }
    }
    let mut best: Vec<usize> = entries.iter().map(rank).collect();
    best.push(usize::MAX);
    // Rows come after the folder that holds them, so walking back meets
    // every row before its folder.
    for index in (0..top).rev() {
        if let Some(first) = children[index].iter().map(|child| best[*child]).min() {
            best[index] = best[index].min(first);
        }
    }
    for list in &mut children {
        list.sort_by_key(|child| best[*child]);
    }
    let mut order = Vec::with_capacity(top);
    let mut stack: Vec<usize> = children[top].iter().rev().copied().collect();
    while let Some(index) = stack.pop() {
        order.push(index);
        stack.extend(children[index].iter().rev().copied());
    }
    order
}

/// Keeps a kind loading while its first page has not come, and, ordered
/// other than as Spotify lists it, until all of it has.
fn keep_shelf_loading(app: &mut App, shelf: Filter, sort: LibrarySort) {
    let (list, page) = match shelf {
        Filter::Playlists => return,
        Filter::Albums => (
            (
                app.library.albums.loading,
                app.library.albums.error.is_none(),
                app.library.albums.loaded_once,
                app.library.albums.can_load_more(),
            ),
            Page::Albums,
        ),
        Filter::Artists => (
            (
                app.library.artists.loading,
                app.library.artists.error.is_none(),
                app.library.artists.loaded_once,
                app.library.artists.can_load_more(),
            ),
            Page::Artists,
        ),
        Filter::Podcasts => (
            (
                app.library.shows.loading,
                app.library.shows.error.is_none(),
                app.library.shows.loaded_once,
                app.library.shows.can_load_more(),
            ),
            Page::Podcasts,
        ),
    };
    let (loading, ok, loaded_once, more) = list;
    if !loading && ok && (!loaded_once || (sort != LibrarySort::Library && more)) {
        app.actions.push(Action::LoadMore(page));
    }
}

/// The rows of one kind, unordered; playlists keep their folders where
/// `tree` lets them.
fn shelf_rows(
    app: &App,
    shelf: Filter,
    sort: LibrarySort,
    needle: &str,
    tree: bool,
    whole: bool,
    entries: &mut Vec<Entry>,
) -> ShelfStatus {
    let locale = app.locale;
    let art = EntryArt::of(app);
    let mut status = ShelfStatus::default();
    match shelf {
        Filter::Playlists => {
            playlist_rows_with(app, sort, needle, tree, whole, entries);
            match &app.library.playlists {
                Loadable::Loaded(_) => {}
                Loadable::Loading | Loadable::NotLoaded => status.loading = true,
                Loadable::Failed(message) => status.error = Some(message.clone()),
            }
        }
        Filter::Albums => {
            for saved in &app.library.albums.items {
                let album = &saved.album;
                if !needle.is_empty()
                    && !album.name.to_lowercase().contains(needle)
                    && !album
                        .artists
                        .iter()
                        .any(|a| a.name.to_lowercase().contains(needle))
                {
                    continue;
                }
                let artists = crate::api::models::join_names(
                    album.artists.iter().map(|artist| artist.name.as_str()),
                );
                entries.push(Entry {
                    image: pick_image(&album.images, art.row).map(str::to_string),
                    grid_image: pick_image(&album.images, art.grid).map(str::to_string),
                    name: album.name.clone(),
                    subtitle: format!("{} • {artists}", app.album_kind_label(album)),
                    grid_subtitle: artists,
                    page: Page::Album(album.id.clone()),
                    uri: album.uri.clone(),
                    round: false,
                    liked: false,
                    owned: false,
                    editable: false,
                    playlist_index: None,
                    folder: None,
                    depth: 0,
                    added_at: saved_time(saved.added_at.as_deref()),
                    pin_key: None,
                });
            }
            status.loading = app.library.albums.loading && app.library.albums.items.is_empty();
            status.error = app.library.albums.error.clone();
            if app.library.albums.error.is_none() && app.library.albums.can_load_more() {
                status.more = Some(Page::Albums);
            }
        }
        Filter::Artists => {
            for artist in &app.library.artists.items {
                if !needle.is_empty() && !artist.name.to_lowercase().contains(needle) {
                    continue;
                }
                entries.push(Entry {
                    image: pick_image(&artist.images, art.row).map(str::to_string),
                    grid_image: pick_image(&artist.images, art.grid).map(str::to_string),
                    name: artist.name.clone(),
                    subtitle: gettext(locale, "Artist").into_owned(),
                    grid_subtitle: String::new(),
                    page: Page::Artist(artist.id.clone()),
                    uri: artist.uri.clone(),
                    round: true,
                    liked: false,
                    owned: false,
                    editable: false,
                    playlist_index: None,
                    folder: None,
                    depth: 0,
                    added_at: None,
                    pin_key: None,
                });
            }
            status.loading = app.library.artists.loading && app.library.artists.items.is_empty();
            status.error = app.library.artists.error.clone();
            if app.library.artists.error.is_none() && app.library.artists.can_load_more() {
                status.more = Some(Page::Artists);
            }
        }
        Filter::Podcasts => {
            for saved in &app.library.shows.items {
                let show = &saved.show;
                // Audiobooks arrive as shows, but librespot can't play them.
                if app.audiobook_shows.contains(&show.uri) {
                    continue;
                }
                if !needle.is_empty() && !show.name.to_lowercase().contains(needle) {
                    continue;
                }
                entries.push(Entry {
                    image: pick_image(&show.images, art.row).map(str::to_string),
                    grid_image: pick_image(&show.images, art.grid).map(str::to_string),
                    name: show.name.clone(),
                    // Translators: {publisher} is the podcast's publisher.
                    subtitle: gettext(locale, "Podcast • {publisher}")
                        .replace("{publisher}", &show.publisher),
                    grid_subtitle: show.publisher.clone(),
                    page: Page::Show(show.id.clone()),
                    uri: show.uri.clone(),
                    round: false,
                    liked: false,
                    owned: false,
                    editable: false,
                    playlist_index: None,
                    folder: None,
                    depth: 0,
                    added_at: saved_time(saved.added_at.as_deref()),
                    pin_key: None,
                });
            }
            status.loading = app.library.shows.loading && app.library.shows.items.is_empty();
            status.error = app.library.shows.error.clone();
            if app.library.shows.error.is_none() && app.library.shows.can_load_more() {
                status.more = Some(Page::Podcasts);
            }
        }
    }
    status
}

pub(crate) fn selected_sort(app: &App, shelf: Filter) -> LibrarySort {
    if let Some(sort) = app.settings.library_sort.get(&shelf).copied()
        && sort.supports(shelf)
    {
        return sort;
    }
    if shelf != Filter::Playlists {
        LibrarySort::Library
    } else if !app.settings.sidebar_order.is_empty() {
        LibrarySort::Local
    } else if app
        .rootlist
        .iter()
        .any(|row| matches!(row, crate::player::RootlistEntry::FolderStart { .. }))
    {
        LibrarySort::Spotify
    } else {
        LibrarySort::RecentlyPlayed
    }
}

/// Offer only the sort modes supported by this shelf and queue the selected change.
fn sort_menu(app: &mut App, ui: &mut egui::Ui, shelf: Filter, selected: LibrarySort) {
    let locale = app.locale;
    let labels = [
        (LibrarySort::Library, gettext(locale, "Library order")),
        (
            LibrarySort::RecentlyPlayed,
            gettext(locale, "Recently played"),
        ),
        (LibrarySort::Name, gettext(locale, "Name")),
        (
            LibrarySort::RecentlyAdded,
            gettext(locale, "Recently added"),
        ),
        (LibrarySort::Local, gettext(locale, "Local custom order")),
        (
            LibrarySort::Spotify,
            gettext(locale, "Spotify custom order"),
        ),
    ];
    let label = &labels
        .iter()
        .find(|(sort, _)| *sort == selected)
        .expect("sort label")
        .1;
    let response = ui.add(
        egui::Button::image_and_text(
            Icon::ChevronDown.image(app.palette.text, 15.0),
            egui::RichText::new(label.as_ref()).font(theme::medium(13.0)),
        )
        .wrap()
        .fill(app.palette.surface)
        .corner_radius(12)
        .min_size(vec2(0.0, 28.0)),
    );
    egui::Popup::menu(&response)
        .frame(super::widgets::menu_frame(&app.palette))
        .show(|ui| {
            let width = labels
                .iter()
                .map(|(_, label)| {
                    ui.painter()
                        .layout_no_wrap(label.to_string(), theme::regular(13.5), app.palette.text)
                        .size()
                        .x
                })
                .fold(140.0_f32, f32::max)
                + 52.0;
            ui.set_width(width.min(ui.ctx().content_rect().width() - 24.0));
            for (sort, label) in &labels {
                if !sort.supports(shelf)
                    || (*sort == LibrarySort::Library && shelf == Filter::Playlists)
                    || (*sort == LibrarySort::Local && app.settings.sidebar_order.is_empty())
                {
                    continue;
                }
                if super::widgets::menu_item(
                    ui,
                    &app.palette,
                    (*sort == selected).then_some(Icon::Check),
                    label,
                ) {
                    app.actions
                        .push(Action::SetLibrarySort { shelf, sort: *sort });
                }
            }
        });
}

/// The order menu of a Library of several kinds: the orders they all
/// take, then whether to keep each kind together or mix them.
fn mixed_sort_menu(app: &mut App, ui: &mut egui::Ui, shelves: &[Filter], selected: LibrarySort) {
    let locale = app.locale;
    let label = |sort: LibrarySort| match sort {
        LibrarySort::Name => gettext(locale, "Name"),
        LibrarySort::RecentlyAdded => gettext(locale, "Recently added"),
        _ => gettext(locale, "Recently played"),
    };
    let grouped = app.settings.library_group_kinds;
    let response = ui.add(
        egui::Button::image_and_text(
            Icon::ChevronDown.image(app.palette.text, 15.0),
            egui::RichText::new(label(selected).as_ref()).font(theme::medium(13.0)),
        )
        .wrap()
        .fill(app.palette.surface)
        .corner_radius(12)
        .min_size(vec2(0.0, 28.0)),
    );
    egui::Popup::menu(&response)
        .frame(super::widgets::menu_frame(&app.palette))
        .show(|ui| {
            // Translators: Library order menu, when it lists more than one kind: keep playlists, albums, artists and podcasts each together.
            let group = gettext(locale, "Group by kind");
            // Translators: Library order menu, when it lists more than one kind: list playlists, albums, artists and podcasts in one list.
            let mix = gettext(locale, "Mix together");
            let sorts = mixed_sorts(shelves);
            let width = sorts
                .iter()
                .map(|sort| label(*sort))
                .chain([group.clone(), mix.clone()])
                .map(|label| {
                    ui.painter()
                        .layout_no_wrap(label.to_string(), theme::regular(13.5), app.palette.text)
                        .size()
                        .x
                })
                .fold(140.0_f32, f32::max)
                + 52.0;
            ui.set_width(width.min(ui.ctx().content_rect().width() - 24.0));
            for sort in sorts {
                if super::widgets::menu_item(
                    ui,
                    &app.palette,
                    (sort == selected).then_some(Icon::Check),
                    &label(sort),
                ) {
                    app.actions.push(Action::SetLibraryMixedSort(sort));
                }
            }
            ui.separator();
            for (keep, text) in [(true, &group), (false, &mix)] {
                if super::widgets::menu_item(
                    ui,
                    &app.palette,
                    (keep == grouped).then_some(Icon::Check),
                    text,
                ) {
                    app.actions.push(Action::SetLibraryGroupKinds(keep));
                }
            }
        });
}

/// The hairline above the first row of each kind after the first.
fn group_line(ui: &egui::Ui, palette: &Palette, x: egui::Rangef, y: f32) {
    ui.painter()
        .hline(x.shrink(6.0), y, egui::Stroke::new(0.5, palette.outline));
}

fn saved_time(value: Option<&str>) -> Option<i64> {
    value
        .and_then(|text| text.parse::<jiff::Timestamp>().ok())
        .map(|time| time.as_millisecond())
}

/// Build the ranks once per frame, keeping the first occurrence just as
/// `position` did. Searching the whole order from a sort comparator makes
/// scrolling a large library quadratic even when only visible rows are drawn.
fn first_ranks<'a>(
    keys: impl Iterator<Item = &'a str>,
) -> std::collections::HashMap<&'a str, usize> {
    let mut ranks = std::collections::HashMap::new();
    for (rank, key) in keys.enumerate() {
        ranks.entry(key).or_insert(rank);
    }
    ranks
}

/// Applies the selected library order, then places pins first in their saved
/// order. New playlists precede a saved local arrangement, and pinned playlists
/// are shown at the top level rather than inside their folders.
fn order_entries(app: &App, shelf: Filter, sort: LibrarySort, entries: &mut [Entry]) {
    match sort {
        LibrarySort::Name => {
            // Each row sorts under the folders that hold it: its key is the
            // path of names down to it, so a folder's rows follow the folder
            // in name order and the folders sort among their siblings.
            let mut folders: Vec<(String, String)> = Vec::new();
            let paths: Vec<Vec<(String, String)>> = entries
                .iter()
                .map(|entry| {
                    folders.truncate(entry.depth as usize);
                    let key = (
                        entry.name.to_lowercase(),
                        match &entry.folder {
                            Some((id, _, _)) => id.clone(),
                            None => entry.uri.clone(),
                        },
                    );
                    let mut path = folders.clone();
                    path.push(key.clone());
                    if entry.folder.is_some() {
                        folders.push(key);
                    }
                    path
                })
                .collect();
            let mut order: Vec<usize> = (0..entries.len()).collect();
            order.sort_by(|a, b| paths[*a].cmp(&paths[*b]));
            permute(entries, &order);
        }
        LibrarySort::RecentlyPlayed => {
            let ranks = first_ranks(app.recent_contexts.iter().map(String::as_str));
            let liked_uri = app
                .user_id()
                .map(|id| format!("spotify:user:{id}:collection"));
            entries.sort_by_cached_key(|entry| {
                let key = if entry.liked {
                    liked_uri.as_deref()
                } else {
                    Some(entry.uri.as_str())
                };
                key.and_then(|key| ranks.get(key))
                    .copied()
                    .unwrap_or(usize::MAX)
            });
        }
        LibrarySort::RecentlyAdded => entries
            .sort_by_key(|entry| (entry.added_at.is_none(), std::cmp::Reverse(entry.added_at))),
        LibrarySort::Local => {
            let ranks = first_ranks(app.settings.sidebar_order.iter().map(String::as_str));
            entries.sort_by_cached_key(|entry| match ranks.get(entry.ordering_key()) {
                Some(&rank) => (1, rank),
                None => (0, entry.playlist_index.unwrap_or(0)),
            });
        }
        LibrarySort::Spotify if !entries.iter().any(|entry| entry.folder.is_some()) => {
            let ranks = first_ranks(app.rootlist.iter().filter_map(|row| match row {
                crate::player::RootlistEntry::Playlist(uri) => Some(uri.as_str()),
                _ => None,
            }));
            // Spotify's custom order puts Liked Songs ahead of the playlist
            // tree, after the pins.
            entries.sort_by_cached_key(|entry| {
                (
                    !entry.liked,
                    entry.page != Page::LocalFiles,
                    ranks.get(entry.uri.as_str()).copied().unwrap_or(usize::MAX),
                )
            });
        }
        LibrarySort::Spotify => {
            entries.sort_by_key(|entry| (!entry.liked, entry.page != Page::LocalFiles));
        }
        LibrarySort::Library => {}
    }
    let pins = app.library_pins();
    let ranks = first_ranks(pins.iter().map(String::as_str));
    // A pinned folder takes what it holds up with it: the rows after it
    // that sit deeper share its rank.
    let mut block: Option<(u8, usize)> = None;
    let mut order = Vec::with_capacity(entries.len());
    for entry in entries.iter() {
        if let Some((depth, rank)) = block
            && entry.depth > depth
        {
            order.push(rank);
            continue;
        }
        block = None;
        let rank = ranks
            .get(entry.ordering_key())
            .copied()
            .unwrap_or(usize::MAX);
        if entry.folder.is_some() && rank != usize::MAX {
            block = Some((entry.depth, rank));
        }
        order.push(rank);
    }
    sort_by_ranks(entries, &order);
    if shelf == Filter::Playlists {
        for entry in entries {
            if entry.folder.is_none() && ranks.contains_key(entry.ordering_key()) {
                entry.depth = 0;
            }
        }
    }
}

/// Puts `entries[order[i]]` at position `i`.
fn permute<T>(entries: &mut [T], order: &[usize]) {
    let mut place: Vec<usize> = vec![0; order.len()];
    for (to, &from) in order.iter().enumerate() {
        place[from] = to;
    }
    for start in 0..entries.len() {
        while place[start] != start {
            let to = place[start];
            entries.swap(start, to);
            place.swap(start, to);
        }
    }
}

/// Puts `entries` in the order of `ranks`, keeping equals in their order.
fn sort_by_ranks<T>(entries: &mut [T], ranks: &[usize]) {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&index| ranks[index]);
    permute(entries, &order);
}

/// How many rows at the top belong to pins, a pinned folder's rows with it.
fn pinned_rows(entries: &[Entry], pins: &[String]) -> usize {
    let mut block: Option<u8> = None;
    let mut count = 0;
    for entry in entries {
        if block.is_some_and(|depth| entry.depth > depth) {
            count += 1;
            continue;
        }
        block = None;
        if !pins.iter().any(|key| key == entry.ordering_key()) {
            break;
        }
        if entry.folder.is_some() {
            block = Some(entry.depth);
        }
        count += 1;
    }
    count
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let expanded_art = has_expanded_art(app);
    let map = app.settings.sidebar_map;
    let floating_art = app.settings.sidebar_grid && !map && expanded_art;
    // The map takes a panel of its own, wider, with a width of its own.
    let (id, widest, chosen) = if map {
        ("sidebar-map", 1400.0, app.settings.sidebar_map_width)
    } else {
        ("sidebar", 600.0, app.settings.sidebar_width)
    };
    // The traffic lights float over the top-left of the sidebar now, so the
    // first nav row has to start below them.
    let top = 12
        + theme::titlebar_inset(ui.ctx()) as i8
        + super::linux_left_controls_inset(ui.ctx()) as i8;
    let beside = if app.show_queue_panel || app.show_lyrics_panel || app.show_friends_panel {
        theme::SIDE_PANEL_MIN_WIDTH
    } else {
        0.0
    };
    let fit = super::yielding_panel(
        ui.ctx(),
        id,
        super::SIDEBAR_MIN_WIDTH..=widest,
        chosen,
        ui.available_width() - super::topbar::least_width(ui.ctx()) - beside,
    );
    let panel = egui::Panel::left(id)
        .resizable(true)
        .default_size(chosen)
        .size_range(fit.range.clone())
        .show_separator_line(false)
        .frame(Frame::new().fill(palette.panel).inner_margin(Margin {
            left: 12,
            right: 8,
            top,
            bottom: if expanded_art { 0 } else { 8 },
        }));
    let response = panel.show(ui, |ui| {
        let art_rect = expanded_art.then(|| expanded_art_rect(ui));
        if let Some(rect) = art_rect.filter(|_| !floating_art) {
            reserve_expanded_art(ui, rect);
        }
        contents(app, ui, art_rect.filter(|_| floating_art));
        if let Some(rect) = art_rect {
            if floating_art {
                paint_grid_art_mask(app, ui, rect);
            }
            paint_expanded_art(app, ui, rect);
        }
    });
    let width = response.response.rect.width();
    if (width - chosen).abs() > 1.0 && super::panel_width_chosen(ui.ctx(), id, &fit) {
        if map {
            app.settings.sidebar_map_width = width;
        } else {
            app.settings.sidebar_width = width;
        }
        app.actions.push(Action::SettingsChanged);
    }
}

fn expanded_art_rect(ui: &egui::Ui) -> Rect {
    let side = expanded_art_side(ui);
    Rect::from_min_size(
        pos2(
            ui.max_rect().left(),
            ui.max_rect().bottom() - EXPANDED_ART_GAP - side,
        ),
        Vec2::splat(side),
    )
}

fn reserve_expanded_art(ui: &mut egui::Ui, rect: Rect) {
    egui::Panel::bottom("sidebar-art-space")
        .exact_size(rect.height() + EXPANDED_ART_GAP)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::new())
        .show(ui, |_| {});
}

/// In grid mode content continues behind the shared fixed artwork.
fn paint_grid_art_mask(app: &App, ui: &egui::Ui, rect: Rect) {
    let bottom_fade_rect = Rect::from_min_max(
        pos2(ui.max_rect().left(), rect.bottom() - 64.0),
        pos2(ui.max_rect().right(), rect.bottom()),
    );
    super::widgets::paint_vertical_gradient(
        ui,
        bottom_fade_rect,
        egui::Color32::TRANSPARENT,
        app.palette.panel,
    );
    ui.painter().rect_filled(
        Rect::from_min_max(
            pos2(ui.max_rect().left(), rect.bottom()),
            ui.max_rect().right_bottom(),
        ),
        0.0,
        app.palette.panel,
    );
}

fn has_expanded_art(app: &App) -> bool {
    app.settings.art_expanded
        && app
            .now_playing()
            .is_some_and(|now| now.art_url.is_some() || now.art_small.is_some())
}

fn expanded_art_side(ui: &egui::Ui) -> f32 {
    ui.max_rect()
        .width()
        .min(ui.max_rect().height() * 0.45)
        .max(80.0)
}

fn paint_expanded_art(app: &mut App, ui: &mut egui::Ui, rect: Rect) {
    let Some(now) = app.now_playing() else {
        return;
    };
    let Some(url) = now.art_url.clone().or_else(|| now.art_small.clone()) else {
        return;
    };
    let album_id = now.album_id.clone();
    let show_id = now.show_id.clone();
    let palette = app.palette;
    ui.painter().add(
        egui::epaint::Shadow {
            offset: [0, 0],
            blur: 28,
            spread: 0,
            color: egui::Color32::from_black_alpha(if palette.dark { 120 } else { 40 }),
        }
        .as_shape(rect, egui::CornerRadius::same(8)),
    );
    super::widgets::playing_cover(
        ui,
        &palette,
        app.backend.art(),
        "sidebar-art",
        Some(&url),
        rect,
        8.0,
    );
    let art = ui.interact(rect, egui::Id::new("sidebar-art"), Sense::click());
    let chevron_rect = Rect::from_center_size(
        pos2(rect.right() - 16.0, rect.top() + 16.0),
        Vec2::splat(20.0),
    );
    let over_chevron = ui.rect_contains_pointer(chevron_rect);
    if art.hovered() || over_chevron {
        let chevron = ui.interact(
            chevron_rect,
            egui::Id::new("sidebar-art-collapse"),
            Sense::click(),
        );
        ui.painter().circle_filled(
            chevron_rect.center(),
            10.0,
            palette.panel.gamma_multiply(0.9),
        );
        Icon::ChevronDown.image(palette.text, 14.0).paint_at(
            ui,
            Rect::from_center_size(chevron_rect.center(), Vec2::splat(14.0)),
        );
        if chevron.clicked() {
            app.settings.art_expanded = false;
            app.actions.push(Action::SettingsChanged);
        }
    }
    if art.clicked() && !over_chevron {
        if let Some(id) = album_id {
            app.actions.push(Action::Open(Page::Album(id)));
        } else if let Some(id) = show_id {
            app.actions.push(Action::Open(Page::Show(id)));
        }
    }
}

/// Project the playlist shelf without changing its saved order or pins.
/// Ownership filtering is flat, like text search, so matching playlists in
/// collapsed folders remain reachable. Liked Songs belongs to the account.
#[cfg(test)]
fn playlist_rows(app: &App, sort: LibrarySort, needle: &str, entries: &mut Vec<Entry>) {
    playlist_rows_with(app, sort, needle, true, false, entries);
}

/// The playlist rows, in Spotify's folders where `tree` keeps them and the
/// order shows them, or flat. `whole` lists the whole Library, as the map
/// does: no folder opened or rolled up, and By You leaving nothing out.
fn playlist_rows_with(
    app: &App,
    sort: LibrarySort,
    needle: &str,
    tree: bool,
    whole: bool,
    entries: &mut Vec<Entry>,
) {
    let entered = if whole { None } else { entered_span(app) };
    let liked = liked_entry(app);
    if entered.is_none() && (needle.is_empty() || liked.name.to_lowercase().contains(needle)) {
        entries.push(liked);
    }
    // Local Files, once there is a folder or song to play from.
    if entered.is_none() && !app.local_sources().is_empty() {
        let local = local_files_entry(app);
        if needle.is_empty() || local.name.to_lowercase().contains(needle) {
            entries.push(local);
        }
    }
    let user_id = app.user_id().filter(|id| !id.is_empty());
    // Name keeps playlists inside their folders, as Spotify does, and sorts
    // each folder on its own (#651).
    let folders = tree
        && match sort {
            LibrarySort::Spotify => true,
            LibrarySort::Name => app
                .rootlist
                .iter()
                .any(|row| matches!(row, crate::player::RootlistEntry::FolderStart { .. })),
            _ => false,
        };
    if folders && needle.is_empty() {
        folder_rows_in(app, user_id.unwrap_or(""), whole, entries);
    } else if let Some(playlists) = app.library.playlists.get() {
        // Opened into a folder, a flat list holds only what the folder holds,
        // in folders inside it too.
        let inside: Option<std::collections::HashSet<&str>> = entered.map(|(start, end)| {
            app.rootlist[start..=end]
                .iter()
                .filter_map(|row| match row {
                    crate::player::RootlistEntry::Playlist(uri) => Some(uri.as_str()),
                    _ => None,
                })
                .collect()
        });
        for (index, playlist) in playlists.iter().enumerate() {
            if inside
                .as_ref()
                .is_some_and(|inside| !inside.contains(playlist.uri.as_str()))
            {
                continue;
            }
            if !whole
                && app.settings.library_by_you
                && !user_id.is_some_and(|id| playlist.owned_by(id))
            {
                continue;
            }
            if !needle.is_empty() && !playlist.name.to_lowercase().contains(needle) {
                continue;
            }
            entries.push(playlist_entry(
                app.locale,
                playlist,
                index,
                user_id.unwrap_or(""),
                app.can_edit_playlist(playlist),
                0,
                EntryArt::of(app),
            ));
        }
    }
}

/// Playlist rows in account order, including collapsible folders (#95).
/// With By You on, the tree keeps its shape and its open and closed
/// folders but holds only the account's own playlists; a folder holding
/// none of them leaves the view.
#[cfg(test)]
fn folder_rows(app: &App, user_id: &str, entries: &mut Vec<Entry>) {
    folder_rows_in(app, user_id, false, entries);
}

/// [`folder_rows`], or with `whole` the whole tree: every folder open, none
/// entered, and By You leaving nothing out.
fn folder_rows_in(app: &App, user_id: &str, whole: bool, entries: &mut Vec<Entry>) {
    use crate::player::RootlistEntry;
    let Some(playlists) = app.library.playlists.get() else {
        return;
    };
    let by_uri: std::collections::HashMap<&str, (usize, &crate::api::models::Playlist)> = playlists
        .iter()
        .enumerate()
        .map(|(index, playlist)| (playlist.uri.as_str(), (index, playlist)))
        .collect();
    let by_you = app.settings.library_by_you && !whole;
    let keep = |uri: &str| {
        !by_you
            || by_uri
                .get(uri)
                .is_some_and(|(_, playlist)| playlist.owned_by(user_id))
    };
    let counts = folder_counts(&app.rootlist, keep);
    let holdings = folder_holdings(&app.rootlist);
    let pins = app.library_pins();
    // Opened into a folder, only what it holds is listed, from its own rows
    // down, whether or not it is rolled up in the tree.
    let entered = if whole { None } else { entered_span(app) };
    let rows = match entered {
        Some((start, end)) => &app.rootlist[start + 1..end],
        None => &app.rootlist[..],
    };
    let mut depth = 0u8;
    // Rows inside a rolled-up folder stay off the list; the stack knows
    // how deep the rolled-up one sits.
    let mut hidden_from: Option<u8> = None;
    let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for row in rows {
        match row {
            RootlistEntry::FolderStart { id, name } => {
                // The grid has no arrows: a folder there is a card that
                // opens, its contents inside it, as in Spotify's grid.
                let collapsed =
                    !whole && (app.settings.sidebar_grid || app.collapsed_folders.contains(id));
                let count = counts.get(id.as_str()).copied().unwrap_or(0);
                let (own_playlists, own_folders) =
                    holdings.get(id.as_str()).copied().unwrap_or_default();
                if by_you && count == 0 && hidden_from.is_none() {
                    // Nothing of the account's inside: the folder and all it
                    // holds stay off the list.
                    hidden_from = Some(depth);
                } else if hidden_from.is_none() {
                    entries.push(Entry {
                        image: None,
                        grid_image: None,
                        name: if name.is_empty() {
                            gettext(app.locale, "Folder").into_owned()
                        } else {
                            name.clone()
                        },
                        // What the folder holds itself, as Spotify counts
                        // it: its own playlists and folders.
                        subtitle: format!(
                            "{} \u{2022} {}",
                            gettext(app.locale, "Folder"),
                            app.locale
                                .folder_contents(own_playlists as u32, own_folders as u32)
                        ),
                        grid_subtitle: app
                            .locale
                            .folder_contents(own_playlists as u32, own_folders as u32),
                        page: Page::Home,
                        uri: String::new(),
                        round: false,
                        liked: false,
                        owned: false,
                        editable: false,
                        playlist_index: None,
                        folder: Some((id.clone(), collapsed, count)),
                        depth,
                        added_at: None,
                        pin_key: (!user_id.is_empty())
                            .then(|| crate::pins::folder_uri(user_id, id)),
                    });
                    if collapsed {
                        hidden_from = Some(depth);
                    }
                }
                depth += 1;
            }
            RootlistEntry::FolderEnd => {
                depth = depth.saturating_sub(1);
                if hidden_from == Some(depth) {
                    hidden_from = None;
                }
            }
            RootlistEntry::Playlist(uri) => {
                let Some((index, playlist)) = by_uri.get(uri.as_str()) else {
                    continue;
                };
                if !seen.insert(uri.as_str()) {
                    continue;
                }
                if !keep(uri) {
                    continue;
                }
                if hidden_from.is_some() && !pins.contains(uri) {
                    continue;
                }
                entries.push(playlist_entry(
                    app.locale,
                    playlist,
                    *index,
                    user_id,
                    app.can_edit_playlist(playlist),
                    depth,
                    EntryArt::of(app),
                ));
            }
        }
    }
    // Playlists the rootlist has not met yet, the newly followed, wait at
    // the end rather than vanish; they are in no folder.
    for (index, playlist) in playlists.iter().enumerate() {
        if entered.is_none() && !seen.contains(playlist.uri.as_str()) && keep(&playlist.uri) {
            entries.push(playlist_entry(
                app.locale,
                playlist,
                index,
                user_id,
                app.can_edit_playlist(playlist),
                0,
                EntryArt::of(app),
            ));
        }
    }
}

/// The rows of the folder the playlists are opened into, its markers
/// included; `None` at the top level, or once that folder is gone.
fn entered_span(app: &App) -> Option<(usize, usize)> {
    let id = app.library_folder.as_ref()?;
    crate::folders::folder_span(&app.rootlist, id)
}

/// Where a drop below the last row goes: the end of the library, or, in an
/// opened folder, the end of that folder. A folder whose last row is
/// another folder takes the drop just before that one.
fn end_place(app: &App) -> Place {
    use crate::player::RootlistEntry;
    let Some((start, end)) = entered_span(app) else {
        return Place::End;
    };
    match &app.rootlist[end - 1] {
        RootlistEntry::Playlist(uri) => Place::AfterPlaylist(uri.clone()),
        RootlistEntry::FolderEnd => crate::folders::folders(&app.rootlist[start + 1..end])
            .into_iter()
            .rev()
            .find(|folder| {
                crate::folders::folder_span(&app.rootlist, &folder.id)
                    .is_some_and(|(_, last)| last == end - 1)
            })
            .map_or_else(
                || inside_opened(app, start),
                |folder| Place::Before(Row::Folder(folder.id)),
            ),
        RootlistEntry::FolderStart { .. } => inside_opened(app, start),
    }
}

/// First inside the opened folder whose start marker is at `start`.
fn inside_opened(app: &App, start: usize) -> Place {
    match &app.rootlist[start] {
        RootlistEntry::FolderStart { id, .. } => Place::InFolder(id.clone()),
        _ => Place::End,
    }
}

/// The slot a drop line shows: where the row will land. Below an opened
/// folder that ends in a folder, Spotify's anchors place a playlist just
/// above that folder, so the line shows there.
fn landing_slot(app: &App, entries: &[Entry], slot: usize) -> usize {
    if slot != entries.len() || !shows_spotify_tree(app) {
        return slot;
    }
    // A pinned folder sits in the pin block, not where the tree has it;
    // the drop then stays at the end.
    let pinned = pinned_rows(entries, &app.library_pins());
    match end_place(app) {
        Place::Before(Row::Folder(id)) => entries
            .iter()
            .skip(pinned)
            .position(|entry| {
                entry
                    .folder
                    .as_ref()
                    .is_some_and(|(held, _, _)| *held == id)
            })
            .map_or(slot, |at| pinned + at),
        _ => slot,
    }
}

/// The folder the Playlists shelf is opened into, while it still exists.
fn opened_folder(app: &App, filter: Filter) -> Option<String> {
    (filter == Filter::Playlists && entered_span(app).is_some())
        .then(|| app.library_folder.clone())
        .flatten()
}

/// Where Back leads from an opened folder: the folder around it, or the
/// whole library.
fn way_back(app: &App, id: &str) -> Option<String> {
    crate::folders::parent(&app.rootlist, &Row::Folder(id.to_string()))
}

/// Where the + button's new folder goes: inside the opened folder, else at
/// the top of the library.
fn new_folder_place(app: &App, filter: Filter) -> Place {
    match opened_folder(app, filter) {
        Some(id) => Place::InFolder(id),
        None => Place::Start,
    }
}

/// What each folder holds itself, its own playlists and its own folders,
/// as Spotify counts them.
fn folder_holdings(
    rootlist: &[crate::player::RootlistEntry],
) -> std::collections::HashMap<&str, (usize, usize)> {
    use crate::player::RootlistEntry;
    let mut open: Vec<&str> = Vec::new();
    let mut holdings = std::collections::HashMap::new();
    for row in rootlist {
        match row {
            RootlistEntry::FolderStart { id, .. } => {
                if let Some(parent) = open.last() {
                    holdings.entry(*parent).or_insert((0, 0)).1 += 1;
                }
                holdings.entry(id.as_str()).or_insert((0, 0));
                open.push(id);
            }
            RootlistEntry::FolderEnd => {
                open.pop();
            }
            RootlistEntry::Playlist(_) => {
                if let Some(parent) = open.last() {
                    holdings.entry(*parent).or_insert((0, 0)).0 += 1;
                }
            }
        }
    }
    holdings
}

/// How many playlists each folder holds, nested ones included, counting
/// only those `keep` lets through. One pass over the tree, so a library of
/// many folders does not walk it once for each.
fn folder_counts(
    rootlist: &[crate::player::RootlistEntry],
    keep: impl Fn(&str) -> bool,
) -> std::collections::HashMap<&str, usize> {
    use crate::player::RootlistEntry;
    let mut open: Vec<&str> = Vec::new();
    let mut counts = std::collections::HashMap::new();
    for row in rootlist {
        match row {
            RootlistEntry::FolderStart { id, .. } => {
                counts.entry(id.as_str()).or_insert(0);
                open.push(id);
            }
            RootlistEntry::FolderEnd => {
                open.pop();
            }
            RootlistEntry::Playlist(uri) if keep(uri) => {
                for id in &open {
                    *counts.entry(*id).or_insert(0) += 1;
                }
            }
            RootlistEntry::Playlist(_) => {}
        }
    }
    counts
}

/// The artwork widths a row and a grid card ask for on this display.
#[derive(Clone, Copy)]
struct EntryArt {
    row: u32,
    grid: u32,
}

impl EntryArt {
    fn of(app: &App) -> Self {
        Self {
            row: app.art_px(ROW_ART),
            grid: app.art_px(super::widgets::CARD_ART),
        }
    }
}

fn playlist_entry(
    locale: Locale,
    playlist: &crate::api::models::Playlist,
    index: usize,
    user_id: &str,
    editable: bool,
    depth: u8,
    art: EntryArt,
) -> Entry {
    Entry {
        image: pick_image(&playlist.images, art.row).map(str::to_string),
        grid_image: pick_image(&playlist.images, art.grid).map(str::to_string),
        name: playlist.name.clone(),
        // Translators: {owner} is the name of the playlist's owner.
        subtitle: gettext(locale, "Playlist • {owner}").replace("{owner}", playlist.owner_name()),
        grid_subtitle: playlist.owner_name().to_string(),
        page: Page::Playlist(playlist.id.clone()),
        uri: playlist.uri.clone(),
        round: false,
        liked: false,
        owned: playlist.owned_by(user_id),
        editable,
        playlist_index: Some(index),
        folder: None,
        depth,
        added_at: None,
        pin_key: None,
    }
}

fn nav_row(
    ui: &mut egui::Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
    active: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let color = if active || response.hovered() {
            palette.text
        } else {
            palette.secondary
        };
        let icon_rect =
            Rect::from_center_size(pos2(rect.left() + 22.0, rect.center().y), Vec2::splat(22.0));
        icon.image(color, 22.0).paint_at(ui, icon_rect);
        ui.painter().text(
            pos2(rect.left() + 46.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            theme::bold(15.0),
            color,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), active, label)
    });
    theme::focus_ring(ui, &response);
    response
}

/// Draw Library navigation, shelf controls, search, and the visible rows.
/// Playlist ownership and text filters are applied before ordering and pin placement.
fn contents(app: &mut App, ui: &mut egui::Ui, grid_art: Option<Rect>) {
    let palette = app.palette;
    let page = app.page().clone();
    let locale = app.locale;
    ui.add_space(4.0);
    if nav_row(
        ui,
        &palette,
        Icon::House,
        &gettext(locale, "Home"),
        page == Page::Home,
    )
    .clicked()
    {
        app.actions.push(Action::Open(Page::Home));
    }
    if nav_row(
        ui,
        &palette,
        Icon::Search,
        &gettext(locale, "Search"),
        page == Page::Search,
    )
    .clicked()
    {
        app.actions.push(Action::FocusSearch);
    }
    ui.add_space(10.0);
    ui.painter().hline(
        ui.max_rect().x_range().shrink(4.0),
        ui.cursor().top(),
        egui::Stroke::new(1.0, palette.outline),
    );
    ui.add_space(10.0);

    let mut shown = Shown::of(&app.settings.library_kinds);
    let filter = listed_shelves(app, shown)[0];
    let show_search_id = egui::Id::new("sidebar-show-search");
    let mut show_search = ui
        .data(|data| data.get_temp::<bool>(show_search_id))
        .unwrap_or(false);

    let mut focus_search = false;

    ui.horizontal(|ui| {
        ui.add_space(6.0);
        theme::icon(ui, Icon::Library, 22.0, palette.secondary);
        ui.add_space(2.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if theme::icon_button(
                ui,
                Icon::PanelLeft,
                16.0,
                palette.secondary,
                palette.text,
                super::keys::platform_shortcut(
                    &gettext(locale, "Hide sidebar (Ctrl+B)"),
                    &gettext(locale, "Hide sidebar (Cmd+B)"),
                ),
            )
            .clicked()
            {
                app.actions.push(Action::ToggleSidebar);
            }
            // How the Library is shown: its rows, its cards, or the map in
            // their place. One button, so the heading keeps its room in the
            // default width; its menu offers the other two.
            let grid = app.settings.sidebar_grid;
            let map = app.settings.sidebar_map;
            let shown_as = if map {
                Icon::Network
            } else if grid {
                Icon::LayoutGrid
            } else {
                Icon::LayoutList
            };
            // Translators: Library sidebar button: choose to show the Library as rows, cover cards or the playlist map.
            let view = gettext(locale, "Show as");
            let response =
                theme::icon_button(ui, shown_as, 16.0, palette.secondary, palette.text, &view);
            egui::Popup::menu(&response)
                .frame(super::widgets::menu_frame(&palette))
                .show(|ui| {
                    ui.set_min_width(160.0);
                    ui.set_max_width(260.0);
                    if (map || grid)
                        && super::widgets::menu_item(
                            ui,
                            &palette,
                            Some(Icon::LayoutList),
                            &gettext(locale, "Show as list"),
                        )
                    {
                        app.actions.push(Action::SetLibraryMap(false));
                        app.actions.push(Action::SetLibraryGrid(false));
                    }
                    if (map || !grid)
                        && super::widgets::menu_item(
                            ui,
                            &palette,
                            Some(Icon::LayoutGrid),
                            &gettext(locale, "Show as grid"),
                        )
                    {
                        app.actions.push(Action::SetLibraryMap(false));
                        app.actions.push(Action::SetLibraryGrid(true));
                    }
                    if !map
                        && super::widgets::menu_item(
                            ui,
                            &palette,
                            Some(Icon::Network),
                            // Translators: Library sidebar button: show the Library as the playlist map in place of its rows.
                            &gettext(locale, "Show as map"),
                        )
                    {
                        app.actions.push(Action::SetLibraryMap(true));
                    }
                });
            // One item never deserved a menu: the plus creates directly.
            // Where folders can be made there are two, as in Spotify's apps.
            if app.folders_editable() {
                let response = theme::icon_button(
                    ui,
                    Icon::Plus,
                    16.0,
                    palette.secondary,
                    palette.text,
                    &gettext(locale, "Create a playlist or folder"),
                );
                egui::Popup::menu(&response)
                    .frame(super::widgets::menu_frame(&palette))
                    .show(|ui| {
                        ui.set_min_width(200.0);
                        ui.set_max_width(300.0);
                        if super::widgets::menu_item(
                            ui,
                            &palette,
                            Some(Icon::ListMusic),
                            &gettext(locale, "Playlist"),
                        ) {
                            app.actions.push(Action::ShowDialog(Dialog::CreatePlaylist {
                                name: String::new(),
                                public: false,
                                add_uris: Vec::new(),
                            }));
                        }
                        if super::widgets::menu_item(
                            ui,
                            &palette,
                            Some(Icon::Library),
                            &gettext(locale, "Folder"),
                        ) {
                            app.actions.push(Action::ShowDialog(new_folder(
                                new_folder_place(app, filter),
                                None,
                            )));
                        }
                    });
            } else if theme::icon_button(
                ui,
                Icon::Plus,
                16.0,
                palette.secondary,
                palette.text,
                &gettext(locale, "Create a playlist"),
            )
            .clicked()
            {
                app.actions.push(Action::ShowDialog(Dialog::CreatePlaylist {
                    name: String::new(),
                    public: false,
                    add_uris: Vec::new(),
                }));
            }
            if theme::icon_button(
                ui,
                Icon::Search,
                16.0,
                palette.secondary,
                palette.text,
                &gettext(locale, "Search Your Library"),
            )
            .clicked()
            {
                show_search = !show_search;
                if show_search {
                    focus_search = true;
                } else {
                    app.library.filter.clear();
                }
            }
            // The buttons come first; the heading takes the space left,
            // a little smaller where a translation runs long, and gives way
            // to them in the narrowest sidebar, where the icon still names
            // the section.
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let heading = gettext(locale, "Library");
                let room = ui.available_width() - 6.0;
                let fits = [15.0, 14.0, 13.0].into_iter().find(|&size| {
                    ui.painter()
                        .layout_no_wrap(heading.to_string(), theme::bold(size), palette.text)
                        .size()
                        .x
                        <= room
                });
                if let Some(size) = fits {
                    theme::text(ui, heading, theme::bold(size), palette.text);
                }
            });
        });
    });
    ui.add_space(6.0);

    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        // Each kind is a checkbox: tick several to list them together,
        // or none to list them all.
        for (value, label) in [
            (Filter::Playlists, gettext(locale, "Playlists")),
            (Filter::Albums, gettext(locale, "Albums")),
            (Filter::Artists, gettext(locale, "Artists")),
            (Filter::Podcasts, gettext(locale, "Podcasts")),
        ] {
            let ticked = shown.has(value);
            let response =
                theme::soft_button(ui, &palette, ticked.then_some(Icon::Check), &label, ticked);
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Checkbox,
                    ui.is_enabled(),
                    ticked,
                    &*label,
                )
            });
            if response.clicked() {
                shown = shown.toggled(value);
                app.actions.push(Action::SetLibraryKinds(shown.ticked()));
            }
        }
    });
    let shelves = listed_shelves(app, shown);
    let mixed = shelves.len() > 1;
    let filter = shelves[0];
    let sort = if mixed {
        mixed_sort(app, &shelves)
    } else {
        selected_sort(app, filter)
    };
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if mixed {
            mixed_sort_menu(app, ui, &shelves, sort);
        } else {
            sort_menu(app, ui, filter, sort);
        }
        if shelves.contains(&Filter::Playlists) {
            // Translators: Library filter for playlists created by the signed-in user.
            let label = gettext(locale, "By You");
            let by_you = app.settings.library_by_you;
            let response = theme::soft_button(ui, &palette, None, &label, by_you);
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::Button,
                    ui.is_enabled(),
                    by_you,
                    &*label,
                )
            });
            if response.clicked() {
                app.actions.push(Action::SetLibraryByYou(!by_you));
            }
        }
    });
    ui.data_mut(|data| data.insert_temp(show_search_id, show_search));
    if show_search {
        ui.add_space(4.0);
        let response = super::widgets::search_field(
            ui,
            &palette,
            app.locale,
            egui::Id::new("sidebar-search"),
            &mut app.library.filter,
            &gettext(locale, "Search in Your Library"),
            ui.available_width() - 4.0,
        );
        if focus_search {
            response.request_focus();
        }
    }
    ui.add_space(6.0);

    // Make sure every listed kind is loading.
    for &shelf in &shelves {
        keep_shelf_loading(app, shelf, sort);
    }
    // The map in place of the rows: an open folder is its root, and the
    // way back out is on the map too.
    if app.settings.sidebar_map {
        super::playlist_map::sidebar(app, ui);
        return;
    }

    // Opened into a folder: its name, and the way back to the folder that
    // holds it, or to the whole library.
    if filter == Filter::Playlists
        && let Some((start, _)) = entered_span(app)
        && let crate::player::RootlistEntry::FolderStart { id, name } = &app.rootlist[start]
    {
        let back = way_back(app, id);
        let name = if name.is_empty() {
            gettext(locale, "Folder").into_owned()
        } else {
            name.clone()
        };
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            let response = theme::icon_button(
                ui,
                Icon::ArrowLeft,
                16.0,
                palette.secondary,
                palette.text,
                &gettext(locale, "Back"),
            );
            if response.clicked() {
                ui.data_mut(|data| data.insert_temp(last_click_id(), response.id));
                app.actions.push(Action::EnterLibraryFolder(back));
            }
            theme::icon(ui, Icon::Library, 18.0, palette.secondary);
            theme::text(ui, name, theme::bold(14.0), palette.text);
        });
        ui.add_space(4.0);
    }
    let needle = app.library.filter.trim().to_lowercase();
    let LibraryRows {
        entries,
        group_starts,
        loading,
        error,
        more_pages,
    } = library_rows(app, &shelves, &needle);
    let custom_order = !mixed && filter == Filter::Playlists && sort == LibrarySort::Local;
    let pins = app.library_pins();
    let pinned_rows = pinned_rows(&entries, &pins);
    let playing_context = app.playing_context_uri();
    let context_playing = app.believed_playing();
    let current_page = app.page().clone();

    // Each opened folder starts at its top, and the library keeps its own
    // place for the way back.
    let opened = opened_folder(app, filter);
    crate::autoscroll::show(
        ui,
        egui::ScrollArea::vertical()
            .id_salt(("sidebar-list", opened))
            .auto_shrink([false, false]),
        egui::Vec2b::new(false, true),
        |ui| {
            if egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx())
                || egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx())
            {
                super::widgets::scroll_during_drag(ui);
            }
            if loading {
                super::widgets::loading_row(ui, &palette, app.locale);
            }
            if let Some(error) = &error {
                super::widgets::error_row(ui, app, error, None);
            }
            if entries.is_empty() && !loading && error.is_none() {
                ui.add_space(12.0);
                theme::subtle(
                    ui,
                    &palette,
                    &if needle.is_empty() {
                        gettext(app.locale, "Nothing here yet.")
                    } else {
                        gettext(app.locale, "No matches.")
                    },
                );
            }
            if app.settings.sidebar_grid {
                library_grid(
                    app,
                    ui,
                    &entries,
                    &group_starts,
                    (!mixed).then_some(filter),
                    custom_order,
                    pinned_rows,
                    grid_art,
                );
                for page in &more_pages {
                    super::widgets::load_more_when_near_end(ui, app, page.clone(), true);
                }
                ui.add_space(grid_art.map_or(0.0, |rect| ui.max_rect().bottom() - rect.top()));
                return;
            }

            let compact = app.settings.sidebar_compact;
            let row_height = if compact {
                COMPACT_ROW_HEIGHT
            } else {
                DEFAULT_ROW_HEIGHT
            };
            // Calculate drop positions from fixed row height because rows shift
            // before drawing.
            let list_top = ui.cursor().top();
            let pointer = ui.ctx().pointer_latest_pos().filter(|pos| {
                ui.clip_rect().contains(*pos) && ui.rect_contains_pointer(ui.clip_rect())
            });
            // Tracks may drop on Liked Songs or playlists that take songs
            // from this account.
            let dragging_song = egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx());
            let drop_target = dragging_song
                .then_some(pointer)
                .flatten()
                .map(|pos| ((pos.y - list_top) / row_height).floor())
                .filter(|row| *row >= 0.0 && *row < entries.len() as f32)
                .map(|row| row as usize)
                .filter(|row| entries[*row].liked || entries[*row].editable);
            // Sidebar entries, including Liked Songs, drop between rows.
            // Rows of several kinds keep their shared order: none moves.
            let reordering =
                !mixed && egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx());
            let reorder_slot = reordering.then_some(pointer).flatten().map(|pos| {
                let slot = (((pos.y - list_top) / row_height).round().max(0.0) as usize)
                    .min(entries.len());
                landing_slot(app, &entries, slot)
            });
            let art = app.backend.art().clone();
            super::widgets::virtual_rows(ui, entries.len(), row_height, |ui, index| {
                let entry = &entries[index];
                if !app.settings.sidebar_compact
                    && let Some(image) = &entry.image
                {
                    // Prepare the enlarged preview before this row is opened.
                    app.softened_covers.texture(ui.ctx(), &art, image);
                }
                let droppable = entry.liked || entry.editable;
                let drop_hover = drop_target == Some(index);
                let active = entry.folder.is_none() && entry.page == current_page;
                // Liked Songs has no URI of its own here; Spotify plays it
                // as the account's collection context.
                let current = entry_is_playing_context(entry, playing_context.as_deref());
                let playing = context_playing && current;
                let pinned = pins.iter().any(|key| key == entry.ordering_key());
                let (_, rect) = ui.allocate_space(vec2(ui.available_width(), row_height));
                if group_starts.contains(&index) {
                    group_line(ui, &palette, rect.x_range(), rect.top());
                }
                let id = ui.id().with((
                    "library-row",
                    &entry.uri,
                    entry.liked,
                    entry.folder.as_ref().map(|(id, _, _)| id),
                ));
                let response = ui.interact(rect, id, Sense::click_and_drag());
                response.widget_info(|| {
                    egui::WidgetInfo::selected(
                        egui::WidgetType::Button,
                        ui.is_enabled(),
                        active,
                        if let Some((_, collapsed, _)) = &entry.folder {
                            app.locale.folder_state_label(&entry.name, *collapsed)
                        } else {
                            entry.name.clone()
                        },
                    )
                });
                // Animate rows around the current track or entry drop target.
                let shift = ui.ctx().animate_value_with_time(
                    ui.id().with(("drop-shift", index)),
                    if let Some(slot) = reorder_slot {
                        if index < slot { -4.0 } else { 4.0 }
                    } else {
                        match drop_target {
                            Some(target) if index < target => -4.0,
                            Some(target) if index > target => 4.0,
                            _ => 0.0,
                        }
                    },
                    crate::motion::seconds(ui.ctx(), 0.12),
                );
                let rect = rect.translate(vec2(0.0, shift));
                // Set when the cover play button takes a click, so a double
                // click on it does not also play from the row.
                let mut cover_took_click = false;
                if ui.is_rect_visible(rect) {
                    if active {
                        ui.painter()
                            .rect_filled(rect, CornerRadius::same(6), palette.surface);
                    } else if response.hovered() {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            palette.surface_hover.gamma_multiply(0.6),
                        );
                    }
                    if drop_hover {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            palette.accent.gamma_multiply(0.18),
                        );
                        ui.painter().rect_stroke(
                            rect,
                            CornerRadius::same(6),
                            egui::Stroke::new(1.5, palette.accent),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let name_color = if playing {
                        palette.accent
                    } else {
                        palette.text
                    };
                    let indent = f32::from(entry.depth) * 14.0;
                    if let Some((folder_id, collapsed, _)) = &entry.folder {
                        let chevron = if *collapsed {
                            Icon::ChevronRight
                        } else {
                            Icon::ChevronDown
                        };
                        let left = rect.left() + LIBRARY_ITEM_PADDING + indent;
                        let chevron_rect = Rect::from_center_size(
                            pos2(left + 8.0, rect.center().y),
                            Vec2::splat(16.0),
                        );
                        // The chevron rolls the folder up or out in place; the
                        // rest of the row opens it.
                        let toggle = ui.interact(
                            chevron_rect.expand(6.0),
                            id.with("chevron"),
                            Sense::click(),
                        );
                        let locale = app.locale;
                        toggle.widget_info(|| {
                            egui::WidgetInfo::labeled(
                                egui::WidgetType::Button,
                                ui.is_enabled(),
                                locale.folder_toggle_label(&entry.name, *collapsed),
                            )
                        });
                        theme::focus_ring(ui, &toggle);
                        if toggle.clicked() {
                            app.actions
                                .push(Action::ToggleLibraryFolder(folder_id.clone()));
                            cover_took_click = true;
                        }
                        chevron
                            .image(
                                if toggle.hovered() {
                                    palette.text
                                } else {
                                    palette.secondary
                                },
                                16.0,
                            )
                            .paint_at(ui, chevron_rect);
                        Icon::Library.image(palette.secondary, 20.0).paint_at(
                            ui,
                            Rect::from_center_size(
                                pos2(left + 30.0, rect.center().y),
                                Vec2::splat(20.0),
                            ),
                        );
                        let text_left = left + 46.0;
                        let text_right = rect.right() - LIBRARY_ITEM_PADDING;
                        let painter = ui.painter().with_clip_rect(Rect::from_min_max(
                            pos2(text_left, rect.top()),
                            pos2(text_right, rect.bottom()),
                        ));
                        crate::bidi::paint_line(
                            &painter,
                            text_left,
                            text_right,
                            rect.center().y - if compact { 0.0 } else { 9.0 },
                            &entry.name,
                            theme::medium(if compact { 13.5 } else { 14.0 }),
                            name_color,
                        );
                        if !compact {
                            crate::bidi::paint_line(
                                &painter,
                                text_left,
                                text_right,
                                rect.center().y + 10.0,
                                &entry.subtitle,
                                theme::regular(12.5),
                                palette.secondary,
                            );
                        }
                    } else if compact {
                        let text_left = rect.left() + LIBRARY_ITEM_PADDING + indent;
                        let text_right = rect.right()
                            - if playing || pinned {
                                28.0
                            } else {
                                LIBRARY_ITEM_PADDING
                            };
                        let painter = ui.painter().with_clip_rect(Rect::from_min_max(
                            pos2(text_left, rect.top()),
                            pos2(text_right, rect.bottom()),
                        ));
                        crate::bidi::paint_line(
                            &painter,
                            text_left,
                            text_right,
                            rect.center().y,
                            &entry.name,
                            theme::medium(13.5),
                            name_color,
                        );
                    } else {
                        let cover_rect = Rect::from_center_size(
                            pos2(
                                rect.left() + LIBRARY_ITEM_PADDING + indent + 22.0,
                                rect.center().y,
                            ),
                            Vec2::splat(ROW_ART),
                        );
                        if entry.liked {
                            liked_cover(ui, cover_rect, 6.0);
                        } else if entry.page == Page::LocalFiles {
                            local_files_cover(ui, &palette, cover_rect, 6.0);
                        } else {
                            super::widgets::paint_cover(
                                ui,
                                &palette,
                                entry.image.as_deref(),
                                cover_rect,
                                if entry.round { 22.0 } else { 6.0 },
                                if entry.round { Icon::User } else { Icon::Music },
                                Some(app.backend.art()),
                            );
                        }
                        let text_left = cover_rect.right() + 12.0;
                        let text_right = rect.right()
                            - if playing || pinned {
                                28.0
                            } else {
                                LIBRARY_ITEM_PADDING
                            };
                        let painter = ui.painter().with_clip_rect(Rect::from_min_max(
                            pos2(text_left, rect.top()),
                            pos2(text_right, rect.bottom()),
                        ));
                        crate::bidi::paint_line(
                            &painter,
                            text_left,
                            text_right,
                            rect.center().y - 9.0,
                            &entry.name,
                            theme::medium(14.0),
                            name_color,
                        );
                        crate::bidi::paint_line(
                            &painter,
                            text_left,
                            text_right,
                            rect.center().y + 10.0,
                            &entry.subtitle,
                            theme::regular(12.5),
                            palette.secondary,
                        );
                        // Hovering the art offers to play right from here.
                        cover_took_click = cover_play_button(
                            app, ui, entry, index, cover_rect, &response, current,
                        );
                    }
                    if playing {
                        let icon_rect = Rect::from_center_size(
                            pos2(rect.right() - 16.0, rect.center().y),
                            Vec2::splat(16.0),
                        );
                        Icon::Volume2
                            .image(palette.accent, 16.0)
                            .paint_at(ui, icon_rect);
                    } else if pinned {
                        let icon_rect = Rect::from_center_size(
                            pos2(rect.right() - 16.0, rect.center().y),
                            Vec2::splat(13.0),
                        );
                        Icon::Pin
                            .image(palette.secondary, 13.0)
                            .paint_at(ui, icon_rect);
                    }
                    // Rows that cannot take the song step back a little.
                    if dragging_song && !droppable {
                        ui.painter().rect_filled(
                            rect,
                            CornerRadius::same(6),
                            palette.panel.gamma_multiply(0.5),
                        );
                    }
                }
                finish_entry_interaction(
                    app,
                    ui,
                    &response,
                    entry,
                    cover_took_click,
                    custom_order,
                    true,
                );
            });
            if let Some(slot) = reorder_slot {
                // A line in the gap the rows opened, so the eye lands
                // where the row will.
                let y = list_top + slot as f32 * row_height;
                ui.painter().hline(
                    ui.max_rect().x_range().shrink(6.0),
                    y,
                    egui::Stroke::new(2.0, palette.accent),
                );
                if ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
                    && let Some(drag) = egui::DragAndDrop::take_payload::<DragEntry>(ui.ctx())
                {
                    if filter == Filter::Playlists {
                        drop_playlist_row(app, &entries, pinned_rows, slot, &drag.uri);
                    } else {
                        drop_row(app, &entries, pinned_rows, slot, &drag.uri);
                    }
                }
            }
            for page in &more_pages {
                super::widgets::load_more_when_near_end(ui, app, page.clone(), true);
            }
        },
    );
}

const EXPANDED_ART_GAP: f32 = 10.0;
const LIBRARY_ITEM_PADDING: f32 = 8.0;
/// The cover of a list row.
const ROW_ART: f32 = 44.0;
const GRID_MIN_CARD_WIDTH: f32 = 108.0;
const GRID_MAX_COLUMNS: usize = 4;
const GRID_TEXT_HEIGHT: f32 = 44.0;

#[derive(Clone, Copy)]
struct GridLayout {
    columns: usize,
    card_width: f32,
    card_height: f32,
    row_height: f32,
}

fn grid_layout(width: f32) -> GridLayout {
    let columns = ((width / GRID_MIN_CARD_WIDTH).floor() as usize).clamp(2, GRID_MAX_COLUMNS);
    let card_width = (width / columns as f32).max(1.0);
    let card_height = card_width + GRID_TEXT_HEIGHT;
    GridLayout {
        columns,
        card_width,
        card_height,
        row_height: card_height,
    }
}

fn paint_grid_text(
    ui: &egui::Ui,
    rect: Rect,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
) {
    let galley = crate::bidi::layout(
        ui.painter(),
        text,
        font,
        color,
        rect.width(),
        1,
        Some(crate::bidi::ELLIPSIS),
    );
    ui.painter()
        .galley(crate::bidi::galley_pos(rect, &galley), galley, color);
}

fn grid_reorder_slot(
    pointer: egui::Pos2,
    origin: egui::Pos2,
    layout: GridLayout,
    count: usize,
) -> usize {
    let row = ((pointer.y - origin.y) / layout.row_height)
        .floor()
        .max(0.0) as usize;
    let column = ((pointer.x - origin.x) / layout.card_width)
        .floor()
        .max(0.0)
        .min((layout.columns - 1) as f32) as usize;
    let after = pointer.x - origin.x - column as f32 * layout.card_width > layout.card_width / 2.0;
    (row * layout.columns + column + usize::from(after)).min(count)
}

/// The grid's rows as ranges of `entries`, `columns` to a row, each group
/// from a fresh row.
fn grid_rows(count: usize, columns: usize, group_starts: &[usize]) -> Vec<(usize, usize)> {
    let mut rows = Vec::new();
    let mut bounds: Vec<usize> = group_starts
        .iter()
        .copied()
        .filter(|start| *start > 0 && *start < count)
        .collect();
    bounds.push(count);
    let mut start = 0;
    for end in bounds {
        let mut at = start;
        while at < end {
            rows.push((at, (at + columns.max(1)).min(end)));
            at += columns.max(1);
        }
        start = end;
    }
    rows
}

/// `filter` is the one kind listed, whose cards can be moved; `None` for
/// several kinds, whose shared order keeps them where they are.
#[allow(clippy::too_many_arguments)]
fn library_grid(
    app: &mut App,
    ui: &mut egui::Ui,
    entries: &[Entry],
    group_starts: &[usize],
    filter: Option<Filter>,
    custom_order: bool,
    pinned_rows: usize,
    grid_art: Option<Rect>,
) {
    if entries.is_empty() {
        return;
    }
    let palette = app.palette;
    let pins = app.library_pins();
    let playing_context = app.playing_context_uri();
    let context_playing = app.believed_playing();
    let current_page = app.page().clone();
    let layout = grid_layout(ui.available_width());
    let origin = ui.cursor().min;
    let pointer = ui.ctx().pointer_latest_pos().filter(|pos| {
        ui.clip_rect().contains(*pos)
            && ui.rect_contains_pointer(ui.clip_rect())
            && !grid_art.is_some_and(|art| art.contains(*pos))
    });
    let dragging_song = egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx());
    let reordering =
        filter.is_some() && egui::DragAndDrop::has_payload_of_type::<DragEntry>(ui.ctx());
    let reorder_slot = reordering
        .then_some(pointer)
        .flatten()
        .map(|pointer| grid_reorder_slot(pointer, origin, layout, entries.len()))
        .map(|slot| landing_slot(app, entries, slot));
    let rows = grid_rows(entries.len(), layout.columns, group_starts);
    let grid_id = ui.unique_id().with("library-grid");

    super::widgets::virtual_rows(ui, rows.len(), layout.row_height, |ui, row| {
        let (start, end) = rows[row];
        if group_starts.contains(&start) {
            let top = ui.cursor().top();
            group_line(ui, &palette, ui.max_rect().x_range(), top);
        }
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for index in start..end {
                let entry = &entries[index];
                let entry_id = (
                    entry.ordering_key(),
                    entry.liked,
                    entry.folder.as_ref().map(|(id, _, _)| id.as_str()),
                );
                ui.scope_builder(egui::UiBuilder::new().id(grid_id.with(entry_id)), |ui| {
                    let active = entry.folder.is_none() && entry.page == current_page;
                    let playing_here = entry_is_playing_context(entry, playing_context.as_deref());
                    let playing = context_playing && playing_here;
                    let pinned = pins.iter().any(|key| key == entry.ordering_key());
                    let droppable = entry.liked || entry.editable;
                    let (rect, response) = ui.allocate_exact_size(
                        vec2(layout.card_width, layout.card_height),
                        Sense::click_and_drag(),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            ui.is_enabled(),
                            active,
                            if let Some((_, collapsed, _)) = &entry.folder {
                                app.locale.folder_state_label(&entry.name, *collapsed)
                            } else {
                                entry.name.clone()
                            },
                        )
                    });
                    let cover_rect = Rect::from_min_size(
                        rect.min + Vec2::splat(LIBRARY_ITEM_PADDING),
                        Vec2::splat(layout.card_width - LIBRARY_ITEM_PADDING * 2.0),
                    );
                    let mut cover_took_click = false;
                    if ui.is_rect_visible(rect) {
                        if active {
                            ui.painter()
                                .rect_filled(rect, CornerRadius::same(6), palette.surface);
                        } else if response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                CornerRadius::same(6),
                                palette.surface_hover.gamma_multiply(0.6),
                            );
                        }
                        if entry.liked {
                            liked_cover(ui, cover_rect, 6.0);
                        } else if entry.page == Page::LocalFiles {
                            local_files_cover(ui, &palette, cover_rect, 6.0);
                        } else if entry.folder.is_some() {
                            ui.painter().rect_filled(
                                cover_rect,
                                CornerRadius::same(6),
                                palette.surface,
                            );
                            Icon::Library
                                .image(palette.secondary, layout.card_width * 0.3)
                                .paint_at(
                                    ui,
                                    Rect::from_center_size(
                                        cover_rect.center(),
                                        Vec2::splat(layout.card_width * 0.3),
                                    ),
                                );
                        } else {
                            super::widgets::paint_cover(
                                ui,
                                &palette,
                                entry.grid_image.as_deref().or(entry.image.as_deref()),
                                cover_rect,
                                if entry.round {
                                    layout.card_width / 2.0
                                } else {
                                    6.0
                                },
                                if entry.round { Icon::User } else { Icon::Music },
                                Some(app.backend.art()),
                            );
                        }

                        cover_took_click =
                            grid_play_button(app, ui, entry, cover_rect, &response, playing_here);

                        let text_left = rect.left() + LIBRARY_ITEM_PADDING;
                        let text_width = (rect.width() - LIBRARY_ITEM_PADDING * 2.0).max(0.0);
                        paint_grid_text(
                            ui,
                            Rect::from_min_size(
                                pos2(text_left, cover_rect.bottom() + 4.0),
                                vec2(text_width, 18.0),
                            ),
                            &entry.name,
                            theme::medium(13.5),
                            if playing {
                                palette.accent
                            } else {
                                palette.text
                            },
                        );
                        paint_grid_text(
                            ui,
                            Rect::from_min_size(
                                pos2(text_left, cover_rect.bottom() + 23.0),
                                vec2(text_width, 17.0),
                            ),
                            &entry.grid_subtitle,
                            theme::regular(12.0),
                            palette.secondary,
                        );

                        if pinned {
                            let pin_rect = grid_pin_rect(cover_rect);
                            ui.painter().circle_filled(
                                pin_rect.center(),
                                pin_rect.width() / 2.0,
                                egui::Color32::from_black_alpha(170),
                            );
                            Icon::Pin.image(egui::Color32::WHITE, 12.0).paint_at(
                                ui,
                                Rect::from_center_size(pin_rect.center(), Vec2::splat(12.0)),
                            );
                        }
                        if dragging_song && !droppable {
                            ui.painter().rect_filled(
                                rect,
                                CornerRadius::same(6),
                                palette.panel.gamma_multiply(0.5),
                            );
                        } else if dragging_song
                            && droppable
                            && pointer.is_some_and(|pos| rect.contains(pos))
                        {
                            ui.painter().rect_stroke(
                                cover_rect,
                                CornerRadius::same(6),
                                egui::Stroke::new(2.0, palette.accent),
                                egui::StrokeKind::Inside,
                            );
                        }
                        if reorder_slot == Some(index) {
                            ui.painter().vline(
                                rect.left(),
                                rect.y_range(),
                                egui::Stroke::new(2.0, palette.accent),
                            );
                        }
                        if index + 1 == entries.len() && reorder_slot == Some(entries.len()) {
                            ui.painter().vline(
                                rect.right(),
                                rect.y_range(),
                                egui::Stroke::new(2.0, palette.accent),
                            );
                        }
                    }

                    finish_entry_interaction(
                        app,
                        ui,
                        &response,
                        entry,
                        cover_took_click,
                        custom_order,
                        pointer.is_some(),
                    );
                });
            }
        });
    });

    if let Some(slot) = reorder_slot
        && ui.input(|input| input.pointer.button_released(egui::PointerButton::Primary))
        && let Some(drag) = egui::DragAndDrop::take_payload::<DragEntry>(ui.ctx())
    {
        if filter == Some(Filter::Playlists) {
            drop_playlist_row(app, entries, pinned_rows, slot, &drag.uri);
        } else {
            drop_row(app, entries, pinned_rows, slot, &drag.uri);
        }
    }
}

fn entry_menu(app: &mut App, response: &egui::Response, entry: &Entry, custom_order: bool) {
    if !entry.uri.is_empty() {
        let owned_playlist = entry
            .owned
            .then_some(entry.playlist_index)
            .flatten()
            .and_then(|index| {
                app.library
                    .playlists
                    .get()
                    .and_then(|list| list.get(index))
                    .cloned()
            });
        egui::Popup::context_menu(response)
            .frame(super::widgets::menu_frame(&app.palette))
            .show(|ui| {
                super::widgets::context_menu_items(
                    ui,
                    app,
                    &entry.uri,
                    &entry.name,
                    owned_playlist.as_ref(),
                );
                move_to_folder_menu(app, ui, &Row::Playlist(entry.uri.clone()));
                // Spotify pins nothing inside a folder; one already pinned
                // can still be unpinned.
                let in_folder = app.pins_synced()
                    && crate::folders::parent(&app.rootlist, &Row::Playlist(entry.uri.clone()))
                        .is_some();
                let pinned = app
                    .library_pins()
                    .iter()
                    .any(|key| key == entry.ordering_key());
                if !in_folder || pinned {
                    pin_menu(app, ui, entry.ordering_key());
                }
                if custom_order
                    && super::widgets::menu_item(
                        ui,
                        &app.palette,
                        Some(Icon::Clock),
                        &gettext(app.locale, "Sort by recently played"),
                    )
                {
                    app.actions.push(Action::SetLibrarySort {
                        shelf: Filter::Playlists,
                        sort: LibrarySort::RecentlyPlayed,
                    });
                }
            });
    } else if entry.liked {
        egui::Popup::context_menu(response)
            .frame(super::widgets::menu_frame(&app.palette))
            .show(|ui| {
                // The same width as every other menu; without it the menu
                // stretches as wide as the window.
                ui.set_min_width(200.0);
                ui.set_max_width(300.0);
                if super::widgets::menu_item(
                    ui,
                    &app.palette,
                    Some(Icon::Play),
                    &gettext(app.locale, "Play"),
                ) && let Some(user) = &app.user
                {
                    app.actions.push(Action::PlayContext {
                        uri: format!("spotify:user:{}:collection", user.id),
                        offset_uri: None,
                        offset_index: None,
                    });
                }
                pin_menu(app, ui, entry.ordering_key());
            });
    } else if let Some((id, _, _)) = &entry.folder
        && app.folders_editable()
    {
        let palette = app.palette;
        let locale = app.locale;
        // The row shows "Folder" for a folder with no name; the dialog
        // starts from the name Spotify really holds.
        let held = app
            .rootlist
            .iter()
            .find_map(|row| match row {
                RootlistEntry::FolderStart { id: found, name } if found == id => Some(name.clone()),
                _ => None,
            })
            .unwrap_or_default();
        egui::Popup::context_menu(response)
            .frame(super::widgets::menu_frame(&palette))
            .show(|ui| {
                ui.set_min_width(200.0);
                ui.set_max_width(300.0);
                if super::widgets::menu_item(
                    ui,
                    &palette,
                    Some(Icon::Pencil),
                    &gettext(locale, "Rename"),
                ) {
                    app.actions.push(Action::ShowDialog(Dialog::NameFolder {
                        id: Some(id.clone()),
                        name: held.clone(),
                        place: Place::Start,
                        adopt: None,
                    }));
                }
                if super::widgets::menu_item(
                    ui,
                    &palette,
                    Some(Icon::Plus),
                    &gettext(locale, "New folder inside"),
                ) {
                    app.actions.push(Action::ShowDialog(new_folder(
                        Place::InFolder(id.clone()),
                        None,
                    )));
                }
                move_to_folder_menu(app, ui, &Row::Folder(id.clone()));
                if app.pins_synced()
                    && crate::folders::parent(&app.rootlist, &Row::Folder(id.clone())).is_none()
                    && let Some(key) = &entry.pin_key
                {
                    pin_menu(app, ui, key);
                }
                if super::widgets::menu_item(
                    ui,
                    &palette,
                    Some(Icon::Trash),
                    &gettext(locale, "Remove folder"),
                ) {
                    app.actions
                        .push(Action::ShowDialog(Dialog::ConfirmRemoveFolder {
                            id: id.clone(),
                            name: entry.name.clone(),
                        }));
                }
            });
    }
}

/// The menu a right click opens on the map: what the thing's own row in the
/// Library offers, or, on the Library itself, a new playlist or folder.
pub(crate) fn map_menu(app: &mut App, response: &egui::Response, entry: Option<&Entry>) {
    if let Some(entry) = entry {
        entry_menu(app, response, entry, false);
        return;
    }
    let palette = app.palette;
    let locale = app.locale;
    egui::Popup::context_menu(response)
        .frame(super::widgets::menu_frame(&palette))
        .show(|ui| {
            ui.set_min_width(200.0);
            ui.set_max_width(300.0);
            if super::widgets::menu_item(
                ui,
                &palette,
                Some(Icon::ListMusic),
                &gettext(locale, "Playlist"),
            ) {
                app.actions.push(Action::ShowDialog(Dialog::CreatePlaylist {
                    name: String::new(),
                    public: false,
                    add_uris: Vec::new(),
                }));
            }
            if app.folders_editable()
                && super::widgets::menu_item(
                    ui,
                    &palette,
                    Some(Icon::Library),
                    &gettext(locale, "Folder"),
                )
            {
                // The Library's own: at its top, whatever folder is open.
                app.actions
                    .push(Action::ShowDialog(new_folder(Place::Start, None)));
            }
        });
}

/// The dialog that names a new folder at `place`, taking in `adopt`.
fn new_folder(place: Place, adopt: Option<String>) -> Dialog {
    Dialog::NameFolder {
        id: None,
        name: String::new(),
        place,
        adopt,
    }
}

/// Where a playlist or folder can go: into any other folder, out of the
/// one it is in, or, for a playlist, into a new folder made where it is.
/// A folder never goes into itself or a folder inside it.
/// Where a row can be moved from its menu: out of its folder, to just past
/// it, when it is in one, and into each folder by id, name and depth. A
/// folder is never offered itself, a folder inside it, or the folder it is
/// already in. `None` when the row is not in the tree.
fn folder_targets(
    rows: &[RootlistEntry],
    row: &Row,
) -> Option<(Option<Place>, Vec<crate::folders::Folder>)> {
    let present = match row {
        Row::Playlist(uri) => rows
            .iter()
            .any(|held| matches!(held, RootlistEntry::Playlist(held) if held == uri)),
        Row::Folder(id) => crate::folders::folder_span(rows, id).is_some(),
    };
    if !present {
        return None;
    }
    let parent = crate::folders::parent(rows, row);
    let out = crate::folders::out_of_folder(rows, row);
    let targets = crate::folders::folders(rows)
        .into_iter()
        .filter(|folder| Some(&folder.id) != parent.as_ref())
        .filter(|folder| match row {
            Row::Folder(moving) => !crate::folders::within(rows, &folder.id, moving),
            Row::Playlist(_) => true,
        })
        .collect();
    Some((out, targets))
}

fn move_to_folder_menu(app: &mut App, ui: &mut egui::Ui, row: &Row) {
    if !app.folders_editable() {
        return;
    }
    let Some((out, targets)) = folder_targets(&app.rootlist, row) else {
        return;
    };
    let edit = |place: Place| match row {
        Row::Playlist(uri) => FolderEdit::MovePlaylist {
            uri: uri.clone(),
            place,
        },
        Row::Folder(id) => FolderEdit::MoveFolder {
            id: id.clone(),
            place,
        },
    };
    let palette = app.palette;
    let locale = app.locale;
    let mut chosen = None;
    super::widgets::menu_submenu(
        ui,
        &palette,
        Some(Icon::Library),
        &gettext(locale, "Move to folder"),
        |ui| {
            ui.set_min_width(200.0);
            ui.set_max_width(300.0);
            if let Row::Playlist(uri) = row
                && super::widgets::menu_item(
                    ui,
                    &palette,
                    Some(Icon::Plus),
                    &gettext(locale, "New folder"),
                )
            {
                chosen = Some(Action::ShowDialog(new_folder(
                    Place::Before(Row::Playlist(uri.clone())),
                    Some(uri.clone()),
                )));
            }
            if let Some(place) = out
                && super::widgets::menu_item(
                    ui,
                    &palette,
                    Some(Icon::ArrowLeft),
                    &gettext(locale, "Remove from folder"),
                )
            {
                chosen = Some(Action::EditFolders(edit(place)));
            }
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    for crate::folders::Folder { id, name, depth } in &targets {
                        let name = if name.is_empty() {
                            gettext(locale, "Folder").into_owned()
                        } else {
                            name.clone()
                        };
                        let label = format!("{}{name}", "\u{2003}".repeat(*depth));
                        if super::widgets::menu_item(ui, &palette, None, &label) {
                            chosen = Some(Action::EditFolders(edit(Place::InFolder(id.clone()))));
                        }
                    }
                });
        },
    );
    if let Some(action) = chosen {
        app.actions.push(action);
    }
}

fn pin_menu(app: &mut App, ui: &mut egui::Ui, key: &str) {
    let mut pins = app.library_pins();
    let pinned = pins.iter().any(|held| held == key);
    let label = if pinned {
        gettext(app.locale, "Unpin")
    } else {
        gettext(app.locale, "Pin to top")
    };
    if super::widgets::menu_item(
        ui,
        &app.palette,
        Some(if pinned { Icon::PinOff } else { Icon::Pin }),
        &label,
    ) {
        if pinned {
            pins.retain(|held| held != key);
        } else {
            pins.push(key.to_string());
        }
        app.actions.push(Action::ArrangeLibrary {
            pinned: pins,
            playlist_order: None,
        });
    }
}

/// Drops within the pin block arrange pins. Below it, a drop unpins the
/// moved row and saves the full playlist arrangement at the chosen gap.
fn drop_playlist_row(app: &mut App, entries: &[Entry], pinned_rows: usize, slot: usize, key: &str) {
    // A folder rides as its start marker; among the pins it is known by its
    // library URI.
    let folder = crate::folders::start_id(key).map(str::to_owned);
    let pin_key = match (&folder, app.user_id()) {
        (Some(id), Some(user)) => crate::pins::folder_uri(user, id),
        _ => key.to_string(),
    };
    let was_pinned = app.library_pins().contains(&pin_key);
    let into_pins = slot < pinned_rows || (was_pinned && slot == pinned_rows);
    if !into_pins && let Some(edit) = tree_drop(app, entries, slot, key) {
        if was_pinned {
            let mut pins = app.library_pins();
            pins.retain(|held| *held != pin_key);
            app.actions.push(Action::ArrangeLibrary {
                pinned: pins,
                playlist_order: None,
            });
        }
        app.actions.push(Action::EditFolders(edit));
        return;
    }
    if folder.is_some() {
        // A folder joins the pins only where they are Spotify's, and has no
        // place in a local order.
        if into_pins && app.pins_synced() {
            drop_row(app, entries, pinned_rows, slot, &pin_key);
        }
        return;
    }
    if key != LIKED_SONGS_KEY
        && !app
            .library
            .playlists
            .get()
            .is_some_and(|playlists| playlists.iter().any(|playlist| playlist.uri == key))
    {
        return;
    }
    if into_pins {
        drop_row(app, entries, pinned_rows, slot, key);
        return;
    }
    let mut pins = app.library_pins();
    pins.retain(|held| held != key);
    let mut order = full_playlist_order(app);
    let anchor = entries
        .iter()
        .skip(slot)
        .find_map(|entry| {
            if let Some((id, _, _)) = &entry.folder {
                // A drop before a collapsed folder precedes its first child
                // when switching to the flat local arrangement.
                let start = app.rootlist.iter().position(|row| matches!(row, crate::player::RootlistEntry::FolderStart { id: found, .. } if found == id))?;
                app.rootlist[start + 1..].iter().find_map(|row| match row {
                    crate::player::RootlistEntry::Playlist(held) if held != key && order.contains(held) => Some(held.as_str()),
                    _ => None,
                })
            } else {
                let held = entry.ordering_key();
                (!held.is_empty() && held != key).then_some(held)
            }
        })
        .map(str::to_string);
    order.retain(|held| held != key);
    let at = anchor
        .and_then(|anchor| order.iter().position(|held| *held == anchor))
        .unwrap_or(order.len());
    order.insert(at, key.to_string());
    app.actions.push(Action::ArrangeLibrary {
        pinned: pins,
        playlist_order: Some(order),
    });
}

/// Whether the playlist shelf shows Spotify's own tree, folders and all,
/// so a drag there arranges the account rather than this computer. By You
/// keeps the tree, and every row it shows is a real row to drop beside.
fn shows_spotify_tree(app: &App) -> bool {
    selected_sort(app, Filter::Playlists) == LibrarySort::Spotify
        && app.library.filter.trim().is_empty()
        && app.folders_editable()
}

/// A drop in Spotify's tree moves the row on Spotify: before the row that
/// now stands at the gap, in whatever folder that row is in, or to the end
/// when nothing below belongs to the tree. `None` leaves the drop to the
/// local arrangement.
fn tree_drop(app: &App, entries: &[Entry], slot: usize, key: &str) -> Option<FolderEdit> {
    if !shows_spotify_tree(app) {
        return None;
    }
    let in_tree = |uri: &str| {
        app.rootlist
            .iter()
            .any(|row| matches!(row, RootlistEntry::Playlist(held) if held == uri))
    };
    let moving = match crate::folders::start_id(key) {
        Some(id) => Row::Folder(id.to_string()),
        None if in_tree(key) => Row::Playlist(key.to_string()),
        None => return None,
    };
    // A folder's own rows are no place to land it: a drop among them lands
    // past them, which leaves the folder where it was.
    let span = match &moving {
        Row::Folder(id) => crate::folders::folder_span(&app.rootlist, id),
        Row::Playlist(_) => None,
    };
    let own = |entry: &Entry| {
        let Some((start, end)) = span else {
            return false;
        };
        let at = match &entry.folder {
            Some((id, _, _)) => crate::folders::folder_span(&app.rootlist, id).map(|(at, _)| at),
            None => app
                .rootlist
                .iter()
                .position(|row| matches!(row, RootlistEntry::Playlist(held) if *held == entry.uri)),
        };
        at.is_some_and(|at| (start..=end).contains(&at))
    };
    let place = entries
        .iter()
        .skip(slot)
        .filter(|entry| !own(entry))
        .find_map(|entry| match &entry.folder {
            Some((id, _, _)) => Some(Place::Before(Row::Folder(id.clone()))),
            None if !entry.liked && entry.uri != key && in_tree(&entry.uri) => {
                Some(Place::Before(Row::Playlist(entry.uri.clone())))
            }
            None => None,
        })
        .unwrap_or_else(|| end_place(app));
    Some(match moving {
        Row::Playlist(uri) => FolderEdit::MovePlaylist { uri, place },
        Row::Folder(id) => FolderEdit::MoveFolder { id, place },
    })
}

/// Every unpinned loaded playlist in the selected shelf order, including
/// rows hidden by a search or collapsed folder. A drag snapshots this
/// whole arrangement before applying its new local position.
fn full_playlist_order(app: &App) -> Vec<String> {
    let Some(playlists) = app.library.playlists.get() else {
        return Vec::new();
    };
    let mut entries: Vec<_> = playlists
        .iter()
        .enumerate()
        .map(|(index, playlist)| {
            playlist_entry(
                app.locale,
                playlist,
                index,
                app.user_id().unwrap_or(""),
                false,
                0,
                EntryArt::of(app),
            )
        })
        .collect();
    entries.push(liked_entry(app));
    order_entries(
        app,
        Filter::Playlists,
        selected_sort(app, Filter::Playlists),
        &mut entries,
    );
    let pins = app.library_pins();
    entries
        .iter()
        .map(|entry| entry.ordering_key().to_string())
        .filter(|key| !pins.contains(key))
        .collect()
}

/// Reorders the pin block without disturbing pins on another shelf.
fn drop_row(app: &mut App, entries: &[Entry], pinned_rows: usize, slot: usize, key: &str) {
    let mut pins = app.library_pins();
    pins.retain(|held| held != key);
    if slot <= pinned_rows {
        let anchor = entries[..pinned_rows]
            .iter()
            .skip(slot)
            .map(Entry::ordering_key)
            .find(|held| *held != key);
        let at = anchor
            .and_then(|anchor| pins.iter().position(|held| held == anchor))
            .unwrap_or(pins.len());
        pins.insert(at, key.to_string());
    }
    app.actions.push(Action::ArrangeLibrary {
        pinned: pins,
        playlist_order: None,
    });
}

/// The Local Files tile: a note on the accent colour, as Spotify's green
/// tile marks its own.
pub fn local_files_cover(ui: &egui::Ui, palette: &Palette, rect: Rect, radius: f32) {
    ui.painter()
        .rect_filled(rect, CornerRadius::same(radius as u8), palette.accent);
    let size = rect.width() * 0.42;
    Icon::Music
        .image(palette.on_accent, size)
        .paint_at(ui, Rect::from_center_size(rect.center(), Vec2::splat(size)));
}

/// The purple-to-blue Liked Songs tile.
pub fn liked_cover(ui: &egui::Ui, rect: Rect, radius: f32) {
    let texture_id = egui::Id::new("liked-cover-gradient");
    let texture = ui
        .data(|data| data.get_temp::<egui::TextureHandle>(texture_id))
        .unwrap_or_else(|| {
            let size = 64;
            let lerp = |a: u8, b: u8, t: f32| (a as f32 + (b as f32 - a as f32) * t) as u8;
            let top_left = [0x45, 0x0a, 0xf5];
            let top_right = [0x6a, 0x3a, 0xe8];
            let bottom_left = [0x8e, 0x9f, 0xe5];
            let bottom_right = [0xc4, 0xef, 0xd9];
            let pixels = (0..size)
                .flat_map(|y| {
                    let y = y as f32 / (size - 1) as f32;
                    (0..size).map(move |x| {
                        let x = x as f32 / (size - 1) as f32;
                        egui::Color32::from_rgb(
                            lerp(
                                lerp(top_left[0], top_right[0], x),
                                lerp(bottom_left[0], bottom_right[0], x),
                                y,
                            ),
                            lerp(
                                lerp(top_left[1], top_right[1], x),
                                lerp(bottom_left[1], bottom_right[1], x),
                                y,
                            ),
                            lerp(
                                lerp(top_left[2], top_right[2], x),
                                lerp(bottom_left[2], bottom_right[2], x),
                                y,
                            ),
                        )
                    })
                })
                .collect();
            let texture = ui.ctx().load_texture(
                "liked-cover-gradient",
                egui::ColorImage::new([size, size], pixels),
                egui::TextureOptions::LINEAR,
            );
            ui.data_mut(|data| data.insert_temp(texture_id, texture.clone()));
            texture
        });
    egui::Image::new(&texture)
        .corner_radius(CornerRadius::same(radius.min(127.0) as u8))
        .paint_at(ui, rect);
    let size = rect.width() * 0.45;
    let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(size));
    Icon::HeartFilled
        .image(egui::Color32::WHITE, size)
        .paint_at(ui, icon_rect);
}

#[cfg(all(test, feature = "demo"))]
mod ordering_tests {
    use super::*;
    use crate::api::models::Playlist;
    use crate::settings::Settings;

    const TEST_ART: EntryArt = EntryArt { row: 64, grid: 300 };

    /// Create an isolated demo application with deliberately nonalphabetical rows
    /// and known recent plays so ordering tests can distinguish each sort mode.
    fn app(name: &str) -> App {
        let root =
            std::env::temp_dir().join(format!("spotifast-order-{name}-{}", std::process::id()));
        let mut app = App::new(
            &crate::backend::Waker::default(),
            crate::paths::AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings::default(),
            crate::app::AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
                autostart: false,
            },
        );
        crate::demo::populate(&mut app);
        app.library.playlists = Loadable::Loaded(
            [
                ("a", "Zebra"),
                ("b", "Alpha"),
                ("c", "alpha"),
                ("d", "Beta"),
            ]
            .into_iter()
            .map(|(id, name)| Playlist {
                id: id.into(),
                name: name.into(),
                uri: format!("spotify:playlist:{id}"),
                ..Default::default()
            })
            .collect(),
        );
        app.recent_contexts = vec![uri("d"), uri("a")];
        app
    }

    /// Drain queued UI actions through the real handler before checking the
    /// resulting saved order, pin state, or filter preference.
    fn apply_actions(app: &mut App) {
        for action in std::mem::take(&mut app.actions) {
            app.apply(action, &egui::Context::default());
        }
    }

    /// Give a fixture playlist the same URI key used by rootlist order and pins.
    fn uri(id: &str) -> String {
        format!("spotify:playlist:{id}")
    }

    /// Project every fixture playlist as a flat row, retaining its library index
    /// and leaving ownership and edit permissions disabled for ordering-only tests.
    fn rows(app: &App) -> Vec<Entry> {
        app.library
            .playlists
            .get()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(index, playlist)| {
                playlist_entry(Locale::English, playlist, index, "", false, 0, TEST_ART)
            })
            .collect()
    }

    /// Extract compact playlist IDs in display order for assertions.
    /// Liked Songs has an empty URI, so its position is represented by an empty ID.
    fn ids(entries: &[Entry]) -> Vec<&str> {
        entries
            .iter()
            .map(|entry| entry.uri.rsplit(':').next().unwrap())
            .collect()
    }

    /// Build a mixed library with two owned playlists, a foreign collaborative
    /// playlist, and an unknown owner, retaining their original library indices.
    fn ownership_app(name: &str) -> App {
        let mut app = app(name);
        let id = app.user_id().unwrap().to_string();
        let playlists = app.library.playlists.get_mut().unwrap();
        playlists[0].owner.id = Some(id.clone());
        playlists[1].owner.id = Some("someone-else".into());
        playlists[1].collaborative = true;
        playlists[2].owner.id = Some(id);
        // The last playlist has no known owner.
        app
    }

    /// Run the production playlist projection and ordering together so assertions
    /// observe the same filtered, sorted, and pinned rows as the sidebar.
    fn projected(app: &App, needle: &str) -> Vec<Entry> {
        let mut entries = Vec::new();
        playlist_rows(
            app,
            selected_sort(app, Filter::Playlists),
            needle,
            &mut entries,
        );
        order_entries(
            app,
            Filter::Playlists,
            selected_sort(app, Filter::Playlists),
            &mut entries,
        );
        entries
    }

    /// Verify that ownership filtering composes with search and pins while
    /// retaining source indices, sort selection, and the saved local arrangement.
    #[test]
    fn by_you_uses_account_ownership_and_composes_with_search_sort_and_pins() {
        let mut app = ownership_app("by-you");
        app.settings.sidebar_order = [uri("b"), uri("c"), uri("a"), uri("d")].to_vec();
        app.settings.pinned_contexts = [uri("b"), uri("a")].to_vec();
        app.apply(Action::SetLibraryByYou(true), &egui::Context::default());
        let entries = projected(&app, "");
        assert!(entries.iter().any(|entry| entry.liked));
        assert_eq!(
            ids(&entries)
                .into_iter()
                .filter(|id| !id.is_empty())
                .collect::<Vec<_>>(),
            ["a", "c"]
        );
        assert_eq!(
            entries
                .iter()
                .find(|entry| entry.uri == uri("c"))
                .unwrap()
                .playlist_index,
            Some(2)
        );
        assert_eq!(ids(&projected(&app, "alpha")), ["c"]);
        assert!(projected(&app, "no match").is_empty());
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        assert_eq!(ids(&projected(&app, "alpha")), ["c"]);
        app.settings.pinned_contexts.clear();
        let named = projected(&app, "");
        assert_eq!(
            named
                .iter()
                .filter(|entry| !entry.liked)
                .map(|entry| entry.uri.as_str())
                .collect::<Vec<_>>(),
            [uri("c"), uri("a")]
        );
        app.apply(Action::SetLibraryByYou(false), &egui::Context::default());
        assert_eq!(projected(&app, "").len(), 5);
        assert_eq!(
            app.settings.sidebar_order,
            [uri("b"), uri("c"), uri("a"), uri("d")]
        );
        assert_eq!(selected_sort(&app, Filter::Playlists), LibrarySort::Name);
        app.backend.shutdown();
    }

    /// Ensure changing accounts reevaluates ownership and that missing or empty
    /// account identities never match playlists whose owners are unknown.
    #[test]
    fn by_you_rechecks_ownership_after_account_changes_and_keeps_missing_owners_out() {
        let mut app = ownership_app("by-you-account");
        app.settings.library_by_you = true;
        app.user.as_mut().unwrap().id = "someone-else".into();
        assert_eq!(ids(&projected(&app, "alpha")), ["b"]);
        app.user = None;
        assert!(projected(&app, "").iter().all(|entry| entry.liked));
        app.library.playlists.get_mut().unwrap()[3].owner.id = Some(String::new());
        assert!(projected(&app, "beta").is_empty());
        app.backend.shutdown();
    }

    /// By You keeps Spotify's tree: folders stay, rolled up or open as they
    /// were, holding only the account's own playlists, and a folder with
    /// none of them leaves the view. Turning it off restores the full tree
    /// without touching which folders are rolled up.
    #[test]
    fn by_you_keeps_folders_as_they_were_and_drops_the_empty_ones() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = ownership_app("by-you-folders");
        let folder = |id: &str| FolderStart {
            id: id.into(),
            name: id.into(),
        };
        // a and c are the account's own; b is someone else's.
        app.rootlist = vec![
            folder("mine"),
            Playlist(uri("a")),
            Playlist(uri("b")),
            FolderEnd,
            folder("theirs"),
            Playlist(uri("b")),
            FolderEnd,
            folder("rolled"),
            Playlist(uri("c")),
            FolderEnd,
        ];
        app.collapsed_folders = vec!["rolled".into()];
        app.settings.liked_songs_pinned = false;
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        let folders = |entries: &[Entry]| {
            entries
                .iter()
                .filter_map(|entry| entry.folder.as_ref())
                .map(|(id, collapsed, count)| (id.clone(), *collapsed, *count))
                .collect::<Vec<_>>()
        };

        app.settings.library_by_you = true;
        let entries = projected(&app, "");
        assert_eq!(
            folders(&entries),
            [
                ("mine".to_string(), false, 1),
                ("rolled".to_string(), true, 1)
            ]
        );
        // a shows inside its open folder; c stays inside the rolled-up one.
        let a = entries.iter().find(|entry| entry.uri == uri("a")).unwrap();
        assert_eq!(a.depth, 1);
        assert!(!entries.iter().any(|entry| entry.uri == uri("b")));
        assert!(!entries.iter().any(|entry| entry.uri == uri("c")));

        // Opening the folder under the filter shows what it holds.
        app.collapsed_folders.clear();
        assert!(
            projected(&app, "")
                .iter()
                .any(|entry| entry.uri == uri("c") && entry.depth == 1)
        );
        app.collapsed_folders = vec!["rolled".into()];

        app.settings.library_by_you = false;
        assert_eq!(
            folders(&projected(&app, "")),
            [
                ("mine".to_string(), false, 2),
                ("theirs".to_string(), false, 1),
                ("rolled".to_string(), true, 1)
            ]
        );
        assert_eq!(app.collapsed_folders, ["rolled"]);
        app.backend.shutdown();
    }

    /// Name order keeps folders too, and By You keeps them there as it does
    /// in Spotify's order: rolled up or open as they were, holding only the
    /// account's own playlists, a folder without any of them left out.
    #[test]
    fn by_you_keeps_folders_in_name_order() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = ownership_app("by-you-name-folders");
        let folder = |id: &str| FolderStart {
            id: id.into(),
            name: id.into(),
        };
        // a ("Zebra") and c are the account's own; b is someone else's.
        app.rootlist = vec![
            folder("mine"),
            Playlist(uri("a")),
            Playlist(uri("b")),
            FolderEnd,
            folder("theirs"),
            Playlist(uri("b")),
            FolderEnd,
            folder("rolled"),
            Playlist(uri("c")),
            FolderEnd,
        ];
        app.collapsed_folders = vec!["rolled".into()];
        app.settings.liked_songs_pinned = false;
        app.settings.library_by_you = true;
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        // Name, depth, and for a folder whether it is rolled up and its count.
        type Row = (String, u8, Option<(bool, usize)>);
        let rows: Vec<Row> = projected(&app, "")
            .iter()
            .map(|entry| {
                (
                    entry.name.clone(),
                    entry.depth,
                    entry
                        .folder
                        .as_ref()
                        .map(|(_, collapsed, count)| (*collapsed, *count)),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                ("Liked Songs".to_string(), 0, None),
                ("mine".to_string(), 0, Some((false, 1))),
                ("Zebra".to_string(), 1, None),
                ("rolled".to_string(), 0, Some((true, 1))),
            ]
        );
        app.backend.shutdown();
    }

    /// Verify indexed Spotify ordering uses the first duplicate rootlist entry,
    /// keeps unlisted playlists visible, and applies pins without rewriting data.
    #[test]
    fn by_you_spotify_order_keeps_first_occurrences_missing_rows_and_pins() {
        use crate::player::RootlistEntry;
        let mut app = ownership_app("by-you-rootlist-index");
        let owner = app.user_id().unwrap().to_string();
        app.library.playlists.get_mut().unwrap()[3].owner.id = Some(owner);
        app.settings.library_by_you = true;
        app.settings.liked_songs_pinned = false;
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        app.rootlist = vec![
            RootlistEntry::FolderStart {
                id: "folder".into(),
                name: "Folder".into(),
            },
            RootlistEntry::Playlist(uri("c")),
            RootlistEntry::Playlist(uri("b")),
            RootlistEntry::FolderEnd,
            RootlistEntry::Playlist(uri("a")),
            RootlistEntry::Playlist(uri("c")),
        ];
        let rootlist = app.rootlist.clone();
        // The folder stays, holding the account's c; b is someone else's.
        // Liked Songs leads, as in Spotify's custom order.
        assert_eq!(ids(&projected(&app, "")), ["", "", "c", "a", "d"]);
        app.settings.pinned_contexts = vec![uri("a")];
        assert_eq!(ids(&projected(&app, "")), ["a", "", "", "c", "d"]);
        assert_eq!(app.rootlist, rootlist);
        assert_eq!(
            app.library
                .playlists
                .get()
                .unwrap()
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b", "c", "d"]
        );
        app.backend.shutdown();
    }

    /// Check that pre-filter settings load with By You disabled and an enabled
    /// preference survives serialization without requiring a settings migration.
    #[test]
    fn by_you_defaults_off_in_older_settings_and_round_trips() {
        let mut settings: Settings = serde_json::from_str("{}").unwrap();
        assert!(!settings.library_by_you);
        settings.library_by_you = true;
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert!(restored.library_by_you);
    }

    #[test]
    fn grid_cards_use_smaller_art_than_page_headers() {
        let images = [64, 300, 640]
            .map(|width| crate::api::models::Image {
                url: width.to_string(),
                width: Some(width),
                height: Some(width),
            })
            .to_vec();
        let playlist = Playlist {
            images: images.clone(),
            ..Default::default()
        };
        let entry = playlist_entry(Locale::English, &playlist, 0, "", false, 0, TEST_ART);
        assert_eq!(entry.image.as_deref(), Some("64"));
        assert_eq!(entry.grid_image.as_deref(), Some("300"));
        assert_eq!(pick_image(&images, 640), Some("640"));
    }

    #[test]
    fn library_grid_adds_columns_as_the_sidebar_grows() {
        let narrow = grid_layout(230.0);
        let medium = grid_layout(360.0);
        let wide = grid_layout(500.0);
        assert_eq!(narrow.columns, 2);
        assert_eq!(medium.columns, 3);
        assert_eq!(wide.columns, 4);
        for (width, layout) in [(230.0, narrow), (360.0, medium), (500.0, wide)] {
            let filled = layout.card_width * layout.columns as f32;
            assert!((filled - width).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn grid_pin_and_play_controls_keep_opposite_corners() {
        let cover = Rect::from_min_size(pos2(10.0, 20.0), Vec2::splat(GRID_MIN_CARD_WIDTH));
        let pin = grid_pin_rect(cover);
        let play = grid_play_rect(cover);
        assert!(pin.center().x < cover.center().x && pin.center().y < cover.center().y);
        assert!(play.center().x > cover.center().x && play.center().y > cover.center().y);
        assert!(!pin.intersects(play));
    }

    #[test]
    fn grid_drop_positions_follow_visual_row_order() {
        let layout = grid_layout(360.0);
        let origin = pos2(20.0, 40.0);
        assert_eq!(grid_reorder_slot(pos2(21.0, 41.0), origin, layout, 8), 0);
        assert_eq!(
            grid_reorder_slot(
                pos2(origin.x + layout.card_width, origin.y + 1.0),
                origin,
                layout,
                8,
            ),
            1
        );
        assert_eq!(
            grid_reorder_slot(
                pos2(origin.x + 1.0, origin.y + layout.row_height + 1.0),
                origin,
                layout,
                8,
            ),
            3
        );
    }

    #[test]
    fn name_and_recent_sorts_keep_pins_and_do_not_rewrite_library_data() {
        let mut app = app("sorts");
        let mut entries = rows(&app);
        order_entries(
            &app,
            Filter::Playlists,
            LibrarySort::RecentlyPlayed,
            &mut entries,
        );
        assert_eq!(ids(&entries), ["d", "a", "b", "c"]);
        order_entries(&app, Filter::Playlists, LibrarySort::Name, &mut entries);
        assert_eq!(ids(&entries), ["b", "c", "d", "a"]);
        app.settings.pinned_contexts = vec![uri("a")];
        order_entries(&app, Filter::Playlists, LibrarySort::Name, &mut entries);
        assert_eq!(ids(&entries), ["a", "b", "c", "d"]);
        assert_eq!(ids(&rows(&app)), ["a", "b", "c", "d"]);
        app.backend.shutdown();
    }

    /// Duplicate saved keys keep their first rank, while rows absent from
    /// the saved order keep their relative positions without changing source data.
    #[test]
    fn ranked_sorts_keep_first_duplicate_and_stable_order_for_missing_entries() {
        use crate::player::RootlistEntry;

        let mut app = app("ranked-sorts");
        app.settings.liked_songs_pinned = false;
        app.settings.pinned_contexts.clear();
        app.recent_contexts = ["c", "a", "c"].map(uri).to_vec();
        app.settings.sidebar_order = ["c", "a", "c"].map(uri).to_vec();
        app.rootlist = ["c", "a", "c"]
            .map(|id| RootlistEntry::Playlist(uri(id)))
            .to_vec();

        for (sort, expected) in [
            (LibrarySort::RecentlyPlayed, ["c", "a", "b", "d"]),
            (LibrarySort::Local, ["b", "d", "c", "a"]),
            (LibrarySort::Spotify, ["c", "a", "b", "d"]),
        ] {
            let mut entries = rows(&app);
            order_entries(&app, Filter::Playlists, sort, &mut entries);
            assert_eq!(ids(&entries), expected, "{sort:?}");
        }

        app.settings.pinned_contexts = ["c", "a", "c"].map(uri).to_vec();
        let mut entries = rows(&app);
        order_entries(&app, Filter::Playlists, LibrarySort::Library, &mut entries);
        assert_eq!(ids(&entries), ["c", "a", "b", "d"]);
        assert_eq!(ids(&rows(&app)), ["a", "b", "c", "d"]);
        app.backend.shutdown();
    }

    #[test]
    fn switching_sort_and_filtered_drag_preserve_the_full_local_arrangement() {
        let mut app = app("saved");
        app.settings.sidebar_order = ["c", "a", "b", "d"].map(uri).to_vec();
        let saved = app.settings.sidebar_order.clone();
        assert_eq!(selected_sort(&app, Filter::Playlists), LibrarySort::Local);
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        assert_eq!(full_playlist_order(&app), ["b", "c", "d", "a"].map(uri));
        assert_eq!(app.settings.sidebar_order, saved);
        app.rootlist = vec![crate::player::RootlistEntry::Playlist(uri("d"))];
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Local);
        assert_eq!(
            full_playlist_order(&app),
            saved,
            "a late rootlist cannot replace the chosen local order"
        );
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        let filtered: Vec<_> = rows(&app)
            .into_iter()
            .filter(|row| row.uri == uri("c") || row.uri == uri("a"))
            .rev()
            .collect();
        drop_playlist_row(&mut app, &filtered, 0, 0, &uri("a"));
        apply_actions(&mut app);
        assert_eq!(app.settings.sidebar_order, ["b", "a", "c", "d"].map(uri));
        assert_eq!(selected_sort(&app, Filter::Playlists), LibrarySort::Local);
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        drop_playlist_row(&mut app, &filtered, 0, 0, &uri("a"));
        apply_actions(&mut app);
        assert_eq!(app.settings.sidebar_order, ["b", "a", "c", "d"].map(uri));
        assert_eq!(
            selected_sort(&app, Filter::Playlists),
            LibrarySort::Local,
            "recreating a saved arrangement still leaves the automatic sort"
        );
        app.backend.shutdown();
    }

    #[test]
    fn spotify_order_keeps_nested_folders_and_pins_visible_when_collapsed() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("folders");
        app.rootlist = vec![
            FolderStart {
                id: "outer".into(),
                name: "Outer".into(),
            },
            Playlist(uri("a")),
            FolderStart {
                id: "inner".into(),
                name: "Inner".into(),
            },
            Playlist(uri("b")),
            Playlist(uri("c")),
            FolderEnd,
            FolderEnd,
            Playlist(uri("d")),
            Playlist(uri("b")),
        ];
        app.collapsed_folders = vec!["inner".into()];
        app.settings.pinned_contexts = vec![uri("b")];
        let mut entries = vec![];
        folder_rows(&app, "", &mut entries);
        order_entries(&app, Filter::Playlists, LibrarySort::Spotify, &mut entries);
        assert_eq!(
            entries
                .iter()
                .map(|row| (row.name.as_str(), row.depth))
                .collect::<Vec<_>>(),
            [
                ("Alpha", 0),
                ("Outer", 0),
                ("Zebra", 1),
                ("Inner", 1),
                ("Beta", 0)
            ]
        );
        app.collapsed_folders.clear();
        let mut entries = vec![];
        folder_rows(&app, "", &mut entries);
        order_entries(&app, Filter::Playlists, LibrarySort::Spotify, &mut entries);
        assert_eq!(
            entries
                .iter()
                .map(|row| (row.name.as_str(), row.depth))
                .collect::<Vec<_>>(),
            [
                ("Alpha", 0),
                ("Outer", 0),
                ("Zebra", 1),
                ("Inner", 1),
                ("alpha", 2),
                ("Beta", 0)
            ]
        );
        app.backend.shutdown();
    }

    /// Name keeps playlists in their folders, as Spotify does: each folder's
    /// rows follow it in name order, folders sort among their siblings by
    /// name, and a pin still comes out of its folder to the top (#651).
    #[test]
    fn name_order_sorts_within_each_folder() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("name-folders");
        app.settings.liked_songs_pinned = false;
        app.settings.pinned_contexts.clear();
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        app.rootlist = vec![
            FolderStart {
                id: "outer".into(),
                name: "Outer".into(),
            },
            Playlist(uri("a")),
            FolderStart {
                id: "inner".into(),
                name: "Inner".into(),
            },
            Playlist(uri("c")),
            Playlist(uri("b")),
            FolderEnd,
            FolderEnd,
            Playlist(uri("d")),
        ];
        let rows = |app: &App| {
            projected(app, "")
                .iter()
                .map(|row| (row.name.clone(), row.depth))
                .collect::<Vec<_>>()
        };
        let expected = |rows: &[(&str, u8)]| {
            rows.iter()
                .map(|(name, depth)| (name.to_string(), *depth))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            rows(&app),
            expected(&[
                ("Beta", 0),
                ("Liked Songs", 0),
                ("Outer", 0),
                ("Inner", 1),
                ("Alpha", 2),
                ("alpha", 2),
                ("Zebra", 1),
            ])
        );
        app.settings.pinned_contexts = vec![uri("b")];
        assert_eq!(
            rows(&app),
            expected(&[
                ("Alpha", 0),
                ("Beta", 0),
                ("Liked Songs", 0),
                ("Outer", 0),
                ("Inner", 1),
                ("alpha", 2),
                ("Zebra", 1),
            ])
        );
        // A search still lists matching playlists flat.
        assert!(projected(&app, "a").iter().all(|row| row.depth == 0));
        app.backend.shutdown();
    }

    #[test]
    fn permute_places_each_entry_at_its_rank() {
        let mut items = ['a', 'b', 'c', 'd', 'e'];
        permute(&mut items, &[3, 0, 4, 1, 2]);
        assert_eq!(items, ['d', 'a', 'e', 'b', 'c']);
    }

    #[test]
    fn dragging_before_a_collapsed_folder_and_unpinning_keep_the_drop_position() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("folder-drop");
        app.rootlist = vec![
            FolderStart {
                id: "folder".into(),
                name: "Folder".into(),
            },
            Playlist(uri("b")),
            Playlist(uri("c")),
            FolderEnd,
            Playlist(uri("d")),
            Playlist(uri("a")),
        ];
        app.collapsed_folders = vec!["folder".into()];
        let mut entries = vec![];
        folder_rows(&app, "", &mut entries);
        order_entries(&app, Filter::Playlists, LibrarySort::Spotify, &mut entries);
        drop_playlist_row(&mut app, &entries, 0, 0, &uri("a"));
        apply_actions(&mut app);
        assert_eq!(app.settings.sidebar_order, ["a", "b", "c", "d"].map(uri));
        app.settings.sidebar_order.clear();
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        app.settings.pinned_contexts = vec![uri("a")];
        let mut entries = rows(&app);
        order_entries(&app, Filter::Playlists, LibrarySort::Name, &mut entries);
        drop_playlist_row(&mut app, &entries, 1, 2, &uri("a"));
        apply_actions(&mut app);
        assert_eq!(app.settings.library_pins(), [LIKED_SONGS_KEY]);
        assert_eq!(app.settings.sidebar_order, ["b", "a", "c", "d"].map(uri));
        app.backend.shutdown();
    }

    fn tree_rows(app: &App) -> Vec<Entry> {
        let mut entries = vec![];
        folder_rows(app, "", &mut entries);
        order_entries(app, Filter::Playlists, LibrarySort::Spotify, &mut entries);
        entries
    }

    /// With the session's tree shown in Spotify's order, a drag moves the
    /// row on Spotify: into the folder of the row it lands before, or a
    /// whole folder to the end. Nothing turns into a local order.
    #[test]
    fn dragging_in_spotifys_tree_moves_rows_on_spotify() {
        use crate::folders::{FolderEdit, start_marker};
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        // #given Spotify's order with a folder holding b and c
        let mut app = app("folder-tree-drop");
        app.settings.sidebar_order.clear();
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        // Demo data stands in for the library; the tree arrives as the
        // session's read would, so the app must take events.
        app.offline = false;
        app.receive_rootlist(vec![
            FolderStart {
                id: "folder".into(),
                name: "Folder".into(),
            },
            Playlist(uri("b")),
            Playlist(uri("c")),
            FolderEnd,
            Playlist(uri("d")),
            Playlist(uri("a")),
        ]);
        app.backend.take_folder_edit_requests();
        assert!(shows_spotify_tree(&app));

        // #when a is dropped in the gap above c
        let entries = tree_rows(&app);
        drop_playlist_row(&mut app, &entries, 0, 2, &uri("a"));
        apply_actions(&mut app);

        // #then it moves into the folder, on Spotify and on screen
        assert_eq!(
            app.backend.take_folder_edit_requests(),
            [FolderEdit::MovePlaylist {
                uri: uri("a"),
                place: Place::Before(Row::Playlist(uri("c"))),
            }]
        );
        assert_eq!(
            crate::folders::parent(&app.rootlist, &Row::Playlist(uri("a"))),
            Some("folder".into())
        );
        assert!(app.settings.sidebar_order.is_empty());
        assert_eq!(selected_sort(&app, Filter::Playlists), LibrarySort::Spotify);

        // #when the folder is dropped below the last row, it moves whole
        let entries = tree_rows(&app);
        drop_playlist_row(
            &mut app,
            &entries,
            0,
            entries.len(),
            &start_marker("folder", None),
        );
        apply_actions(&mut app);
        assert_eq!(
            app.backend.take_folder_edit_requests(),
            [FolderEdit::MoveFolder {
                id: "folder".into(),
                place: Place::End,
            }]
        );
        assert_eq!(app.rootlist.first(), Some(&Playlist(uri("d"))));
        assert_eq!(app.rootlist.last(), Some(&FolderEnd));

        // #when the folder is dropped among its own rows, it stays put
        let before = app.rootlist.clone();
        let entries = tree_rows(&app);
        let inside = entries
            .iter()
            .position(|entry| entry.uri == uri("a"))
            .unwrap();
        let toasts = app.toasts.len();
        drop_playlist_row(&mut app, &entries, 0, inside, &start_marker("folder", None));
        apply_actions(&mut app);
        assert!(app.backend.take_folder_edit_requests().is_empty());
        assert_eq!(app.rootlist, before);
        assert_eq!(app.toasts.len(), toasts, "staying put is not an error");
        app.backend.shutdown();
    }

    /// Before the session has read the tree, a drag keeps the local order it
    /// always made.
    #[test]
    fn without_the_sessions_tree_a_drag_stays_local() {
        let mut app = app("folder-tree-cached");
        app.settings.sidebar_order.clear();
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        app.rootlist = vec![
            RootlistEntry::Playlist(uri("b")),
            RootlistEntry::Playlist(uri("a")),
        ];
        assert!(!shows_spotify_tree(&app));
        let entries = tree_rows(&app);
        drop_playlist_row(&mut app, &entries, 0, 0, &uri("a"));
        apply_actions(&mut app);
        assert!(app.backend.take_folder_edit_requests().is_empty());
        assert!(!app.settings.sidebar_order.is_empty());
        app.backend.shutdown();
    }

    /// Name each row by its folder id or its playlist id, with its depth.
    fn tree_shape(entries: &[Entry]) -> Vec<(String, u8)> {
        entries
            .iter()
            .map(|entry| match &entry.folder {
                Some((id, _, _)) => (id.clone(), entry.depth),
                None => (
                    entry.uri.rsplit(':').next().unwrap().to_string(),
                    entry.depth,
                ),
            })
            .collect()
    }

    /// A folder opens to what it holds, from its own rows down, however it
    /// is rolled up in the tree; Liked Songs and the rest of the library stay
    /// outside. Its subtitle counts what it holds itself, as Spotify does.
    #[test]
    fn an_opened_folder_shows_only_what_it_holds() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("opened-folder");
        app.settings.sidebar_order.clear();
        app.settings.pinned_contexts.clear();
        app.settings.liked_songs_pinned = false;
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        let folder = |id: &str| FolderStart {
            id: id.into(),
            name: id.into(),
        };
        // d is newly followed: the tree has not met it yet.
        app.rootlist = vec![
            Playlist(uri("a")),
            folder("outer"),
            Playlist(uri("b")),
            folder("inner"),
            Playlist(uri("c")),
            FolderEnd,
            FolderEnd,
            folder("shelf"),
            folder("x"),
            FolderEnd,
            folder("y"),
            FolderEnd,
            FolderEnd,
        ];

        // #then a folder counts its own playlists and its own folders
        let top = projected(&app, "");
        let row = |id: &str| {
            top.iter()
                .find(|entry| entry.folder.as_ref().is_some_and(|(held, _, _)| held == id))
                .unwrap()
        };
        assert_eq!(
            row("outer").subtitle,
            "Folder \u{2022} 1 playlist, 1 folder"
        );
        assert_eq!(row("outer").grid_subtitle, "1 playlist, 1 folder");
        assert_eq!(row("inner").subtitle, "Folder \u{2022} 1 playlist");
        assert_eq!(row("inner").grid_subtitle, "1 playlist");
        assert_eq!(row("shelf").subtitle, "Folder \u{2022} 2 folders");
        assert_eq!(row("shelf").grid_subtitle, "2 folders");
        assert_eq!(row("x").subtitle, "Folder \u{2022} 0 playlists");
        assert_eq!(ids(&top).last(), Some(&"d"), "d waits at the end");

        // #when the folder, rolled up in the tree, is opened
        app.collapsed_folders = vec!["outer".into()];
        app.actions
            .push(Action::EnterLibraryFolder(Some("outer".into())));
        apply_actions(&mut app);

        // #then only what it holds is listed, from its own depth; d, in no
        // folder, stays outside
        assert_eq!(
            tree_shape(&projected(&app, "")),
            [("b".into(), 0), ("inner".into(), 0), ("c".into(), 1)]
        );

        // #then a flat sort lists everything inside it, nested rows too
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::RecentlyPlayed);
        let flat = projected(&app, "");
        let mut flat = ids(&flat);
        flat.sort_unstable();
        assert_eq!(flat, ["b", "c"]);
        // Every fixture name holds an "a"; only the folder's are found.
        let found = projected(&app, "a");
        let mut found = ids(&found);
        found.sort_unstable();
        assert_eq!(found, ["b", "c"], "a search stays in the folder");

        // #then a folder that is gone opens nothing: the library is back
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        app.rootlist
            .retain(|row| !matches!(row, FolderStart { id, .. } if id == "outer"));
        app.rootlist.remove(
            app.rootlist
                .iter()
                .rposition(|row| *row == FolderEnd)
                .unwrap(),
        );
        let back = projected(&app, "");
        assert!(back[0].liked);
        assert_eq!(back.len(), 9);

        // #then the way back leads up to the whole library
        app.actions.push(Action::EnterLibraryFolder(None));
        apply_actions(&mut app);
        assert_eq!(app.library_folder, None);
        app.backend.shutdown();
    }

    /// Dropped below the last row of an opened folder, a playlist stays in
    /// that folder: after its last playlist, or just before a last folder,
    /// as near as Spotify's anchors reach.
    #[test]
    fn a_drop_below_an_opened_folder_stays_in_it() {
        use crate::folders::FolderEdit;
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("opened-folder-drop");
        app.settings.sidebar_order.clear();
        app.settings.pinned_contexts.clear();
        app.settings.liked_songs_pinned = false;
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        app.offline = false;
        let folder = |id: &str| FolderStart {
            id: id.into(),
            name: id.into(),
        };
        app.receive_rootlist(vec![
            folder("outer"),
            folder("inner"),
            Playlist(uri("c")),
            FolderEnd,
            Playlist(uri("b")),
            FolderEnd,
            Playlist(uri("d")),
            Playlist(uri("a")),
        ]);
        app.backend.take_folder_edit_requests();
        app.library_folder = Some("outer".into());

        // #when c leaves its folder for the bottom of the opened one
        let entries = projected(&app, "");
        assert_eq!(
            tree_shape(&entries),
            [("inner".into(), 0), ("c".into(), 1), ("b".into(), 0)]
        );
        drop_playlist_row(&mut app, &entries, 0, entries.len(), &uri("c"));
        apply_actions(&mut app);

        // #then it lands after b, still in the opened folder
        assert_eq!(
            app.backend.take_folder_edit_requests(),
            [FolderEdit::MovePlaylist {
                uri: uri("c"),
                place: Place::AfterPlaylist(uri("b")),
            }]
        );
        assert_eq!(
            crate::folders::parent(&app.rootlist, &Row::Playlist(uri("c"))),
            Some("outer".into())
        );

        // #when the opened folder ends in a folder, a drop below lands
        // just before that folder
        app.library_folder = Some("outer".into());
        app.rootlist = vec![
            folder("outer"),
            Playlist(uri("b")),
            Playlist(uri("c")),
            folder("inner"),
            FolderEnd,
            FolderEnd,
            Playlist(uri("d")),
            Playlist(uri("a")),
        ];
        let entries = projected(&app, "");
        drop_playlist_row(&mut app, &entries, 0, entries.len(), &uri("b"));
        apply_actions(&mut app);
        assert_eq!(
            app.backend.take_folder_edit_requests(),
            [FolderEdit::MovePlaylist {
                uri: uri("b"),
                place: Place::Before(Row::Folder("inner".into())),
            }]
        );
        assert_eq!(
            crate::folders::parent(&app.rootlist, &Row::Playlist(uri("b"))),
            Some("outer".into())
        );
        app.backend.shutdown();
    }

    /// Back leads to the folder around the opened one, then to the top.
    /// The + button's folder goes inside the opened one, an empty opened
    /// folder takes a drop first inside it, and below a folder that ends in
    /// a folder the drop line shows above that folder, where a row lands.
    /// A folder that goes away closes.
    #[test]
    fn an_opened_folder_knows_its_way_back_and_where_rows_go() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("opened-folder-ways");
        app.settings.sidebar_order.clear();
        app.settings.pinned_contexts.clear();
        app.settings.liked_songs_pinned = false;
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        app.offline = false;
        let folder = |id: &str| FolderStart {
            id: id.into(),
            name: id.into(),
        };
        app.receive_rootlist(vec![
            folder("outer"),
            Playlist(uri("b")),
            folder("inner"),
            FolderEnd,
            FolderEnd,
            Playlist(uri("d")),
            Playlist(uri("a")),
        ]);
        app.backend.take_folder_edit_requests();
        assert_eq!(way_back(&app, "inner"), Some("outer".into()));
        assert_eq!(way_back(&app, "outer"), None);
        assert_eq!(new_folder_place(&app, Filter::Playlists), Place::Start);

        app.library_folder = Some("inner".into());
        assert_eq!(
            new_folder_place(&app, Filter::Playlists),
            Place::InFolder("inner".into())
        );
        assert_eq!(new_folder_place(&app, Filter::Albums), Place::Start);
        assert_eq!(end_place(&app), Place::InFolder("inner".into()));

        app.library_folder = Some("outer".into());
        let entries = projected(&app, "");
        assert_eq!(tree_shape(&entries), [("b".into(), 0), ("inner".into(), 0)]);
        assert_eq!(landing_slot(&app, &entries, entries.len()), 1);
        assert_eq!(landing_slot(&app, &entries, 0), 0);
        app.library_folder = None;
        let entries = projected(&app, "");
        assert_eq!(landing_slot(&app, &entries, entries.len()), entries.len());

        app.library_folder = Some("inner".into());
        app.receive_rootlist(vec![
            folder("outer"),
            Playlist(uri("b")),
            FolderEnd,
            Playlist(uri("d")),
            Playlist(uri("a")),
        ]);
        assert_eq!(app.library_folder, None);
        app.backend.shutdown();
    }

    /// The kinds are checkboxes: Playlists alone to begin with, each click
    /// ticks or unticks one, and none ticked lists every kind.
    #[test]
    fn library_kinds_tick_like_checkboxes() {
        let shown = Shown::default();
        assert_eq!(shown.shelves(), [Filter::Playlists]);
        let both = shown.toggled(Filter::Albums);
        assert_eq!(both.shelves(), [Filter::Playlists, Filter::Albums]);
        let albums = both.toggled(Filter::Playlists);
        assert_eq!(albums.shelves(), [Filter::Albums]);
        assert_eq!(albums.toggled(Filter::Albums).shelves(), SHELVES);
        assert!(Shown::only(Filter::Podcasts).has(Filter::Podcasts));
        assert!(!Shown::only(Filter::Podcasts).has(Filter::Artists));
    }

    /// Several kinds share one order, chosen among those they all take,
    /// and kept together a kind at a time, a thin line between kinds; or
    /// mixed in one flat list. A folder opened into lists only itself.
    #[test]
    fn several_kinds_list_grouped_or_mixed_in_one_order() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("mixed-kinds");
        app.settings.pinned_contexts.clear();
        app.settings.liked_songs_pinned = false;
        let shelves = [Filter::Playlists, Filter::Albums];
        assert_eq!(
            mixed_sorts(&shelves),
            [LibrarySort::RecentlyPlayed, LibrarySort::Name]
        );
        assert!(
            mixed_sorts(&[Filter::Albums, Filter::Podcasts]).contains(&LibrarySort::RecentlyAdded)
        );
        app.settings.library_mixed_sort = Some(LibrarySort::RecentlyAdded);
        assert_eq!(
            mixed_sort(&app, &shelves),
            LibrarySort::RecentlyPlayed,
            "playlists have no dates"
        );
        app.settings.library_mixed_sort = Some(LibrarySort::Name);
        assert_eq!(mixed_sort(&app, &shelves), LibrarySort::Name);
        // A sort chosen for playlists alone stays theirs.
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::RecentlyPlayed);
        app.rootlist = vec![
            FolderStart {
                id: "f".into(),
                name: "Folder".into(),
            },
            Playlist(uri("a")),
            FolderEnd,
            Playlist(uri("b")),
            Playlist(uri("c")),
            Playlist(uri("d")),
        ];

        // #then grouped: the playlists in name order, folder and all, then
        // the albums in name order, from the line between them
        app.settings.library_group_kinds = true;
        let grouped = library_rows(&app, &shelves, "");
        let start = &grouped.group_starts[..];
        let &[albums_from] = start else {
            panic!("{start:?}")
        };
        assert!(
            grouped.entries[..albums_from]
                .iter()
                .all(|entry| entry.folder.is_some()
                    || entry.liked
                    || entry.uri.starts_with("spotify:playlist:"))
        );
        assert!(
            grouped.entries[..albums_from]
                .iter()
                .any(|entry| entry.folder.is_some())
        );
        assert!(
            grouped.entries[albums_from..]
                .iter()
                .all(|entry| entry.uri.starts_with("spotify:album:"))
        );
        let names = |entries: &[Entry]| -> Vec<String> {
            entries
                .iter()
                .map(|entry| entry.name.to_lowercase())
                .collect()
        };
        let mut sorted = names(&grouped.entries[albums_from..]);
        sorted.sort();
        assert_eq!(names(&grouped.entries[albums_from..]), sorted);

        // #then mixed: one flat list in name order, no folders, no lines
        app.settings.library_group_kinds = false;
        let mixed = library_rows(&app, &shelves, "");
        assert!(mixed.group_starts.is_empty());
        assert!(
            mixed
                .entries
                .iter()
                .all(|entry| entry.folder.is_none() && entry.depth == 0)
        );
        assert!(
            mixed
                .entries
                .iter()
                .any(|entry| entry.uri.starts_with("spotify:album:"))
        );
        let mut sorted = names(&mixed.entries);
        sorted.sort();
        assert_eq!(names(&mixed.entries), sorted);
        assert_eq!(
            mixed.entries.len(),
            grouped.entries.len() - 1,
            "all but the folder row"
        );

        // #then one kind keeps its own order
        let alone = library_rows(&app, &[Filter::Playlists], "");
        assert!(alone.group_starts.is_empty());

        // #then a folder opened into lists only itself, whatever is ticked
        let shown = Shown::default().toggled(Filter::Albums);
        assert_eq!(listed_shelves(&app, shown), shelves);
        app.library_folder = Some("f".into());
        assert_eq!(listed_shelves(&app, shown), [Filter::Playlists]);
        assert_eq!(
            listed_shelves(&app, Shown::only(Filter::Albums)),
            [Filter::Albums]
        );
        app.backend.shutdown();
    }

    /// Local Files joins the Library once there is a folder or song to
    /// play from, right after Liked Songs in Spotify's order; it can be
    /// searched for, and stays out of an opened folder.
    #[test]
    fn local_files_join_the_library_after_liked_songs() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("local-files-row");
        app.settings.pinned_contexts.clear();
        app.settings.liked_songs_pinned = false;
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        let local = |rows: &[Entry]| rows.iter().position(|entry| entry.page == Page::LocalFiles);
        assert_eq!(local(&projected(&app, "")), None, "nowhere to look yet");
        app.settings
            .set_local_files_folders(&[std::path::PathBuf::from("/music")]);
        let rows = projected(&app, "");
        assert!(rows[0].liked);
        assert_eq!(local(&rows), Some(1));
        assert!(rows[1].uri.is_empty(), "never sent to Spotify");
        assert!(local(&projected(&app, "local")).is_some());
        assert_eq!(local(&projected(&app, "zebra")), None);
        app.rootlist = vec![
            FolderStart {
                id: "f".into(),
                name: "f".into(),
            },
            Playlist(uri("a")),
            FolderEnd,
        ];
        app.library_folder = Some("f".into());
        assert_eq!(local(&projected(&app, "")), None);
        app.backend.shutdown();
    }

    /// In the grid, each kind starts a fresh row.
    #[test]
    fn grid_kinds_start_fresh_rows() {
        assert_eq!(grid_rows(5, 2, &[]), [(0, 2), (2, 4), (4, 5)]);
        assert_eq!(grid_rows(5, 2, &[3]), [(0, 2), (2, 3), (3, 5)]);
        assert_eq!(grid_rows(4, 3, &[1, 2]), [(0, 1), (1, 2), (2, 4)]);
        assert_eq!(grid_rows(0, 3, &[]), []);
        assert_eq!(
            grid_rows(3, 3, &[0, 3, 9]),
            [(0, 3)],
            "out-of-range starts are ignored"
        );
    }

    /// A pinned folder rises to the top with what it holds, open or rolled
    /// up as it was, and the pin block counts its rows; a folder dropped
    /// among the pins is pinned on Spotify.
    #[test]
    fn a_pinned_folder_rises_with_what_it_holds() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("pinned-folder");
        app.offline = false;
        app.settings.sidebar_order.clear();
        app.settings.pinned_contexts.clear();
        app.settings.liked_songs_pinned = false;
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        let folder = |id: &str| FolderStart {
            id: id.into(),
            name: id.into(),
        };
        app.receive_rootlist(vec![
            Playlist(uri("a")),
            folder("f1"),
            Playlist(uri("b")),
            folder("f2"),
            Playlist(uri("c")),
            FolderEnd,
            FolderEnd,
            folder("f3"),
            FolderEnd,
            Playlist(uri("d")),
        ]);
        let user = app.user_id().unwrap().to_string();
        app.spotify_pins = Some(vec![crate::pins::folder_uri(&user, "f1")]);
        app.backend.take_pin_requests();

        let entries = projected(&app, "");
        let shape: Vec<(&str, u8)> = entries
            .iter()
            .map(|entry| match &entry.folder {
                Some((id, _, _)) => (id.as_str(), entry.depth),
                None => (entry.uri.rsplit(':').next().unwrap(), entry.depth),
            })
            .collect();
        assert_eq!(
            shape,
            [
                ("f1", 0),
                ("b", 1),
                ("f2", 1),
                ("c", 2),
                ("", 0),
                ("a", 0),
                ("f3", 0),
                ("d", 0)
            ]
        );
        assert_eq!(pinned_rows(&entries, &app.library_pins()), 4);

        app.collapsed_folders = vec!["f1".into()];
        let entries = projected(&app, "");
        assert_eq!(pinned_rows(&entries, &app.library_pins()), 1);

        // f3 dropped into the pin block is pinned, by its library URI.
        drop_playlist_row(
            &mut app,
            &entries,
            1,
            0,
            &crate::folders::start_marker("f3", None),
        );
        apply_actions(&mut app);
        assert_eq!(
            app.backend.take_pin_requests(),
            [(crate::pins::folder_uri(&user, "f3"), true)]
        );
        app.backend.shutdown();
    }

    /// Ranking keeps everything unpinned in its order. Unstable sorts keep
    /// small lists in order by chance; a library is large.
    #[test]
    fn ranking_keeps_a_large_library_in_its_order() {
        let mut items: Vec<usize> = (0..500).collect();
        let mut ranks = vec![usize::MAX; 500];
        ranks[321] = 0;
        ranks[7] = 1;
        sort_by_ranks(&mut items, &ranks);
        let mut expected = vec![321, 7];
        expected.extend((0..500).filter(|item| *item != 321 && *item != 7));
        assert_eq!(items, expected);
    }

    #[test]
    fn a_menu_never_offers_a_folder_itself_or_where_it_already_is() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let folder = |id: &str| FolderStart {
            id: id.into(),
            name: id.into(),
        };
        let rows = vec![
            folder("outer"),
            folder("inner"),
            Playlist(uri("a")),
            FolderEnd,
            FolderEnd,
            folder("other"),
            FolderEnd,
            Playlist(uri("b")),
        ];
        let ids = |targets: &[crate::folders::Folder]| {
            targets
                .iter()
                .map(|folder| folder.id.clone())
                .collect::<Vec<_>>()
        };
        // A playlist in "inner" can leave it, or go anywhere else.
        let (out, targets) = folder_targets(&rows, &Row::Playlist(uri("a"))).unwrap();
        assert_eq!(out, Some(Place::Before(Row::Folder("inner".into()))));
        assert_eq!(ids(&targets), ["outer", "other"]);
        // A top-level playlist has nothing to leave.
        let (out, targets) = folder_targets(&rows, &Row::Playlist(uri("b"))).unwrap();
        assert_eq!(out, None);
        assert_eq!(ids(&targets), ["outer", "inner", "other"]);
        // "outer" can go only into "other": not itself, not "inner".
        let (out, targets) = folder_targets(&rows, &Row::Folder("outer".into())).unwrap();
        assert_eq!(out, None);
        assert_eq!(ids(&targets), ["other"]);
        // "inner" can leave "outer", or go into "other".
        let (out, targets) = folder_targets(&rows, &Row::Folder("inner".into())).unwrap();
        assert_eq!(out, Some(Place::Before(Row::Folder("outer".into()))));
        assert_eq!(ids(&targets), ["other"]);
        assert!(folder_targets(&rows, &Row::Playlist(uri("gone"))).is_none());
    }

    #[test]
    fn folder_names_and_subtitles_follow_the_locale() {
        use crate::i18n::Locale;
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("folder-locale");
        app.locale = Locale::German;
        app.rootlist = vec![
            FolderStart {
                id: "folder".into(),
                name: String::new(),
            },
            Playlist(uri("a")),
            Playlist(uri("b")),
            FolderEnd,
        ];
        let mut entries = vec![];
        folder_rows(&app, "", &mut entries);
        assert_eq!(entries[0].name, "Ordner");
        assert_eq!(entries[0].subtitle, "Ordner • 2 Playlists");
        assert_eq!(entries[0].grid_subtitle, "2 Playlists");
        app.backend.shutdown();
    }

    #[test]
    fn added_sort_uses_actual_instants_and_keeps_missing_dates_last() {
        let mut app = app("dates");
        let mut entries = rows(&app);
        for (row, value) in entries.iter_mut().zip([
            Some("2026-09-09T09:00:00Z"),
            Some("2026-09-09T10:00:00+02:00"),
            None,
            Some("unknown"),
        ]) {
            row.added_at = saved_time(value);
        }
        entries.swap(0, 1);
        order_entries(
            &app,
            Filter::Albums,
            LibrarySort::RecentlyAdded,
            &mut entries,
        );
        assert_eq!(ids(&entries), ["a", "b", "c", "d"]);
        assert!(!LibrarySort::RecentlyAdded.supports(Filter::Playlists));
        assert!(!LibrarySort::RecentlyAdded.supports(Filter::Artists));
        assert!(LibrarySort::RecentlyAdded.supports(Filter::Podcasts));
        app.backend.shutdown();
    }

    #[test]
    fn preferences_round_trip_and_new_playlists_still_precede_saved_order() {
        let mut app = app("migration");
        app.settings = serde_json::from_str(
            r#"{"sidebar_order":["spotify:playlist:c","spotify:playlist:a","spotify:playlist:b"]}"#,
        )
        .unwrap();
        assert_eq!(selected_sort(&app, Filter::Playlists), LibrarySort::Local);
        assert_eq!(selected_sort(&app, Filter::Albums), LibrarySort::Library);
        assert_eq!(full_playlist_order(&app), ["d", "c", "a", "b"].map(uri));
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        app.settings
            .library_sort
            .insert(Filter::Albums, LibrarySort::RecentlyAdded);
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&app.settings).unwrap()).unwrap();
        assert_eq!(restored, app.settings);
        app.backend.shutdown();
    }

    #[test]
    fn dropping_at_the_end_of_the_pin_block_keeps_the_item_pinned() {
        let mut app = app("pin-boundary");
        app.settings.pinned_contexts = [uri("d"), uri("a")].to_vec();
        let mut entries = rows(&app);
        entries.push(liked_entry(&app));
        order_entries(
            &app,
            Filter::Playlists,
            selected_sort(&app, Filter::Playlists),
            &mut entries,
        );

        drop_playlist_row(&mut app, &entries, 3, 3, LIKED_SONGS_KEY);
        apply_actions(&mut app);

        assert_eq!(
            app.settings.library_pins(),
            [uri("d"), uri("a"), LIKED_SONGS_KEY.into()]
        );
        app.backend.shutdown();
    }

    #[test]
    fn liked_songs_moves_among_pins_and_retains_its_local_position() {
        let mut app = app("liked-position");
        app.settings.pinned_contexts = [uri("d"), uri("a")].to_vec();
        let ordered = |app: &App| {
            let mut entries = rows(app);
            entries.push(liked_entry(app));
            order_entries(
                app,
                Filter::Playlists,
                selected_sort(app, Filter::Playlists),
                &mut entries,
            );
            entries
        };
        let entries = ordered(&app);
        assert_eq!(entries[0].ordering_key(), LIKED_SONGS_KEY);
        drop_playlist_row(&mut app, &entries, 3, 2, LIKED_SONGS_KEY);
        assert_eq!(
            app.settings.library_pins()[0],
            LIKED_SONGS_KEY,
            "the view only emits an action"
        );
        apply_actions(&mut app);
        assert_eq!(
            app.settings.library_pins(),
            [uri("d"), LIKED_SONGS_KEY.into(), uri("a")]
        );
        assert!(app.settings.sidebar_order.is_empty());
        let entries = ordered(&app);
        drop_playlist_row(&mut app, &entries, 3, 4, LIKED_SONGS_KEY);
        apply_actions(&mut app);
        assert!(!app.settings.liked_songs_pinned);
        let saved = [uri("b"), LIKED_SONGS_KEY.into(), uri("c")];
        assert_eq!(full_playlist_order(&app), saved);
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Name);
        assert_eq!(
            full_playlist_order(&app),
            [uri("b"), uri("c"), LIKED_SONGS_KEY.into()]
        );
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Local);
        app.rootlist = vec![crate::player::RootlistEntry::Playlist(uri("c"))];
        app.user.as_mut().unwrap().id = "another-account".into();
        app.settings =
            serde_json::from_str(&serde_json::to_string(&app.settings).unwrap()).unwrap();
        assert_eq!(
            full_playlist_order(&app),
            saved,
            "restart and account/rootlist changes preserve the local placement"
        );
        let filtered: Vec<_> = ordered(&app)
            .into_iter()
            .filter(|entry| entry.uri == uri("c") || entry.uri == uri("b"))
            .collect();
        drop_playlist_row(&mut app, &filtered, 0, 0, &uri("c"));
        apply_actions(&mut app);
        assert_eq!(
            full_playlist_order(&app),
            [uri("c"), uri("b"), LIKED_SONGS_KEY.into()],
            "a hidden Liked Songs keeps its place in the full arrangement"
        );
        assert!(
            liked_entry(&app).uri.is_empty(),
            "the ordering key is never a Spotify URI"
        );
        app.backend.shutdown();
    }

    #[test]
    fn unpinned_liked_songs_follows_recent_play_and_spotify_order_without_entering_folders() {
        use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
        let mut app = app("liked-sorts");
        app.settings.liked_songs_pinned = false;
        app.recent_contexts = vec![
            uri("a"),
            format!("spotify:user:{}:collection", app.user_id().unwrap()),
            uri("c"),
        ];
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::RecentlyPlayed);
        assert_eq!(
            full_playlist_order(&app),
            [
                uri("a"),
                LIKED_SONGS_KEY.into(),
                uri("c"),
                uri("b"),
                uri("d")
            ]
        );
        app.rootlist = vec![
            FolderStart {
                id: "folder".into(),
                name: "Folder".into(),
            },
            Playlist(uri("c")),
            FolderEnd,
            Playlist(uri("a")),
        ];
        app.collapsed_folders = vec!["folder".into()];
        app.settings
            .library_sort
            .insert(Filter::Playlists, LibrarySort::Spotify);
        let mut entries = vec![liked_entry(&app)];
        folder_rows(&app, "", &mut entries);
        order_entries(&app, Filter::Playlists, LibrarySort::Spotify, &mut entries);
        // Spotify's custom order puts an unpinned Liked Songs first.
        assert!(entries[0].liked);
        assert_eq!(entries[0].depth, 0);
        drop_playlist_row(&mut app, &entries, 0, 0, LIKED_SONGS_KEY);
        apply_actions(&mut app);
        assert_eq!(
            full_playlist_order(&app),
            [
                LIKED_SONGS_KEY.into(),
                uri("c"),
                uri("a"),
                uri("b"),
                uri("d")
            ]
        );
        let mut entries = rows(&app);
        entries.push(liked_entry(&app));
        let end = entries.len();
        drop_playlist_row(&mut app, &entries, 0, end, LIKED_SONGS_KEY);
        apply_actions(&mut app);
        assert_eq!(full_playlist_order(&app).last().unwrap(), LIKED_SONGS_KEY);
        app.backend.shutdown();
    }
}
