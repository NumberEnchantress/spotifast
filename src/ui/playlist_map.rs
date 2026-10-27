//! Home as a map of the playlists: one node a playlist, sized by its songs,
//! linked to the Spotify folder that holds it, the folders to theirs, and
//! the top level to the library at the centre. The nodes find their places
//! by a small force layout that runs only while they move; the map pans,
//! zooms, and opens what is clicked.

use std::collections::{HashMap, HashSet};

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2, vec2};

use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, Page};
use crate::player::RootlistEntry;
use crate::theme::{self, Icon};

/// Where the map's nodes are and how they move, kept between frames and
/// visits to Home.
#[derive(Default)]
pub struct MapState {
    nodes: HashMap<String, Body>,
    pan: Vec2,
    /// Zero until first drawn, then the scale of the map on screen.
    zoom: f32,
    /// Layout steps still to run; the layout rests at zero.
    steps_left: u32,
    /// What the graph was built from last, to wake the layout on a change.
    shape: u64,
}

#[derive(Clone, Copy, Default)]
struct Body {
    at: Vec2,
    speed: Vec2,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    Library,
    Liked,
    Folder(String),
    Playlist { id: String, uri: String },
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub(crate) key: String,
    pub(crate) kind: Kind,
    pub(crate) name: String,
    pub(crate) radius: f32,
    pub(crate) songs: Option<u32>,
    image: Option<String>,
}

/// A playlist's radius: a small dot for an empty one, growing with the
/// square root of its songs so large ones do not swamp the rest.
pub(crate) fn playlist_radius(songs: u32) -> f32 {
    (9.0 + 3.0 * (songs as f32).sqrt()).min(46.0)
}

/// The nodes and the links between them, by index: the library first,
/// then Liked Songs, the folders and playlists in Spotify's order, and the
/// playlists the folder tree does not hold at the top level.
pub(crate) fn graph(app: &App) -> (Vec<Node>, Vec<(usize, usize)>) {
    let locale = app.locale;
    let mut nodes = vec![Node {
        key: "library".into(),
        kind: Kind::Library,
        name: gettext(locale, "Library").into_owned(),
        radius: 18.0,
        songs: None,
        image: None,
    }];
    let mut links = Vec::new();
    nodes.push(Node {
        key: "liked".into(),
        kind: Kind::Liked,
        name: gettext(locale, "Liked Songs").into_owned(),
        radius: playlist_radius(app.library.liked.total.unwrap_or(0)),
        songs: app.library.liked.total,
        image: None,
    });
    links.push((0, 1));
    let Some(playlists) = app.library.playlists.get() else {
        return (nodes, links);
    };
    let by_uri: HashMap<&str, &crate::api::models::Playlist> = playlists
        .iter()
        .map(|playlist| (playlist.uri.as_str(), playlist))
        .collect();
    let mut placed: HashSet<&str> = HashSet::new();
    let playlist_node = |playlist: &crate::api::models::Playlist| Node {
        key: playlist.uri.clone(),
        kind: Kind::Playlist {
            id: playlist.id.clone(),
            uri: playlist.uri.clone(),
        },
        name: playlist.name.clone(),
        radius: playlist_radius(playlist.track_total()),
        songs: Some(playlist.track_total()),
        image: crate::api::models::pick_image(&playlist.images, 160).map(str::to_string),
    };
    let mut open: Vec<usize> = vec![0];
    let mut folders: HashSet<&str> = HashSet::new();
    for row in &app.rootlist {
        let parent = *open.last().unwrap_or(&0);
        match row {
            RootlistEntry::FolderStart { id, name } => {
                nodes.push(Node {
                    // A folder id met twice gets a key of its own each time.
                    key: if folders.insert(id.as_str()) {
                        format!("folder:{id}")
                    } else {
                        format!("folder:{id}#{}", nodes.len())
                    },
                    kind: Kind::Folder(id.clone()),
                    name: if name.is_empty() {
                        gettext(locale, "Folder").into_owned()
                    } else {
                        name.clone()
                    },
                    radius: 14.0,
                    songs: None,
                    image: None,
                });
                links.push((parent, nodes.len() - 1));
                open.push(nodes.len() - 1);
            }
            RootlistEntry::FolderEnd => {
                if open.len() > 1 {
                    open.pop();
                }
            }
            RootlistEntry::Playlist(uri) => {
                if let Some(playlist) = by_uri.get(uri.as_str())
                    && placed.insert(uri.as_str())
                {
                    nodes.push(playlist_node(playlist));
                    links.push((parent, nodes.len() - 1));
                }
            }
        }
    }
    for playlist in playlists {
        if placed.insert(playlist.uri.as_str()) {
            nodes.push(playlist_node(playlist));
            links.push((0, nodes.len() - 1));
        }
    }
    (nodes, links)
}

