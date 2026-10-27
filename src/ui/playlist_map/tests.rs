use super::*;

/// The demo library, offline, in folders of its own.
fn demo_app(name: &str) -> App {
    let root = std::env::temp_dir().join(format!("spotifast-map-{name}-{}", std::process::id()));
    let mut app = App::new(
        &crate::backend::Waker::default(),
        crate::paths::AppDirs {
            config: root.join("config"),
            state: root.join("state"),
            cache: root.join("cache"),
        },
        crate::settings::Settings::default(),
        crate::app::AppOptions {
            media_controls: false,
            restore_sign_in: false,
            tray: false,
            autostart: false,
        },
    );
    crate::demo::populate(&mut app);
    app
}

fn node(key: &str, radius: f32) -> Node {
    Node {
        key: key.into(),
        kind: Kind::Library,
        name: key.into(),
        radius,
        songs: None,
        image: None,
    }
}

/// Bigger playlists are bigger nodes, within bounds.
#[test]
fn playlists_grow_with_their_songs_within_bounds() {
    assert!(playlist_radius(0) < playlist_radius(10));
    assert!(playlist_radius(10) < playlist_radius(200));
    assert_eq!(playlist_radius(1_000_000), 46.0);
    assert!(playlist_radius(0) >= 9.0);
}

/// The graph follows the folder tree: folders hang from the library or
/// their folder, playlists from theirs, and playlists the tree does not
/// hold from the library; each appears once.
#[test]
fn the_graph_follows_the_folders() {
    use crate::player::RootlistEntry::{FolderEnd, FolderStart, Playlist};
    let mut app = demo_app("playlist-map-graph");
    let uris: Vec<String> = app
        .library
        .playlists
        .get()
        .unwrap()
        .iter()
        .map(|playlist| playlist.uri.clone())
        .collect();
    assert!(uris.len() >= 4);
    app.rootlist = vec![
        FolderStart {
            id: "outer".into(),
            name: "Outer".into(),
        },
        Playlist(uris[0].clone()),
        FolderStart {
            id: "inner".into(),
            name: "Inner".into(),
        },
        Playlist(uris[1].clone()),
        FolderEnd,
        FolderEnd,
        Playlist(uris[2].clone()),
        Playlist(uris[2].clone()),
    ];
    let (nodes, links) = graph(&app);
    let index = |key: &str| nodes.iter().position(|node| node.key == key).unwrap();
    let parent = |key: &str| {
        let child = index(key);
        links
            .iter()
            .find(|(_, held)| *held == child)
            .map(|(parent, _)| nodes[*parent].key.clone())
            .unwrap()
    };
    assert_eq!(nodes[0].kind, Kind::Library);
    assert_eq!(parent("liked"), "library");
    assert_eq!(parent("folder:outer"), "library");
    assert_eq!(parent(&uris[0]), "folder:outer");
    assert_eq!(parent("folder:inner"), "folder:outer");
    assert_eq!(parent(&uris[1]), "folder:inner");
    assert_eq!(parent(&uris[2]), "library");
    assert_eq!(parent(&uris[3]), "library", "a playlist out of the tree");
    for uri in &uris {
        assert_eq!(nodes.iter().filter(|node| &node.key == uri).count(), 1);
    }
    assert_eq!(links.len(), nodes.len() - 1, "a tree");
    app.backend.shutdown();
}

/// The layout spreads nodes that start on top of each other until none
/// overlaps, pulls linked ones together, and leaves a held node in place.
#[test]
fn the_layout_spreads_nodes_and_leaves_a_held_one() {
    let nodes = vec![
        node("a", 20.0),
        node("b", 20.0),
        node("c", 20.0),
        node("d", 20.0),
    ];
    let links = vec![(0, 1), (0, 2), (0, 3)];
    let mut state = MapState::default();
    for key in ["a", "b", "c", "d"] {
        state.nodes.insert(key.into(), Body::default());
    }
    settle(&mut state, &nodes, &links, Some("a"), 600);
    let at = |key: &str| state.nodes[key].at;
    assert_eq!(at("a"), Vec2::ZERO, "the held node stays put");
    for (x, y) in [("b", "c"), ("b", "d"), ("c", "d"), ("a", "b")] {
        let apart = (at(x) - at(y)).length();
        assert!(apart > 40.0, "{x} and {y} overlap at {apart}");
        assert!(apart < 600.0, "{x} and {y} drift apart at {apart}");
    }
}

/// A node met for the first time starts beside its parent, the same place
/// every time.
#[test]
fn new_nodes_start_beside_their_parent() {
    let nodes = vec![node("library", 18.0), node("x", 10.0)];
    let links = vec![(0, 1)];
    let mut first = MapState::default();
    first.nodes.insert(
        "library".into(),
        Body {
            at: vec2(500.0, 500.0),
            speed: Vec2::ZERO,
        },
    );
    let mut second = MapState {
        nodes: first.nodes.clone(),
        ..Default::default()
    };
    settle(&mut first, &nodes, &links, None, 0);
    settle(&mut second, &nodes, &links, None, 0);
    let start = first.nodes["x"].at;
    assert_eq!(start, second.nodes["x"].at);
    assert!((start - vec2(500.0, 500.0)).length() < 80.0);
}

/// A node let go anywhere is let go, even off the map, and the layout
/// comes to rest, after which it runs no more.
#[test]
fn the_map_lets_go_and_comes_to_rest() {
    let ctx = egui::Context::default();
    let mut app = demo_app("playlist-map-rest");
    let held_id = egui::Id::new("playlist-map-held");
    ctx.data_mut(|data| data.insert_temp(held_id, "spotify:playlist:pl1".to_string()));
    let frame = |app: &mut App| {
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| show(app, ui));
        output.textures_delta.clear();
    };
    frame(&mut app);
    assert_eq!(ctx.data(|data| data.get_temp::<String>(held_id)), None);
    for _ in 0..1000 {
        if app.playlist_map.steps_left == 0 {
            break;
        }
        frame(&mut app);
    }
    assert_eq!(app.playlist_map.steps_left, 0, "the layout rests");
    let at_rest: Vec<(String, Vec2)> = app
        .playlist_map
        .nodes
        .iter()
        .map(|(key, body)| (key.clone(), body.at))
        .collect();
    frame(&mut app);
    for (key, at) in at_rest {
        assert_eq!(app.playlist_map.nodes[&key].at, at, "{key} moved at rest");
    }
    app.backend.shutdown();
}
