//! Home as a map of the Library, as the sidebar lists it: the kinds its
//! filters tick, in its order, each folder holding what it holds. Drawn as
//! a round tree, each level of folders a ring further from the root, or as
//! circles, each folder around everything it holds. An open folder is the
//! root. Nodes stay small until the pointer comes, when one grows and its
//! neighbours step aside; everything eases to where it belongs, grows in
//! as it comes and shrinks away as it goes.

use std::collections::{HashMap, HashSet};
use std::f32::consts::{PI, TAU};

use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2, vec2};

use crate::app::App;
use crate::i18n::gettext;
use crate::model::{Action, DragTrack, Page};
use crate::settings::{LibraryShelf, MapFocus, MapLayout};
use crate::theme::{self, Icon};

/// The space kept between neighbours, a grown node's too.
const GAP: f32 = 8.0;
/// The least radius a hovered node grows to on screen.
const GROWN: f32 = 26.0;
/// The band a folder's circle keeps at its top for its name.
const NAME_BAND: f32 = 16.0;

/// Where everything on the map is drawn and where the view looks, kept
/// between frames and visits to Home.
#[derive(Default)]
pub struct MapState {
    drawn: HashMap<String, Drawn>,
    camera: Camera,
    /// The layout last worked out, and what it was worked out from.
    cache: Option<(u64, Vec<Place>)>,
    hovered: Option<String>,
    /// What the view was last fitted to, to fit it again when that changes.
    fitted: Option<u64>,
    /// A double-click fits the view to everything, and another to the
    /// focus again.
    whole: bool,
}

#[cfg(test)]
impl MapState {
    /// Whether the map still draws `key`, here or shrinking away.
    pub(crate) fn draws(&self, key: &str) -> bool {
        self.drawn.contains_key(key)
    }
}

/// Where a node sits and how large it is, in the map's own units.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Place {
    pub(crate) at: Vec2,
    pub(crate) radius: f32,
    /// In the tree, where a folder's branch turns round its ring, as a
    /// share of its distance from the root.
    pub(crate) elbow: f32,
}

/// A node as drawn, with what it is now or, gone, was last.
type Shown<'a> = (&'a Drawn, &'a Node);

/// A node as drawn: easing toward its place, and fading in or out.
struct Drawn {
    at: Vec2,
    radius: f32,
    /// How present it is: 0 coming or gone, 1 here.
    shown: f32,
    /// Still in the Library; one that left shrinks into its parent.
    here: bool,
    elbow: f32,
    node: Node,
    parent: Option<String>,
}

/// The view: where the map's origin is from the middle of the map, and how
/// large a unit is. `auto` follows the fit until the view is moved by hand.
#[derive(Default)]
struct Camera {
    pan: Vec2,
    zoom: f32,
    auto: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    Library,
    Folder(String),
    /// A kind's own branch, while the map holds more than one.
    Group(LibraryShelf),
    /// Anything that opens a page: Liked Songs, Local Files, a playlist,
    /// album, artist or podcast.
    Item(Page),
}

#[derive(Clone, Debug)]
pub(crate) struct Node {
    pub(crate) key: String,
    pub(crate) kind: Kind,
    pub(crate) name: String,
    image: Option<String>,
    round: bool,
    songs: Option<u32>,
    takes_songs: bool,
    pub(crate) parent: Option<usize>,
    pub(crate) children: Vec<usize>,
    /// Steps from the root.
    pub(crate) depth: usize,
    /// Inside what the map is about: the open folder, and what the
    /// Library's filters list.
    pub(crate) focus: bool,
    /// Outside the open folder: the way back out, kept out of view.
    pub(crate) away: bool,
    pub(crate) radius: f32,
}

impl Node {
    fn holds(&self) -> bool {
        matches!(self.kind, Kind::Library | Kind::Folder(_) | Kind::Group(_))
    }
}

