use super::*;

/// A library of many folders, some inside others, and loose playlists of
/// every size, built as `graph` builds one.
fn big_library() -> Vec<Node> {
    let mut nodes = Vec::new();
    let add = |nodes: &mut Vec<Node>, parent: Option<usize>, holds: bool, songs: u32| {
        let index = nodes.len();
        nodes.push(Node {
            key: format!("n{index}"),
            kind: if holds {
                Kind::Folder(format!("f{index}"))
            } else {
                Kind::Item(Page::Playlist(format!("p{index}")))
            },
            name: format!("n{index}"),
            image: None,
            round: false,
            songs: Some(songs),
            takes_songs: false,
            parent,
            children: Vec::new(),
            depth: parent.map_or(0, |parent: usize| nodes[parent].depth + 1),
            focus: true,
            radius: if holds {
                12.0
            } else {
                4.5 + (songs as f32).sqrt() * 0.2
            },
        });
        if let Some(parent) = parent {
            nodes[parent].children.push(index);
        }
        index
    };
    let root = add(&mut nodes, None, true, 0);
    for folder in 0..6 {
        let outer = add(&mut nodes, Some(root), true, 0);
        for song in 0..30 {
            add(&mut nodes, Some(outer), false, song * 7);
        }
        if folder % 2 == 0 {
            let inner = add(&mut nodes, Some(outer), true, 0);
            for song in 0..12 {
                add(&mut nodes, Some(inner), false, song * 31);
            }
        }
    }
    for song in 0..40 {
        add(&mut nodes, Some(root), false, song * 3);
    }
    nodes
}

/// However large the library, no two nodes of the round tree overlap, and
/// in circles each folder holds what is in it while nothing else overlaps;
/// a node grown under the pointer keeps the gap to the others.
#[test]
fn large_libraries_lay_out_without_overlaps() {
    let nodes = big_library();
    let clear = |places: &[Place], x: usize, y: usize| {
        (places[x].at - places[y].at).length() + 0.5 >= places[x].radius + places[y].radius + GAP
    };
    let mut tree = radial(&nodes, 2.0);
    for x in 0..nodes.len() {
        for y in x + 1..nodes.len() {
            assert!(clear(&tree, x, y), "the tree overlaps {x} and {y}");
        }
    }
    make_room(&mut tree, &nodes, 20, 60.0, false);
    assert_eq!(tree[20].radius, 60.0);
    for x in 0..nodes.len() {
        for y in x + 1..nodes.len() {
            assert!(clear(&tree, x, y), "growing 20 overlaps {x} and {y}");
        }
    }

    let circles = nested(&nodes, 2.0);
    for (index, node) in nodes.iter().enumerate() {
        for &child in &node.children {
            let reach = (circles[child].at - circles[index].at).length() + circles[child].radius;
            assert!(
                reach <= circles[index].radius + 0.5,
                "{child} leaves {index}"
            );
        }
        for (position, &x) in node.children.iter().enumerate() {
            for &y in &node.children[position + 1..] {
                let apart = (circles[x].at - circles[y].at).length();
                assert!(
                    apart + 0.5 >= circles[x].radius + circles[y].radius,
                    "{x} and {y}"
                );
            }
        }
    }

    // In circles a hovered playlist's neighbours step aside inside their
    // folder, a folder beside it taking all it holds along.
    let leaf = nodes[1].children[nodes[1].children.len() - 2];
    let mut grown = circles.clone();
    make_room(&mut grown, &nodes, leaf, 40.0, true);
    for (index, node) in nodes.iter().enumerate() {
        for &child in &node.children {
            let held = grown[child].at - grown[index].at;
            let was = circles[child].at - circles[index].at;
            assert!(
                index == 1 || (held - was).length() < 0.01,
                "{child} left its place in {index}"
            );
        }
    }
    let inside = &nodes[1].children;
    for &child in inside {
        let reach = (grown[child].at - grown[1].at).length() + grown[child].radius;
        assert!(reach <= grown[1].radius + 0.5, "{child} left its folder");
    }
    for (position, &x) in inside.iter().enumerate() {
        for &y in &inside[position + 1..] {
            assert!(
                clear(&grown, x, y),
                "{x} and {y} overlap after growing {leaf}"
            );
        }
    }
}