/// A stable spread of starting angles, so a new node lands beside its
/// parent at the same place every time.
fn angle_of(key: &str) -> f32 {
    let hash = key.bytes().fold(2_166_136_261u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    });
    (hash % 6283) as f32 / 1000.0
}

/// Runs the layout `steps` times: every pair of nodes pushes apart, each
/// link pulls its ends toward a length that fits them, and a faint pull
/// keeps the whole near the centre. `held` stays where it is put.
pub(crate) fn settle(
    state: &mut MapState,
    nodes: &[Node],
    links: &[(usize, usize)],
    held: Option<&str>,
    steps: u32,
) {
    let mut bodies: Vec<Body> = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        let body = state.nodes.get(&node.key).copied().unwrap_or_else(|| {
            let parent = links
                .iter()
                .find(|(_, child)| *child == index)
                .and_then(|(parent, _)| bodies.get(*parent).copied())
                .map_or(Vec2::ZERO, |body| body.at);
            let angle = angle_of(&node.key);
            Body {
                at: parent + vec2(angle.cos(), angle.sin()) * (40.0 + node.radius * 2.0),
                speed: Vec2::ZERO,
            }
        });
        bodies.push(body);
    }
    let held = held.and_then(|key| nodes.iter().position(|node| node.key == key));
    for _ in 0..steps {
        let mut forces = vec![Vec2::ZERO; bodies.len()];
        for a in 0..bodies.len() {
            for b in a + 1..bodies.len() {
                let apart = bodies[a].at - bodies[b].at;
                let distance = apart.length().max(1.0);
                let reach = nodes[a].radius + nodes[b].radius;
                let push = (1800.0 + reach * 40.0) / (distance * distance);
                let direction = if apart.length() < 0.01 {
                    vec2(angle_of(&nodes[a].key).cos(), angle_of(&nodes[a].key).sin())
                } else {
                    apart / distance
                };
                forces[a] += direction * push;
                forces[b] -= direction * push;
            }
        }
        for &(a, b) in links {
            let apart = bodies[b].at - bodies[a].at;
            let distance = apart.length().max(1.0);
            let rest = nodes[a].radius + nodes[b].radius + 36.0;
            let pull = (distance - rest) * 0.04;
            forces[a] += apart / distance * pull;
            forces[b] -= apart / distance * pull;
        }
        for (index, body) in bodies.iter_mut().enumerate() {
            if Some(index) == held {
                body.speed = Vec2::ZERO;
                continue;
            }
            let force = forces[index] - body.at * 0.004;
            let speed = (body.speed + force) * 0.82;
            body.speed = if speed.length() > 24.0 {
                speed.normalized() * 24.0
            } else {
                speed
            };
            body.at += body.speed;
        }
    }
    state.nodes = nodes
        .iter()
        .zip(bodies)
        .map(|(node, body)| (node.key.clone(), body))
        .collect();
}

/// What the graph is built from, so a change wakes the layout.
fn shape_of(nodes: &[Node], links: &[(usize, usize)]) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for node in nodes {
        node.key.hash(&mut hasher);
        node.radius.to_bits().hash(&mut hasher);
    }
    links.hash(&mut hasher);
    hasher.finish()
}