/// The map's nodes, the root first and every node before what it holds:
/// the Library as the sidebar lists it, the open folder at the root. With
/// the folder's Only, the rest of the Library is left out; otherwise, in
/// the tree, the way back out hangs from the folder, behind what it holds,
/// and in circles the folder stays inside the rest. With the filters'
/// Only, what they leave out is left out; otherwise it stays, out of
/// focus.
pub(crate) fn graph(app: &App) -> Vec<Node> {
    let home = app.settings.home;
    let rows = super::sidebar::map_rows(app);
    let mut raw: Vec<Node> = Vec::with_capacity(rows.len() + 1);
    raw.push(Node {
        key: "library".into(),
        kind: Kind::Library,
        name: gettext(app.locale, "Library").into_owned(),
        image: None,
        round: false,
        songs: None,
        takes_songs: false,
        parent: None,
        children: Vec::new(),
        depth: 0,
        focus: true,
        away: false,
        radius: 0.0,
    });
    let mut listed = vec![true];
    let mut keys: HashSet<String> = HashSet::from(["library".to_string()]);
    let mut open: Vec<(u8, usize)> = Vec::new();
    for row in rows {
        while open.last().is_some_and(|(depth, _)| *depth >= row.depth) {
            open.pop();
        }
        let parent = open.last().map_or(0, |(_, index)| *index);
        let index = raw.len();
        // Something met twice gets a key of its own each time.
        let key = if keys.contains(&row.key) {
            format!("{}#{index}", row.key)
        } else {
            row.key
        };
        keys.insert(key.clone());
        let kind = match (row.folder, row.group, row.page) {
            (Some(id), _, _) => Kind::Folder(id),
            (None, Some(shelf), _) => Kind::Group(shelf),
            (None, None, Some(page)) => Kind::Item(page),
            (None, None, None) => Kind::Folder(String::new()),
        };
        if matches!(kind, Kind::Folder(_) | Kind::Group(_)) {
            open.push((row.depth, index));
        }
        raw[parent].children.push(index);
        raw.push(Node {
            key,
            kind,
            name: row.name,
            image: row.image,
            round: row.round,
            songs: row.songs,
            takes_songs: row.takes_songs,
            parent: Some(parent),
            children: Vec::new(),
            depth: 0,
            focus: true,
            away: false,
            radius: 0.0,
        });
        listed.push(row.listed);
    }
    let count = raw.len();
    let parent_of = |index: usize| raw[index].parent.unwrap_or(0);
    // Every node comes after the one holding it, so one pass settles each.
    let mut keep = vec![true; count];
    if home.map_filters == MapFocus::Only {
        for index in 1..count {
            keep[index] = listed[index] && keep[parent_of(index)];
        }
    }
    let folder = app.library_folder.as_ref().and_then(|id| {
        (1..count).find(|index| keep[*index] && raw[*index].kind == Kind::Folder(id.clone()))
    });
    let mut inside = vec![folder.is_none(); count];
    if let Some(folder) = folder {
        for index in folder..count {
            inside[index] = index == folder || (index > folder && inside[parent_of(index)]);
        }
        if home.map_folder == MapFocus::Only {
            for index in 0..count {
                keep[index] &= inside[index];
            }
        }
    }
    let circles = home.map_layout == MapLayout::Circles;
    let root = if circles && home.map_folder == MapFocus::Zoom {
        0
    } else {
        folder.unwrap_or(0)
    };
    // From the root down: each node's own children, then the way back out.
    let mut nodes: Vec<Node> = Vec::with_capacity(count);
    let mut stack: Vec<(usize, Option<usize>, Option<usize>)> = vec![(root, None, None)];
    while let Some((index, from, parent)) = stack.pop() {
        let at = nodes.len();
        let mut node = raw[index].clone();
        node.children = Vec::new();
        node.parent = parent;
        node.depth = parent.map_or(0, |parent| nodes[parent].depth + 1);
        // The root and the open folder are in focus; the root is not when
        // the folder is open inside it, as in circles.
        node.focus = (index == root && inside[index])
            || Some(index) == folder
            || (listed[index] && inside[index]);
        node.away = !inside[index];
        nodes.push(node);
        if let Some(parent) = parent {
            nodes[parent].children.push(at);
        }
        let mut next: Vec<usize> = raw[index]
            .children
            .iter()
            .copied()
            .filter(|child| keep[*child] && Some(*child) != from)
            .collect();
        if let Some(up) = raw[index].parent
            && keep[up]
            && Some(up) != from
        {
            next.push(up);
        }
        for child in next.into_iter().rev() {
            stack.push((child, Some(index), Some(at)));
        }
    }
    let deepest = nodes
        .iter()
        .map(|node| node.depth)
        .max()
        .unwrap_or(0)
        .min(6);
    for node in &mut nodes {
        let height = deepest.saturating_sub(node.depth) as f32;
        node.radius = if node.depth == 0 {
            10.0 + 3.0 * height
        } else if node.holds() {
            7.0 + 3.0 * height
        } else {
            let songs = node.songs.map_or(0.0, |songs| (songs as f32).sqrt() * 0.2);
            4.5 + 1.6 * height + songs.min(3.0)
        };
    }
    nodes
}

/// The round tree: the root in the middle and each level of folders a ring
/// further out, each folder given a share of the circle by the room it and
/// the folders under it take, so nothing on a ring overlaps. What a node
/// holds that holds nothing itself, its playlists, albums and the like, is
/// packed close round it, what is in focus nearest, so a large library
/// stays compact. The way back out of an open folder starts on a ring of
/// its own, `outside` times as far as the folder reaches, past what a view
/// fitted to it shows.
pub(crate) fn radial(nodes: &[Node], outside: f32) -> Vec<Place> {
    let count = nodes.len();
    if count == 0 {
        return Vec::new();
    }
    let branch = |index: usize| index == 0 || !nodes[index].children.is_empty();
    let branches = |index: usize| {
        nodes[index]
            .children
            .iter()
            .copied()
            .filter(|child| branch(*child))
            .collect::<Vec<usize>>()
    };
    // Each branch with what it holds packed round it: how far that reaches.
    let mut offset = vec![Vec2::ZERO; count];
    let mut reach = vec![0.0f32; count];
    for index in (0..count).filter(|index| branch(*index)) {
        let mut leaves: Vec<usize> = nodes[index]
            .children
            .iter()
            .copied()
            .filter(|child| !branch(*child))
            .collect();
        leaves.sort_by_key(|leaf| !nodes[*leaf].focus);
        let mut radii = Vec::with_capacity(leaves.len() + 1);
        radii.push(nodes[index].radius + GAP / 2.0);
        radii.extend(leaves.iter().map(|leaf| nodes[*leaf].radius + GAP / 2.0));
        let (centres, _) = pack(&radii);
        let middle = centres[0];
        let mut extent = radii[0];
        for ((leaf, centre), radius) in leaves.iter().zip(&centres[1..]).zip(&radii[1..]) {
            offset[*leaf] = *centre - middle;
            extent = extent.max(offset[*leaf].length() + radius);
        }
        reach[index] = extent;
    }
    // Rings: a level of branches each, the way out past the rest.
    let mut level = vec![0usize; count];
    for index in 1..count {
        level[index] = level[nodes[index].parent.unwrap_or(0)] + 1;
    }
    let home = (0..count)
        .filter(|index| branch(*index) && !nodes[*index].away)
        .map(|index| level[index])
        .max()
        .unwrap_or(0);
    let mut ring = vec![0usize; count];
    for index in (1..count).filter(|index| branch(*index)) {
        let parent = nodes[index].parent.unwrap_or(0);
        ring[index] = if nodes[index].away && !nodes[parent].away {
            (ring[parent] + 1).max(home + 2)
        } else {
            ring[parent] + 1
        };
    }
    // A share of the circle by room; beside the open folder, the way out
    // takes a slim share at most.
    let mut room = vec![0.0f32; count];
    for index in (0..count).rev().filter(|index| branch(*index)) {
        let kids = branches(index);
        let near: f32 = kids
            .iter()
            .filter(|kid| !nodes[**kid].away)
            .map(|kid| room[*kid])
            .sum();
        if !nodes[index].away && near > 0.0 {
            for &kid in &kids {
                if nodes[kid].away {
                    room[kid] = room[kid].min(near * 0.3);
                }
            }
        }
        let under: f32 = kids.iter().map(|kid| room[*kid]).sum();
        room[index] = under.max(2.0 * reach[index] + GAP);
    }
    let mut start = vec![-PI / 2.0; count];
    let mut span = vec![TAU; count];
    for index in (0..count).filter(|index| branch(*index)) {
        let kids = branches(index);
        let under: f32 = kids.iter().map(|kid| room[*kid]).sum();
        let mut angle = start[index];
        for kid in kids {
            span[kid] = span[index] * room[kid] / under.max(f32::EPSILON);
            start[kid] = angle;
            angle += span[kid];
        }
    }
    let rings = ring.iter().max().copied().unwrap_or(0) + 1;
    let mut largest = vec![0.0f32; rings];
    let mut needed = vec![0.0f32; rings];
    for index in (1..count).filter(|index| branch(*index)) {
        let k = ring[index];
        largest[k] = largest[k].max(reach[index]);
        // Neighbours on a ring sit a chord apart: wide enough for this one.
        let half = (span[index].min(PI) / 2.0).sin().max(1e-4);
        needed[k] = needed[k].max((2.0 * reach[index] + GAP) / (2.0 * half));
    }
    largest[0] = reach[0];
    let mut distance = vec![0.0f32; rings];
    for k in 1..rings {
        let mut out = distance[k - 1] + largest[k - 1] + largest[k] + GAP * 2.0;
        out = out.max(needed[k]);
        if k == home + 2 && largest[k] > 0.0 {
            out = out.max((distance[home] + largest[home]) * outside + largest[k] + GAP * 6.0);
        }
        distance[k] = out;
    }
    let mut places = vec![Place::default(); count];
    for index in 0..count {
        let radius = nodes[index].radius;
        places[index] = if index == 0 {
            Place {
                at: Vec2::ZERO,
                radius,
                elbow: 0.0,
            }
        } else if branch(index) {
            let k = ring[index];
            let angle = start[index] + span[index] / 2.0;
            // The branch turns in the empty band between this ring and the
            // one inside it.
            let turn = (distance[k - 1] + largest[k - 1] + distance[k] - largest[k]) / 2.0;
            Place {
                at: vec2(angle.cos(), angle.sin()) * distance[k],
                radius,
                elbow: turn / distance[k].max(1e-3),
            }
        } else {
            let parent = nodes[index].parent.unwrap_or(0);
            Place {
                at: places[parent].at + offset[index],
                radius,
                elbow: 0.0,
            }
        };
    }
    places
}

/// Circles in circles: each folder around everything it holds, packed
/// tight in the sidebar's order, what is in focus first and so nearest
/// the middle. A folder's place is its whole circle. An open folder inside
/// what is out of focus keeps clear room around it, `outside` times its
/// size, so a view fitted to it shows nothing else.
pub(crate) fn nested(nodes: &[Node], outside: f32) -> Vec<Place> {
    let count = nodes.len();
    let mut offset = vec![Vec2::ZERO; count];
    let mut size = vec![0.0f32; count];
    for index in (0..count).rev() {
        let node = &nodes[index];
        if node.children.is_empty() {
            size[index] = node.radius;
            continue;
        }
        let mut order = node.children.clone();
        order.sort_by_key(|child| !nodes[*child].focus);
        let radii: Vec<f32> = order
            .iter()
            .map(|child| {
                let clear = if !nodes[*child].away && node.away {
                    outside
                } else {
                    1.0
                };
                size[*child] * clear + GAP / 2.0
            })
            .collect();
        let (centres, enclosing) = pack(&radii);
        // What it holds sits low enough to leave a band at the top for
        // the folder's name.
        for (child, centre) in order.iter().zip(centres) {
            offset[*child] = centre + vec2(0.0, NAME_BAND / 2.0);
        }
        size[index] = (enclosing + NAME_BAND / 2.0 + GAP).max(node.radius);
    }
    let mut places = vec![Place::default(); count];
    for index in 0..count {
        let at = nodes[index]
            .parent
            .map_or(Vec2::ZERO, |parent| places[parent].at + offset[index]);
        places[index] = Place {
            at,
            radius: size[index],
            elbow: 0.0,
        };
    }
    places
}

/// Packs circles of `radii` side by side, each touching two placed before
/// it as near the middle as it fits (d3's front-chain packing), and returns
/// their centres around the middle of a circle enclosing them all, and
/// that circle's radius.
pub(crate) fn pack(radii: &[f32]) -> (Vec<Vec2>, f32) {
    let count = radii.len();
    let mut at = vec![Vec2::ZERO; count];
    if count == 0 {
        return (at, 0.0);
    }
    if count > 1 {
        at[0] = vec2(-radii[1], 0.0);
        at[1] = vec2(radii[0], 0.0);
    }
    if count > 2 {
        at[2] = touching(at[1], radii[1], at[0], radii[0], radii[2]);
        let mut next = vec![0usize; count];
        let mut previous = vec![0usize; count];
        let (mut a, mut b) = (0usize, 1usize);
        next[0] = 1;
        previous[2] = 1;
        next[1] = 2;
        previous[0] = 2;
        next[2] = 0;
        previous[1] = 0;
        let overlaps = |at: &[Vec2], x: usize, y: usize| {
            let reach = radii[x] + radii[y] - 1e-3;
            reach > 0.0 && reach * reach > (at[y] - at[x]).length_sq()
        };
        let score = |at: &[Vec2], next: &[usize], x: usize| {
            let y = next[x];
            let sum = radii[x] + radii[y];
            ((at[x] * radii[y] + at[y] * radii[x]) / sum).length_sq()
        };
        let mut index = 3;
        // Rounding could in theory keep a circle retrying; past this many
        // tries it takes the place it has.
        let mut tries = 0usize;
        'place: while index < count {
            at[index] = touching(at[a], radii[a], at[b], radii[b], radii[index]);
            let (mut j, mut k) = (next[b], previous[a]);
            let (mut sj, mut sk) = (radii[b], radii[a]);
            tries += 1;
            if tries <= count + 16 {
                loop {
                    if sj <= sk {
                        if overlaps(&at, j, index) {
                            b = j;
                            next[a] = b;
                            previous[b] = a;
                            continue 'place;
                        }
                        sj += radii[j];
                        j = next[j];
                    } else {
                        if overlaps(&at, k, index) {
                            a = k;
                            next[a] = b;
                            previous[b] = a;
                            continue 'place;
                        }
                        sk += radii[k];
                        k = previous[k];
                    }
                    if j == next[k] {
                        break;
                    }
                }
            }
            tries = 0;
            previous[index] = a;
            next[index] = b;
            next[a] = index;
            previous[b] = index;
            b = index;
            let (mut best, mut best_score) = (a, score(&at, &next, a));
            let mut c = next[index];
            while c != b {
                let s = score(&at, &next, c);
                if s < best_score {
                    best = c;
                    best_score = s;
                }
                c = next[c];
            }
            a = best;
            b = next[a];
            index += 1;
        }
    }
    // A small circle around them all: step toward the farthest edge, less
    // each time (Bădoiu and Clarkson), then centre them in it.
    let mut centre = at[0];
    for step in 1..=96 {
        let far = (0..count)
            .max_by(|x, y| {
                let reach = |i: usize| (at[i] - centre).length() + radii[i];
                reach(*x).total_cmp(&reach(*y))
            })
            .unwrap_or(0);
        let outward = (at[far] - centre).normalized();
        let edge = at[far]
            + if outward.is_finite() {
                outward
            } else {
                Vec2::X
            } * radii[far];
        centre += (edge - centre) / (step as f32 + 1.0);
    }
    let enclosing = (0..count)
        .map(|i| (at[i] - centre).length() + radii[i])
        .fold(0.0f32, f32::max);
    for point in &mut at {
        *point -= centre;
    }
    (at, enclosing)
}