/// The map, filling what is left of the page.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let (nodes, links) = graph(app);
    let mut state = std::mem::take(&mut app.playlist_map);
    let shape = shape_of(&nodes, &links);
    if shape != state.shape {
        state.shape = shape;
        state.steps_left = 400;
    }
    if state.zoom == 0.0 {
        state.zoom = 1.0;
    }
    // A node met for the first time takes its place beside its parent now,
    // so the first frame already shows it.
    if nodes
        .iter()
        .any(|node| !state.nodes.contains_key(&node.key))
    {
        settle(&mut state, &nodes, &links, None, 0);
    }

    theme::text(
        ui,
        gettext(
            locale,
            "Scroll to zoom, drag to move, double-click to centre. Click a playlist to open it.",
        ),
        theme::regular(12.5),
        palette.dim,
    );
    ui.add_space(6.0);
    // The page's view less what stands above the map, which scrolling
    // does not change.
    let above = ui.cursor().top() - ui.max_rect().top();
    let height = (ui.clip_rect().height() - above - 24.0).max(360.0);
    let (rect, background) =
        ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click_and_drag());
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::same(10), palette.panel);

    // Zoom about the pointer with the wheel or a pinch, here and not the page.
    if let Some(pointer) = ui
        .ctx()
        .pointer_hover_pos()
        .filter(|_| ui.rect_contains_pointer(rect))
    {
        let (scroll, pinch) = ui.input(|input| (input.smooth_scroll_delta.y, input.zoom_delta()));
        let factor = pinch * (scroll * 0.0025).exp();
        if (factor - 1.0).abs() > f32::EPSILON {
            let zoom = (state.zoom * factor).clamp(0.15, 4.0);
            let from_centre = pointer - rect.center();
            state.pan = from_centre - (from_centre - state.pan) * (zoom / state.zoom);
            state.zoom = zoom;
            ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
        }
    }
    if background.dragged() {
        state.pan += background.drag_delta();
    }
    if background.double_clicked() {
        state.pan = Vec2::ZERO;
        state.zoom = 1.0;
    }

    // Each node takes clicks and drags on top of the background.
    let held_id = egui::Id::new("playlist-map-held");
    let mut held: Option<String> = ui.data(|data| data.get_temp(held_id));
    // A node is held only while a button is down, wherever it was let go.
    if !ui.input(|input| input.pointer.any_down()) {
        held = None;
    }
    let zoom = state.zoom;
    let screen = |at: Vec2, pan: Vec2| rect.center() + pan + at * zoom;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let painter = child.painter().clone();

    for &(a, b) in &links {
        let (Some(from), Some(to)) = (
            state.nodes.get(&nodes[a].key),
            state.nodes.get(&nodes[b].key),
        ) else {
            continue;
        };
        painter.line_segment(
            [screen(from.at, state.pan), screen(to.at, state.pan)],
            Stroke::new(1.0, palette.outline),
        );
    }

    let playing = app.playing_context_uri();
    let art = app.backend.art().clone();
    let mut hovered_label: Option<(Pos2, String)> = None;
    // Names go on top of every node, so a neighbour never hides one.
    let mut names: Vec<(Pos2, String)> = Vec::new();
    for node in &nodes {
        let Some(body) = state.nodes.get(&node.key).copied() else {
            continue;
        };
        let centre = screen(body.at, state.pan);
        let radius = (node.radius * zoom).max(3.0);
        let area = Rect::from_center_size(centre, Vec2::splat(radius * 2.0));
        if !area.intersects(rect) {
            continue;
        }
        let response = child.interact(area, child.id().with(&node.key), Sense::click_and_drag());
        let songs = node.songs.map(|songs| app.locale.song_count(songs));
        let label = match &songs {
            Some(songs) => format!("{}, {songs}", node.name),
            None => node.name.clone(),
        };
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
        });
        match &node.kind {
            Kind::Library => {
                painter.circle_filled(centre, radius, palette.accent);
                let size = radius * 1.1;
                Icon::Library
                    .image(palette.on_accent, size)
                    .paint_at(&child, Rect::from_center_size(centre, Vec2::splat(size)));
            }
            Kind::Liked => {
                super::sidebar::liked_cover(&child, area, radius);
            }
            Kind::Folder(_) => {
                painter.circle_filled(centre, radius, palette.surface_active);
                painter.circle_stroke(centre, radius, Stroke::new(1.0, palette.secondary));
                let size = radius * 1.1;
                Icon::Library
                    .image(palette.secondary, size)
                    .paint_at(&child, Rect::from_center_size(centre, Vec2::splat(size)));
            }
            Kind::Playlist { .. } => {
                super::widgets::paint_cover(
                    &child,
                    &palette,
                    node.image.as_deref(),
                    area,
                    radius,
                    Icon::Music,
                    Some(&art),
                );
            }
        }
        let is_playing = match &node.kind {
            Kind::Playlist { uri, .. } => playing.as_deref() == Some(uri.as_str()),
            Kind::Liked => playing
                .as_deref()
                .is_some_and(|context| context.ends_with(":collection")),
            _ => false,
        };
        if is_playing || response.hovered() {
            painter.circle_stroke(
                centre,
                radius + 2.0,
                Stroke::new(
                    2.0,
                    if is_playing {
                        palette.accent
                    } else {
                        palette.text
                    },
                ),
            );
        }
        if response.hovered() {
            hovered_label = Some((centre + vec2(0.0, radius + 4.0), label.clone()));
        } else if radius >= 16.0 {
            names.push((
                centre + vec2(0.0, radius + 4.0),
                node.name.chars().take(24).collect(),
            ));
        }
        if response.drag_started() {
            held = Some(node.key.clone());
        }
        if response.dragged()
            && let Some(moved) = state.nodes.get_mut(&node.key)
        {
            moved.at += response.drag_delta() / zoom;
            state.steps_left = state.steps_left.max(120);
        }
        if response.drag_stopped() {
            held = None;
        }
        if response.clicked() {
            match &node.kind {
                Kind::Playlist { id, .. } => {
                    app.actions.push(Action::Open(Page::Playlist(id.clone())));
                }
                Kind::Liked => app.actions.push(Action::Open(Page::LikedSongs)),
                Kind::Folder(id) => {
                    app.actions
                        .push(Action::EnterLibraryFolder(Some(id.clone())));
                }
                Kind::Library => {}
            }
        }
    }
    for (at, name) in names {
        painter.text(
            at,
            egui::Align2::CENTER_TOP,
            name,
            theme::regular(11.5),
            palette.secondary,
        );
    }
    if let Some((at, label)) = hovered_label {
        let galley = painter.layout_no_wrap(label, theme::medium(12.5), palette.text);
        let back = Rect::from_center_size(
            at + vec2(0.0, galley.size().y / 2.0 + 3.0),
            galley.size() + vec2(12.0, 6.0),
        );
        painter.rect_filled(back, egui::CornerRadius::same(6), palette.overlay);
        painter.galley(back.min + vec2(6.0, 3.0), galley, Color32::PLACEHOLDER);
    }
    ui.data_mut(|data| match &held {
        Some(key) => {
            data.insert_temp(held_id, key.clone());
        }
        None => data.remove::<String>(held_id),
    });

    // The layout runs a few steps a frame while anything still moves.
    if state.steps_left > 0 || held.is_some() {
        // A large library moves a step a frame, a small one three.
        let most = if nodes.len() > 300 { 1 } else { 3 };
        let steps = state.steps_left.clamp(1, most);
        settle(&mut state, &nodes, &links, held.as_deref(), steps);
        state.steps_left = state.steps_left.saturating_sub(steps);
        ui.ctx().request_repaint();
    }
    app.playlist_map = state;
}

#[cfg(test)]
mod tests;