/// Where a circle of radius `rc` touches both the circle at `b` and the one
/// at `a`, on the left of the way from `a` to `b`.
fn touching(b: Vec2, rb: f32, a: Vec2, ra: f32, rc: f32) -> Vec2 {
    let d = b - a;
    let d2 = d.length_sq();
    if d2 <= 0.0 {
        return a + vec2(rc, 0.0);
    }
    let a2 = (ra + rc) * (ra + rc);
    let b2 = (rb + rc) * (rb + rc);
    if a2 > b2 {
        let x = (d2 + b2 - a2) / (2.0 * d2);
        let y = (b2 / d2 - x * x).max(0.0).sqrt();
        vec2(b.x - x * d.x - y * d.y, b.y - x * d.y + y * d.x)
    } else {
        let x = (d2 + a2 - b2) / (2.0 * d2);
        let y = (a2 / d2 - x * x).max(0.0).sqrt();
        vec2(a.x + x * d.x - y * d.y, a.y + x * d.y + y * d.x)
    }
}

/// Grows `hovered` to `grown` and moves its neighbours out of its way,
/// each keeping the usual gap to it and to the others. In circles only its
/// own folder's other contents move, and they stay inside it; there it
/// grows to no more than a third of its folder, so they still fit.
pub(crate) fn make_room(
    places: &mut [Place],
    nodes: &[Node],
    hovered: usize,
    grown: f32,
    circles: bool,
) {
    let parent = nodes[hovered].parent;
    let grown = match parent {
        Some(parent) if circles => grown
            .min(places[parent].radius * 0.34)
            .max(places[hovered].radius),
        _ => grown,
    };
    places[hovered].radius = grown;
    let centre = places[hovered].at;
    let reach = grown * 3.0 + 80.0;
    // In circles the whole folder takes part, since what steps aside is
    // kept inside it.
    let mut near: Vec<usize> = (0..places.len())
        .filter(|index| {
            if circles {
                *index == hovered || nodes[*index].parent == parent
            } else {
                (places[*index].at - centre).length() < reach + places[*index].radius
            }
        })
        .collect();
    // Zoomed far out a grown node is large on the map; the nearest few
    // hundred are as far as moving goes, to keep each frame quick.
    if near.len() > 300 {
        near.sort_by(|x, y| {
            (places[*x].at - centre)
                .length_sq()
                .total_cmp(&(places[*y].at - centre).length_sq())
        });
        near.truncate(300);
    }
    let movable = |index: usize| index != hovered && index != 0;
    let before: Vec<Vec2> = near.iter().map(|index| places[*index].at).collect();
    // Each pass parts what overlaps, then, in circles, brings back inside
    // the folder what was pushed out; it ends once nothing moves more than
    // a tenth of a unit.
    for _ in 0..48 {
        let mut largest = 0.0f32;
        for (position, &x) in near.iter().enumerate() {
            for &y in &near[position + 1..] {
                let apart = places[y].at - places[x].at;
                let distance = apart.length();
                let want = places[x].radius + places[y].radius + GAP;
                if distance >= want - 0.01 {
                    continue;
                }
                let direction = if distance > 1e-3 {
                    apart / distance
                } else {
                    let angle = angle_of(&nodes[y].key);
                    vec2(angle.cos(), angle.sin())
                };
                let push = want - distance;
                largest = largest.max(push);
                match (movable(x), movable(y)) {
                    (true, true) => {
                        places[x].at -= direction * push / 2.0;
                        places[y].at += direction * push / 2.0;
                    }
                    (false, true) => places[y].at += direction * push,
                    (true, false) => places[x].at -= direction * push,
                    (false, false) => continue,
                }
            }
        }
        if circles && let Some(parent) = parent {
            let room = places[parent];
            for &index in &near {
                if !movable(index) {
                    continue;
                }
                let off = places[index].at - room.at;
                let most = (room.radius - places[index].radius - GAP / 2.0).max(0.0);
                if off.length() > most + 0.01 {
                    largest = largest.max(off.length() - most);
                    places[index].at = room.at + off.normalized() * most;
                }
            }
        }
        if largest < 0.1 {
            break;
        }
    }
    // In circles a folder that stepped aside takes everything it holds
    // with it; they follow it in the order, deeper than it.
    if circles {
        for (&index, from) in near.iter().zip(before) {
            let shift = places[index].at - from;
            if nodes[index].children.is_empty() || shift == Vec2::ZERO {
                continue;
            }
            let depth = nodes[index].depth;
            for inner in index + 1..nodes.len() {
                if nodes[inner].depth <= depth {
                    break;
                }
                places[inner].at += shift;
            }
        }
    }
}

/// A stable angle for a key, to part two nodes on the same spot.
fn angle_of(key: &str) -> f32 {
    let hash = key.bytes().fold(2_166_136_261u32, |hash, byte| {
        (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
    });
    (hash % 6283) as f32 / 1000.0
}

/// What the layout is worked out from, so it is worked out again only when
/// that changes.
fn shape_of(nodes: &[Node], layout: MapLayout, outside: f32) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    layout.hash(&mut hasher);
    outside.to_bits().hash(&mut hasher);
    for node in nodes {
        node.key.hash(&mut hasher);
        node.parent.hash(&mut hasher);
        node.focus.hash(&mut hasher);
        node.radius.to_bits().hash(&mut hasher);
    }
    hasher.finish()
}

/// What the view is fitted to: the layout, the open folder and the
/// Library's choices. The Library itself changing, a page of albums
/// arriving or a playlist renamed, keeps a view moved by hand.
fn focus_of(app: &App) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let settings = &app.settings;
    settings.home.map_layout.hash(&mut hasher);
    format!(
        "{:?}{:?}",
        settings.home.map_folder, settings.home.map_filters
    )
    .hash(&mut hasher);
    app.library_folder.hash(&mut hasher);
    format!("{:?}", settings.library_kinds).hash(&mut hasher);
    settings.library_by_you.hash(&mut hasher);
    settings.library_group_kinds.hash(&mut hasher);
    app.library.filter.trim().hash(&mut hasher);
    hasher.finish()
}

/// The pan and zoom that show everything in focus in `size`, or with
/// `whole` everything there is, with room for names. In circles the
/// Library around it all is framed by what is in focus inside it, not by
/// its whole circle.
fn fit(nodes: &[Node], places: &[Place], size: Vec2, circles: bool, whole: bool) -> (Vec2, f32) {
    let mut bounds = Rect::NOTHING;
    for (index, (node, place)) in nodes.iter().zip(places).enumerate() {
        let framed_inside = circles
            && index == 0
            && node.kind == Kind::Library
            && node.children.iter().any(|child| nodes[*child].focus);
        if whole || (node.focus && !framed_inside) {
            bounds = bounds.union(Rect::from_center_size(
                place.at.to_pos2(),
                Vec2::splat(place.radius * 2.0),
            ));
        }
    }
    if !bounds.is_positive() {
        return if whole || nodes.is_empty() {
            (Vec2::ZERO, 1.0)
        } else {
            fit(nodes, places, size, circles, true)
        };
    }
    let room = (size - Vec2::splat(56.0)).max(Vec2::splat(40.0));
    // Circles keep their contents small, so a small folder takes more zoom.
    let most = if circles { 8.0 } else { 4.0 };
    let zoom = (room.x / bounds.width())
        .min(room.y / (bounds.height() + 16.0))
        .clamp(0.05, most);
    (-bounds.center().to_vec2() * zoom, zoom)
}

/// The choices above the map: its layout, and what to do with what lies
/// outside an open folder or the Library's filters.
fn toolbar(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let home = app.settings.home;
    let choice = |ui: &mut egui::Ui, label: &str, selected: bool| {
        let response = theme::soft_button(ui, &palette, None, label, selected);
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, label)
        });
        response.clicked() && !selected
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        // Translators: Home map layout: a round tree, each level of folders a ring further out.
        if choice(
            ui,
            &gettext(locale, "Tree"),
            home.map_layout == MapLayout::Tree,
        ) {
            app.actions.push(Action::SetMapLayout(MapLayout::Tree));
        }
        // Translators: Home map layout: each folder a circle around what it holds.
        if choice(
            ui,
            &gettext(locale, "Circles"),
            home.map_layout == MapLayout::Circles,
        ) {
            app.actions.push(Action::SetMapLayout(MapLayout::Circles));
        }
        ui.add_space(10.0);
        // Translators: Home map: what to do with the rest of the Library while a folder is open.
        theme::text(
            ui,
            gettext(locale, "Open folder:"),
            theme::regular(12.5),
            palette.dim,
        );
        // Translators: Home map: keep the rest further out and zoom in on the open folder, or on what the Library's filters list.
        let zoom = gettext(locale, "Zoom in");
        if choice(ui, &zoom, home.map_folder == MapFocus::Zoom) {
            app.actions.push(Action::SetMapFocus {
                folder: true,
                focus: MapFocus::Zoom,
            });
        }
        // Translators: Home map: show only what the open folder holds.
        if choice(
            ui,
            &gettext(locale, "Only its contents"),
            home.map_folder == MapFocus::Only,
        ) {
            app.actions.push(Action::SetMapFocus {
                folder: true,
                focus: MapFocus::Only,
            });
        }
        ui.add_space(10.0);
        // Translators: Home map: what to do with what the Library's filters (Playlists, Albums, By You, search) leave out.
        theme::text(
            ui,
            gettext(locale, "Filters:"),
            theme::regular(12.5),
            palette.dim,
        );
        if choice(ui, &zoom, home.map_filters == MapFocus::Zoom) {
            app.actions.push(Action::SetMapFocus {
                folder: false,
                focus: MapFocus::Zoom,
            });
        }
        // Translators: Home map: show only what the Library's filters list.
        if choice(
            ui,
            &gettext(locale, "Only what they list"),
            home.map_filters == MapFocus::Only,
        ) {
            app.actions.push(Action::SetMapFocus {
                folder: false,
                focus: MapFocus::Only,
            });
        }
    });
}

/// The map, filling what is left of the page.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    toolbar(app, ui);
    ui.add_space(4.0);
    theme::text(
        ui,
        gettext(
            locale,
            "Scroll to zoom, drag to move, double-click to switch between everything and the focus. Click to open; drop songs on a playlist to add them.",
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

    let layout = app.settings.home.map_layout;
    let circles = layout == MapLayout::Circles;
    let nodes = graph(app);
    let mut state = std::mem::take(&mut app.playlist_map);
    // Far enough out that a view fitted to the focus never shows it, in
    // the corners either: past the view's half-diagonal.
    let short = (rect.width().min(rect.height()) - 56.0).max(40.0);
    let outside = (rect.width().hypot(rect.height()) / short * 1.15 * 10.0).round() / 10.0;
    let shape = shape_of(&nodes, layout, outside);
    let reshaped = state.cache.as_ref().is_none_or(|(held, _)| *held != shape);
    if reshaped {
        let places = if circles {
            nested(&nodes, outside)
        } else {
            radial(&nodes, outside)
        };
        state.cache = Some((shape, places));
    }
    let base = state
        .cache
        .as_ref()
        .map(|(_, places)| places.clone())
        .unwrap_or_default();

    // The view fits the focus whenever it changes, and on a double-click.
    let focus = focus_of(app);
    let first = state.camera.zoom == 0.0;
    if state.fitted != Some(focus) {
        state.fitted = Some(focus);
        state.camera.auto = true;
        state.whole = false;
    }
    // A double-click switches between everything and the focus; after the
    // view was moved by hand, the first one fits it again as it was.
    if background.double_clicked() {
        if state.camera.auto {
            state.whole = !state.whole;
        }
        state.camera.auto = true;
    }
    let dt = ui.input(|input| input.stable_dt).clamp(1.0 / 240.0, 0.1);
    let ease = 1.0 - (-dt * 11.0).exp();
    let mut moving = false;
    if state.camera.auto {
        let (pan, zoom) = fit(&nodes, &base, rect.size(), circles, state.whole);
        if first {
            state.camera.pan = pan;
            state.camera.zoom = zoom;
        } else {
            let log = state.camera.zoom.ln() + (zoom.ln() - state.camera.zoom.ln()) * ease;
            state.camera.zoom = log.exp();
            state.camera.pan += (pan - state.camera.pan) * ease;
            moving |= (pan - state.camera.pan).length() > 0.5
                || (zoom / state.camera.zoom - 1.0).abs() > 0.002;
        }
    }

    // Zoom about the pointer with the wheel or a pinch, here and not the page.
    if let Some(pointer) = ui
        .ctx()
        .pointer_hover_pos()
        .filter(|_| ui.rect_contains_pointer(rect))
    {
        let (scroll, pinch) = ui.input(|input| (input.smooth_scroll_delta.y, input.zoom_delta()));
        let factor = pinch * (scroll * 0.0025).exp();
        if (factor - 1.0).abs() > f32::EPSILON {
            let zoom = (state.camera.zoom * factor).clamp(0.03, 12.0);
            let from_centre = pointer - rect.center();
            state.camera.pan =
                from_centre - (from_centre - state.camera.pan) * (zoom / state.camera.zoom);
            state.camera.zoom = zoom;
            state.camera.auto = false;
            ui.input_mut(|input| input.smooth_scroll_delta = Vec2::ZERO);
        }
    }
    if background.dragged() {
        state.camera.pan += background.drag_delta();
        state.camera.auto = false;
    }
    let zoom = state.camera.zoom;
    let pan = state.camera.pan;

    // Where each node is headed: its place, or, beside the one the pointer
    // is on, out of its way.
    let mut goal = base.clone();
    let hovered = state
        .hovered
        .as_ref()
        .and_then(|key| nodes.iter().position(|node| &node.key == key));
    if let Some(hovered) = hovered
        && !(circles && !nodes[hovered].children.is_empty())
    {
        let grown = (base[hovered].radius * 1.6).max(GROWN / zoom);
        make_room(&mut goal, &nodes, hovered, grown, circles);
    }

    // Ease everything toward its goal; what is new grows out of its
    // parent, and what is gone shrinks back into it.
    let mut present: HashSet<&str> = HashSet::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        present.insert(node.key.as_str());
        let parent = node.parent.map(|parent| nodes[parent].key.clone());
        let from = parent
            .as_ref()
            .and_then(|key| state.drawn.get(key))
            .map_or(goal[index].at, |drawn| drawn.at);
        let fresh = !state.drawn.contains_key(&node.key);
        let drawn = state
            .drawn
            .entry(node.key.clone())
            .or_insert_with(|| Drawn {
                at: from,
                radius: 0.0,
                shown: 0.0,
                here: true,
                elbow: 0.0,
                node: node.clone(),
                parent: parent.clone(),
            });
        let target = goal[index];
        drawn.elbow = target.elbow;
        drawn.at += (target.at - drawn.at) * ease;
        drawn.radius += (target.radius - drawn.radius) * ease;
        drawn.shown += (1.0 - drawn.shown) * ease;
        moving |= (target.at - drawn.at).length() * zoom > 0.3
            || (target.radius - drawn.radius).abs() * zoom > 0.3
            || drawn.shown < 0.99;
        drawn.here = true;
        if reshaped && !fresh {
            drawn.node = node.clone();
            drawn.parent = parent;
        }
    }
    let gone: Vec<String> = state
        .drawn
        .keys()
        .filter(|key| !present.contains(key.as_str()))
        .cloned()
        .collect();
    for key in gone {
        let into = state
            .drawn
            .get(&key)
            .and_then(|drawn| drawn.parent.clone())
            .and_then(|parent| state.drawn.get(&parent))
            .map(|parent| parent.at);
        let Some(drawn) = state.drawn.get_mut(&key) else {
            continue;
        };
        drawn.here = false;
        if let Some(into) = into {
            drawn.at += (into - drawn.at) * ease;
        }
        drawn.radius -= drawn.radius * ease;
        drawn.shown -= drawn.shown * ease;
        if drawn.shown < 0.02 {
            state.drawn.remove(&key);
        } else {
            moving = true;
        }
    }

    let screen = |at: Vec2| rect.center() + pan + at * zoom;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let deepest = nodes
        .iter()
        .map(|node| node.depth)
        .max()
        .unwrap_or(0)
        .min(6);
    // What is here is drawn as the Library has it now, what has left as it
    // was last.
    let order: Vec<(&Drawn, &Node)> = {
        let mut order: Vec<(&Drawn, &Node)> = nodes
            .iter()
            .filter_map(|node| Some((state.drawn.get(&node.key)?, node)))
            .collect();
        order.extend(
            state
                .drawn
                .values()
                .filter(|drawn| !drawn.here)
                .map(|drawn| (drawn, &drawn.node)),
        );
        order
    };
    let root_at = order.first().map_or(Vec2::ZERO, |(drawn, _)| drawn.at);

    // The tree's branches: out from the parent, round its ring, and out to
    // the child, thicker the nearer the root.
    if !circles {
        for (drawn, node) in &order {
            if node.children.is_empty() {
                continue;
            }
            let Some(parent) = drawn.parent.as_ref().and_then(|key| state.drawn.get(key)) else {
                continue;
            };
            let presence = drawn.shown.min(parent.shown);
            let height = deepest.saturating_sub(node.depth.saturating_sub(1)) as f32;
            let width = (0.8 + 0.9 * height) * zoom.sqrt().clamp(0.5, 1.4);
            let colour = if node.focus {
                palette.secondary.gamma_multiply(0.55)
            } else {
                palette.outline
            }
            .gamma_multiply(presence);
            let turn = (drawn.at - root_at).length() * drawn.elbow;
            let points = branch(parent.at - root_at, drawn.at - root_at, turn, zoom)
                .into_iter()
                .map(|at| screen(at + root_at))
                .collect();
            child
                .painter()
                .add(egui::Shape::line(points, Stroke::new(width, colour)));
        }
    }

    let playing = app.playing_context_uri();
    let art = app.backend.art().clone();
    let dragging_song = egui::DragAndDrop::has_payload_of_type::<DragTrack>(ui.ctx());
    let open_folder = app.library_folder.clone();
    let mut hovered_now: Option<String> = None;
    let mut hovered_label: Option<(Pos2, String)> = None;
    // Names, the more important first: folders, then what is in focus,
    // the larger before the smaller.
    let mut names: Vec<(u8, f32, Pos2, egui::Align2, String, f32)> = Vec::new();
    // Folders' circles first, so what they hold is drawn, and hovered, on
    // top of them.
    let (containers, items): (Vec<Shown>, Vec<Shown>) = order
        .iter()
        .partition(|(_, node)| circles && !node.children.is_empty());
    for (drawn, node) in containers.into_iter().chain(items) {
        let centre = screen(drawn.at);
        let container = circles && !node.children.is_empty();
        let radius = (drawn.radius * zoom).max(if container { 4.0 } else { 2.5 });
        let area = Rect::from_center_size(centre, Vec2::splat(radius * 2.0));
        if !area.intersects(rect) || drawn.shown < 0.02 {
            continue;
        }
        let presence = drawn.shown * if node.focus { 1.0 } else { 0.35 };
        child.set_opacity(presence);
        let painter = child.painter().clone();
        let label = match node.songs {
            Some(songs) => format!("{}, {}", node.name, locale.song_count(songs)),
            None => node.name.clone(),
        };
        // A folder's circle answers only on its name, leaving the space
        // inside to what it holds and to the map; anything else only on
        // its own circle, not the square around it.
        let target = if container && radius > 28.0 {
            Rect::from_center_size(
                centre - vec2(0.0, radius - 12.0),
                vec2((radius * 1.2).min(180.0), 22.0),
            )
        } else {
            area
        };
        let on_it = |at: Pos2| container || at.distance(centre) <= radius + 2.0;
        let response = drawn.here.then(|| {
            let response = child.interact(target, child.id().with(&node.key), Sense::click());
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label.clone())
            });
            response
        });
        let drop_here = dragging_song
            && node.takes_songs
            && response
                .as_ref()
                .is_some_and(egui::Response::contains_pointer)
            && ui.ctx().pointer_latest_pos().is_some_and(on_it);
        let pointed = response
            .as_ref()
            .is_some_and(|response| response.hover_pos().is_some_and(on_it))
            || drop_here;
        if pointed && !container {
            hovered_now = Some(node.key.clone());
        }
        if container {
            let height = deepest.saturating_sub(node.depth) as f32;
            painter.circle_filled(centre, radius, palette.surface.gamma_multiply(0.55));
            painter.circle_stroke(
                centre,
                radius,
                Stroke::new(
                    (0.8 + 0.9 * height) * zoom.sqrt().clamp(0.5, 1.4),
                    if pointed {
                        palette.text
                    } else {
                        palette.outline
                    },
                ),
            );
            // A folder's name in the band at its top, or, where it is too
            // small for that, just below it.
            if radius > 28.0 {
                names.push((
                    0,
                    radius,
                    centre - vec2(0.0, radius - 3.0),
                    egui::Align2::CENTER_TOP,
                    node.name.chars().take(28).collect(),
                    presence,
                ));
            } else if radius > 8.0 {
                names.push((
                    0,
                    radius,
                    centre + vec2(0.0, radius + 3.0),
                    egui::Align2::CENTER_TOP,
                    node.name.chars().take(28).collect(),
                    presence,
                ));
            }
        } else {
            match &node.kind {
                Kind::Library => {
                    painter.circle_filled(centre, radius, palette.accent);
                    let size = radius * 1.1;
                    Icon::Library
                        .image(palette.on_accent, size)
                        .paint_at(&child, Rect::from_center_size(centre, Vec2::splat(size)));
                }
                Kind::Folder(_) | Kind::Group(_) => {
                    painter.circle_filled(centre, radius, palette.surface_active);
                    painter.circle_stroke(centre, radius, Stroke::new(1.0, palette.secondary));
                    let icon = match &node.kind {
                        Kind::Group(LibraryShelf::Albums) => Icon::Disc,
                        Kind::Group(LibraryShelf::Artists) => Icon::Mic,
                        Kind::Group(LibraryShelf::Podcasts) => Icon::Radio,
                        Kind::Group(LibraryShelf::Playlists) => Icon::ListMusic,
                        _ => Icon::Library,
                    };
                    let size = radius * 1.1;
                    icon.image(palette.secondary, size)
                        .paint_at(&child, Rect::from_center_size(centre, Vec2::splat(size)));
                }
                Kind::Item(Page::LikedSongs) => super::sidebar::liked_cover(&child, area, radius),
                Kind::Item(Page::LocalFiles) => {
                    super::sidebar::local_files_cover(&child, &palette, area, radius);
                }
                Kind::Item(_) => super::widgets::paint_cover(
                    &child,
                    &palette,
                    node.image.as_deref(),
                    area,
                    radius,
                    if node.round { Icon::User } else { Icon::Music },
                    Some(&art),
                ),
            }
            let is_playing = match &node.kind {
                Kind::Item(Page::LikedSongs) => playing
                    .as_deref()
                    .is_some_and(|context| context.ends_with(":collection")),
                Kind::Item(_) => playing.as_deref() == Some(node.key.as_str()),
                _ => false,
            };
            if is_playing || pointed {
                painter.circle_stroke(
                    centre,
                    radius + 2.0,
                    Stroke::new(
                        2.0,
                        if is_playing || drop_here {
                            palette.accent
                        } else {
                            palette.text
                        },
                    ),
                );
            }
            if pointed {
                hovered_label = Some((centre + vec2(0.0, radius + 4.0), label.clone()));
            } else if (radius >= 9.0 && node.focus) || (node.holds() && radius >= 2.5) {
                let from = drawn
                    .parent
                    .as_ref()
                    .and_then(|key| state.drawn.get(key))
                    .map_or(root_at, |parent| parent.at);
                let outward = (drawn.at - from).normalized();
                let (at, align) = if circles || node.holds() || !outward.is_finite() {
                    (centre + vec2(0.0, radius + 4.0), egui::Align2::CENTER_TOP)
                } else if outward.x >= 0.0 {
                    (centre + outward * (radius + 5.0), egui::Align2::LEFT_CENTER)
                } else {
                    (
                        centre + outward * (radius + 5.0),
                        egui::Align2::RIGHT_CENTER,
                    )
                };
                let rank = if node.holds() { 0 } else { 1 };
                names.push((
                    rank,
                    radius,
                    at,
                    align,
                    node.name.chars().take(24).collect(),
                    presence,
                ));
            }
        }
        let Some(response) = response else {
            continue;
        };
        if node.takes_songs
            && let Some(track) = response.dnd_release_payload::<DragTrack>()
        {
            match &node.kind {
                Kind::Item(Page::LikedSongs) => app.actions.push(Action::SetSavedMany {
                    uris: track
                        .items
                        .iter()
                        .map(|item| item.uri().to_string())
                        .collect(),
                    saved: true,
                }),
                Kind::Item(Page::Playlist(id)) => app.actions.push(Action::AddToPlaylist {
                    playlist_id: id.clone(),
                    playlist_name: node.name.clone(),
                    items: track.items.clone(),
                }),
                _ => {}
            }
        }
        if response.clicked() && response.interact_pointer_pos().is_none_or(on_it) {
            match &node.kind {
                Kind::Item(page) => app.actions.push(Action::Open(page.clone())),
                // The open folder, the root, leads back out of it.
                Kind::Folder(id) if open_folder.as_ref() == Some(id) => {
                    app.actions
                        .push(Action::EnterLibraryFolder(crate::folders::parent(
                            &app.rootlist,
                            &crate::folders::Row::Folder(id.clone()),
                        )));
                }
                Kind::Folder(id) if !id.is_empty() => {
                    app.actions
                        .push(Action::EnterLibraryFolder(Some(id.clone())));
                }
                Kind::Library if open_folder.is_some() => {
                    app.actions.push(Action::EnterLibraryFolder(None));
                }
                _ => {}
            }
        }
    }
    child.set_opacity(1.0);
    let painter = child.painter().clone();
    // A name that would cover one already placed is left out; the
    // pointer still shows it.
    names.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)));
    let mut taken: Vec<Rect> = Vec::new();
    for (rank, _, at, align, name, presence) in names {
        let galley = painter.layout_no_wrap(
            name,
            theme::regular(11.5),
            palette.secondary.gamma_multiply(presence),
        );
        let place = align.anchor_size(at, galley.size());
        if taken
            .iter()
            .any(|held| held.expand2(vec2(5.0, 2.0)).intersects(place))
        {
            continue;
        }
        taken.push(place);
        // A folder's name sits on a backing, readable over what is
        // gathered round it.
        if rank == 0 {
            painter.rect_filled(
                place.expand2(vec2(4.0, 1.0)),
                egui::CornerRadius::same(5),
                palette.overlay.gamma_multiply(0.6 * presence),
            );
        }
        painter.galley(place.min, galley, Color32::PLACEHOLDER);
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
    if hovered_now != state.hovered {
        state.hovered = hovered_now;
        moving = true;
    }
    if moving {
        ui.ctx().request_repaint();
    }
    app.playlist_map = state;
}

/// A branch of the round tree from a parent at `from` to a child at `to`,
/// both from the root: out from the parent to the `turn` distance, round
/// that ring to the child's angle, and out to the child. From the root it
/// is a straight line.
fn branch(from: Vec2, to: Vec2, turn: f32, zoom: f32) -> Vec<Vec2> {
    let (inner, outer) = (from.length(), to.length());
    if inner < 1.0 || outer < 1.0 {
        return vec![from, to];
    }
    let (start, end) = (from.y.atan2(from.x), to.y.atan2(to.x));
    let mut sweep = end - start;
    while sweep > PI {
        sweep -= TAU;
    }
    while sweep < -PI {
        sweep += TAU;
    }
    // While things move a parent can be further out than its child.
    let elbow = turn.clamp(inner.min(outer), inner.max(outer));
    let steps = ((sweep.abs() * elbow * zoom / 6.0).ceil() as usize).clamp(1, 48);
    let mut points = Vec::with_capacity(steps + 3);
    points.push(from);
    for step in 0..=steps {
        let angle = start + sweep * step as f32 / steps as f32;
        points.push(vec2(angle.cos(), angle.sin()) * elbow);
    }
    points.push(to);
    points
}

#[cfg(test)]
mod tests;
