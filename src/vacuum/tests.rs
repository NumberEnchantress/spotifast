use super::collections::{
    Evidence, Verdict, classify, dominant_share, names_in_description, title_names,
};
use super::derive::{Picture, hex, normalize_name};
use super::museums::{self, Museum};
use super::*;

/// The extension's own outputs for two test seeds, made by its
/// `port-reference/make-golden-vectors.js`. Every value must match.
const GOLDEN: &str = include_str!("golden-vectors.json");

fn golden() -> serde_json::Value {
    serde_json::from_str(GOLDEN).expect("the golden vectors")
}

fn picture_json(picture: &Picture) -> serde_json::Value {
    match picture {
        Picture::Cover(squares) => serde_json::json!({
            "kind": "cover",
            "squares": squares.iter().map(|square| serde_json::json!({
                "background": square.background,
                "color": square.color,
                "shape": square.shape.name(),
                "turn": square.turn,
            })).collect::<Vec<_>>(),
        }),
        Picture::Rings { rings, profile } => serde_json::json!({
            "kind": "rings",
            "rings": rings,
            "profile": profile,
        }),
    }
}

#[test]
fn every_golden_vector_matches_the_extension() {
    let golden = golden();
    assert_eq!(golden["wordOrder"], "any");
    let seeds = golden["seeds"].as_array().expect("seeds");
    assert!(!seeds.is_empty());
    let mut checked = 0;
    for case in seeds {
        let text = case["seedText"].as_str().unwrap();
        let seed = Seed::custom(text).expect("a valid test seed");
        assert_eq!(seed::key_hex(&seed), case["keyHex"], "key for {text:?}");
        assert_eq!(seed.id, case["seedId"], "seed id for {text:?}");
        let deriver = deriver(&seed);
        for name in case["names"].as_array().unwrap() {
            let real = name["name"].as_str().unwrap();
            let kind = Kind::from_label(name["type"].as_str().unwrap()).expect("a kind");
            assert_eq!(normalize_name(real), name["normalized"], "{real:?}");
            let stand_in = deriver.derive(kind, real).expect("a stand-in");
            assert_eq!(hex(&stand_in.digest), name["digestHex"], "{real:?}");
            assert_eq!(stand_in.name, name["standIn"], "{real:?}");
            assert_eq!(picture_json(&stand_in.picture), name["picture"], "{real:?}");
            checked += 1;
        }
        for chain in case["pictureChains"].as_array().unwrap() {
            let (kind, id) = (
                chain["type"].as_str().unwrap(),
                chain["id"].as_str().unwrap(),
            );
            for link in chain["chain"].as_array().unwrap() {
                let n = link["n"].as_u64().unwrap() as u32;
                assert_eq!(museums::label(kind, id, n), link["label"]);
                assert_eq!(
                    hex(&deriver.mac(&museums::label(kind, id, n))),
                    link["macHex"]
                );
                let candidate = museums::candidate(&deriver, kind, id, n);
                let museum = match candidate.museum {
                    Museum::Met => "met",
                    Museum::Aic => "aic",
                    Museum::Cma => "cma",
                };
                assert_eq!(museum, link["museum"], "{kind}:{id}#{n}");
                assert_eq!(
                    candidate.object,
                    link["objectId"].as_u64().unwrap(),
                    "{kind}:{id}#{n}"
                );
                assert_eq!(
                    serde_json::json!(candidate.frame),
                    link["frame"],
                    "{kind}:{id}#{n}"
                );
                checked += 1;
            }
        }
        for pick in case["customListPick"].as_array().unwrap() {
            let (kind, id) = (pick["type"].as_str().unwrap(), pick["id"].as_str().unwrap());
            let length = pick["listLength"].as_u64().unwrap() as usize;
            let (index, _) = museums::custom_pick(&deriver, kind, id, length).unwrap();
            assert_eq!(index as u64, pick["index"].as_u64().unwrap(), "{kind}:{id}");
            checked += 1;
        }
    }
    assert_eq!(checked, 50, "every value in the vectors is checked");
}

/// The shared seed unscrambles to the extension's: its id, a hash of the
/// key that reveals nothing, matches the one the extension names its
/// caches by.
#[test]
fn the_shared_seed_is_the_extensions() {
    let seed = Seed::shared();
    assert!(!seed.custom);
    assert_eq!(seed.id, "c7b6e046e049c653");
    assert!(
        !format!("{seed:?}").contains(&seed::key_hex(&seed)),
        "never printed"
    );
}

#[test]
fn names_normalize_as_javascript_does() {
    // Runs of JavaScript whitespace, U+FEFF among them, are one space.
    assert_eq!(
        normalize_name("  Blade\u{FEFF}\u{3000} Runner\tBlues \n"),
        "blade runner blues"
    );
    // U+0085 is not JavaScript whitespace.
    assert_eq!(normalize_name("a\u{85}b"), "a\u{85}b");
    // NFC: a decomposed Å is the composed one.
    assert_eq!(normalize_name("A\u{30A}lesund"), "\u{E5}lesund");
    // Lower case, final sigma included.
    assert_eq!(normalize_name("ΟΔΟΣ"), "οδος");
    assert_eq!(normalize_name("ΟΔΟΣ ΣΑΣ"), "οδος σας");
}

#[test]
fn a_name_is_its_stand_in_however_it_is_written() {
    let deriver = deriver(&Seed::custom("test seed one").unwrap());
    let one = deriver.derive(Kind::Artist, "Daft Punk").unwrap();
    let two = deriver.derive(Kind::Artist, "  daft   PUNK ").unwrap();
    assert_eq!(one, two);
    assert_eq!(one.name.split(' ').count(), 3, "artists get three words");
    let album = deriver.derive(Kind::Album, "Daft Punk").unwrap();
    assert_ne!(album.name, one.name, "kinds are kept apart");
    assert_eq!(album.name.split(' ').count(), 2);
    assert!(
        deriver.derive(Kind::Album, " \t ").is_none(),
        "nothing to derive"
    );
    assert!(
        deriver.derive(Kind::Album, &"x".repeat(501)).is_none(),
        "too long"
    );
    assert!(deriver.derive(Kind::Album, &"x".repeat(500)).is_some());
}

#[test]
fn seeds_of_your_own_are_checked_and_hex_keys_kept() {
    assert!(Seed::custom("short").is_none());
    assert!(Seed::custom(&"y".repeat(501)).is_none());
    assert!(Seed::custom("long enough").is_some());
    let hex_key = "AB".repeat(32);
    let seed = Seed::custom(&format!("  {hex_key} ")).unwrap();
    assert_eq!(
        seed::key_hex(&seed),
        "ab".repeat(32),
        "64 hex digits are the key"
    );
    assert!(seed.custom);
    assert_eq!(Seed::from_setting(Some("tiny")).id, Seed::shared().id);
}

#[test]
fn pictures_draw_as_svg() {
    let deriver = deriver(&Seed::custom("test seed one").unwrap());
    let cover = deriver.derive(Kind::Album, "Blade Runner Blues").unwrap();
    let svg = pictures::svg(&cover.picture, 64);
    assert!(svg.starts_with("<svg") && svg.contains("translate(50 50)"));
    let rings = deriver.derive(Kind::Artist, "Daft Punk").unwrap();
    let svg = pictures::svg(&rings.picture, 64);
    assert_eq!(
        svg.matches("<circle").count(),
        9,
        "eight rings and an outline"
    );
    assert!(svg.contains(r#"r="50.00""#));
}

#[test]
fn museum_answers_are_read_as_the_extension_reads_them() {
    let met = serde_json::json!({"isPublicDomain": true, "primaryImageSmall": "https://images.metmuseum.org/x.jpg"});
    assert_eq!(
        museums::met_image(&met).as_deref(),
        Some("https://images.metmuseum.org/x.jpg")
    );
    assert_eq!(
        museums::met_image(&serde_json::json!({"isPublicDomain": false, "primaryImageSmall": "x"})),
        None
    );
    assert_eq!(
        museums::met_image(&serde_json::json!({"isPublicDomain": true, "primaryImageSmall": ""})),
        None
    );
    let cma = serde_json::json!({"data": {"share_license_status": "CC0", "images": {"web": {"url": "https://openaccess-cdn.clevelandart.org/a.jpg"}}}});
    assert!(museums::cma_image(&cma).is_some());
    assert!(
        museums::cma_image(&serde_json::json!({"data": {"share_license_status": "Copyrighted"}}))
            .is_none()
    );
    let aic = serde_json::json!({"id": 5, "is_public_domain": true, "image_id": "abc"});
    assert_eq!(
        museums::aic_image(&aic).as_deref(),
        Some("https://www.artic.edu/iiif/2/abc/full/400,/0/default.jpg")
    );
    assert!(museums::is_museum_image(
        "https://images.metmuseum.org/a.jpg"
    ));
    assert!(!museums::is_museum_image(
        "https://example.com/images.metmuseum.org/a.jpg"
    ));
    assert_eq!(
        museums::aic_batch_url(&[1, 2]),
        "https://api.artic.edu/api/v1/artworks?ids=1,2&fields=id,is_public_domain,image_id&limit=100"
    );
}

fn verdict(evidence: Evidence) -> Verdict {
    classify(&evidence).0
}

fn titled(title: &str) -> Evidence<'_> {
    Evidence {
        title,
        ..Default::default()
    }
}

/// The extension's `test/collections.test.js`, case for case.
#[test]
fn playlists_are_judged_as_the_extension_judges_them() {
    for title in [
        "rainy day",
        "internet crush",
        "Chill Mix",
        "Chill Essentials",
        "Peaceful Piano",
        "Daily Mix 1",
        "Pop Rising",
    ] {
        let evidence = Evidence {
            title,
            description: "Khruangbin, Men I Trust and more",
            ..Default::default()
        };
        assert_eq!(verdict(evidence), Verdict::Theme, "{title}");
    }
    for title in [
        "This Is Justice",
        "Morgan Wallen Radio",
        "Best of Daft Punk",
        "Justice's Picks",
    ] {
        assert_eq!(verdict(titled(title)), Verdict::Person, "{title}");
    }
    for title in [
        "Top 50 - Global",
        "Viral 50 - USA",
        "Today\u{2019}s Top Hits",
        "Hot Hits USA",
        "Hot 100",
        "Featured Charts",
    ] {
        assert_eq!(verdict(titled(title)), Verdict::Popularity, "{title}");
    }
    assert_eq!(
        verdict(Evidence {
            title: "Tame Impala Mix",
            description: "Tame Impala, MGMT and more",
            ..Default::default()
        }),
        Verdict::Person
    );
    assert_eq!(
        verdict(Evidence {
            title: "UNDERTALE | Daft Punk",
            names: vec!["Daft Punk", "Toby Fox"],
            ..Default::default()
        }),
        Verdict::Person
    );
    assert_eq!(
        verdict(Evidence {
            title: "songs like grand theft auto vi",
            session_names: vec!["Grand Theft Auto VI"],
            ..Default::default()
        }),
        Verdict::Person
    );
    assert_eq!(
        verdict(Evidence {
            title: "Justice Mix",
            description: "Justice, Kavinsky and more",
            ..Default::default()
        }),
        Verdict::Unsure
    );
    for title in ["Justice Remixes", "Social Justice Anthems"] {
        assert_eq!(
            verdict(Evidence {
                title,
                session_names: vec!["Justice"],
                ..Default::default()
            }),
            Verdict::Unsure,
            "{title}"
        );
    }
    assert_eq!(
        verdict(Evidence {
            title: "Chillwave nights",
            session_names: vec!["Chill"],
            ..Default::default()
        }),
        Verdict::Theme
    );
    assert_eq!(
        verdict(Evidence {
            title: "Low key",
            session_names: vec!["Low"],
            ..Default::default()
        }),
        Verdict::Unsure
    );
    assert_eq!(
        verdict(Evidence {
            title: "Mo Mood",
            session_names: vec!["Mo"],
            ..Default::default()
        }),
        Verdict::Theme
    );
    assert!(title_names("Love Ty Dolla $ign", "Ty Dolla $ign"));
    assert!(!title_names("Dolla bills", "Ty Dolla $ign"));

    let rows = |artists: &[&'static str]| {
        artists
            .iter()
            .map(|artist| vec![*artist])
            .collect::<Vec<_>>()
    };
    let many = rows(&["a", "a", "a", "a", "b", "c", "d", "e"]);
    assert_eq!(
        verdict(Evidence {
            title: "my favourites",
            rows: many,
            ..Default::default()
        }),
        Verdict::Person
    );
    let spread = rows(&["a", "a", "a", "b", "c", "d", "e", "f"]);
    assert_eq!(
        verdict(Evidence {
            title: "my favourites",
            rows: spread,
            ..Default::default()
        }),
        Verdict::Theme
    );
    let few = rows(&["a", "a", "a"]);
    assert_eq!(
        verdict(Evidence {
            title: "my favourites",
            rows: few,
            ..Default::default()
        }),
        Verdict::Theme
    );
    assert_eq!(
        dominant_share(&[vec!["a", "b"], vec!["a"], vec!["c"], vec!["a", "a"]]),
        0.75
    );

    assert_eq!(
        verdict(Evidence {
            title: "Neon nights",
            description: "This is Justice. The essential tracks, all in one playlist.",
            ..Default::default()
        }),
        Verdict::Person
    );
    assert_eq!(
        verdict(Evidence {
            title: "Today\u{2019}s list",
            description: "The hottest 50. Cover: someone",
            ..Default::default()
        }),
        Verdict::Popularity
    );
    assert_eq!(
        names_in_description("With Bailey Zimmerman, Tucker Wetmore, Jordan Davis and more"),
        ["Bailey Zimmerman", "Tucker Wetmore", "Jordan Davis"]
    );
    assert_eq!(
        names_in_description("Justice, Tame Impala and SZA"),
        ["Justice", "Tame Impala", "SZA"]
    );
    assert!(names_in_description("Chill vibes. Lofi beats to relax to.").is_empty());

    assert_eq!(verdict(titled("")), Verdict::Unsure);
    assert!(Verdict::Theme.shown());
    for hidden in [Verdict::Person, Verdict::Popularity, Verdict::Unsure] {
        assert!(!hidden.shown());
    }
    assert!(collections::is_spotify_playlist("37i9dQZF1DXcBWIGoYBM5M"));
    assert!(!collections::is_spotify_playlist("175ilrDuQOmHLb36aF59EK"));

    let formatted = |title, format| {
        verdict(Evidence {
            title,
            format,
            ..Default::default()
        })
    };
    assert_eq!(
        formatted("Alfonso Peduto Mix", "artist-mix-reader"),
        Verdict::Person
    );
    assert_eq!(
        formatted("This Is Daft Punk", "artistsets"),
        Verdict::Person
    );
    assert_eq!(formatted("Chill Mix", "topic-mix"), Verdict::Theme);
    assert_eq!(formatted("", "inspiredby-mix"), Verdict::Person);
    assert_eq!(formatted("Top 50 - Global", "chart"), Verdict::Popularity);
    assert_eq!(formatted("Chill Mix", "some-other-format"), Verdict::Theme);
}

/// Where JavaScript's regular expressions and Rust's differ, the rules
/// follow JavaScript, as the extension runs them: its whitespace (U+FEFF
/// in, U+0085 out), and ASCII digits and case outside `title_names`.
#[test]
fn playlist_rules_read_text_as_javascript_does() {
    assert_eq!(verdict(titled("\u{FEFF}This is Adele")), Verdict::Person);
    assert_eq!(verdict(titled("Morgan Radio\u{FEFF}")), Verdict::Person);
    assert_eq!(verdict(titled("\u{FEFF}")), Verdict::Unsure);
    assert_eq!(verdict(titled("\u{85}This is Adele")), Verdict::Theme);
    assert_eq!(
        names_in_description("With\u{FEFF}A, B and more"),
        ["A", "B"]
    );
    assert_eq!(names_in_description("A\u{FEFF}&\u{FEFF}B"), ["A", "B"]);
    assert_eq!(verdict(titled("Viral \u{FF15}\u{FF10}")), Verdict::Theme);
    assert_eq!(verdict(titled("Hot \u{661}\u{660}\u{660}")), Verdict::Theme);
    assert_eq!(verdict(titled("Thi\u{17F} is Adele")), Verdict::Theme);
    assert_eq!(
        verdict(Evidence {
            title: "Mix",
            description: "pic\u{212A}ed by us",
            ..Default::default()
        }),
        Verdict::Theme
    );
    // Names in titles are matched as JavaScript's /iu matches them.
    assert!(title_names("ABBA forever", "abba"));
    assert!(title_names("x-a-a-y", "a-y"));
    assert!(!title_names("Abbatoir", "abba"));
    // Many names stay quick: no regular expression compiled per name.
    let names: Vec<String> = (0..2000)
        .map(|index| format!("artist number {index}"))
        .collect();
    let started = std::time::Instant::now();
    let evidence = Evidence {
        title: "rainy day",
        session_names: names.iter().map(String::as_str).collect(),
        ..Default::default()
    };
    assert_eq!(verdict(evidence), Verdict::Theme);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        started.elapsed()
    );
}

fn test_gate(options: gate::Options) -> gate::Gate {
    let seed = Seed::custom("test seed one").unwrap();
    gate::Gate::new(
        gate::Level::Disguised,
        std::sync::Arc::new(deriver(&seed)),
        seed.id.clone(),
        options,
        gate::Own {
            user_id: Some("me".into()),
            user_name: Some("Me Myself".into()),
            playlists: ["mine".to_string()].into(),
        },
    )
}

/// The gate: stand-ins for everything but your own things and Spotify,
/// learnt so search finds what a stand-in stands for, and nothing at all
/// once switched off.
#[test]
fn the_gate_shows_stand_ins_and_searches_find_the_real_thing() {
    gate::set(Some(test_gate(gate::Options::default())));
    let artist = gate::artist("Daft Punk").into_owned();
    assert_ne!(artist, "Daft Punk");
    assert_eq!(artist.split(' ').count(), 3);
    assert_eq!(
        gate::song("One More Time"),
        gate::album("One More Time"),
        "songs are named as albums"
    );
    // Your own things, and Spotify, stay as they are.
    assert_eq!(gate::user(Some("me"), "Me Myself"), "Me Myself");
    assert_eq!(gate::user(Some("spotify"), "Spotify"), "Spotify");
    assert_ne!(gate::user(Some("someone"), "Someone Else"), "Someone Else");
    assert_eq!(gate::playlist("mine", Some("me"), "Road trip"), "Road trip");
    // Spotify's own playlists list you as their owner but are never yours.
    assert_ne!(
        gate::playlist("37i9dQZF1DXcBWIGoYBM5M", Some("me"), "Daily Mix 1"),
        "Daily Mix 1"
    );
    // Descriptions are hidden by default; free text swaps known names.
    assert_eq!(gate::artist_description("A band from Paris."), gate::BLANK);
    assert_eq!(
        gate::text("Daft Punk live"),
        format!("{artist} live"),
        "a name Vacuum has shown is swapped inside text"
    );
    // Search: a stand-in, the start of exactly one, or one inside a query.
    assert_eq!(gate::real_query(&artist), "Daft Punk");
    assert_eq!(gate::real_query(&artist.to_lowercase()), "Daft Punk");
    let start: String = artist.chars().take(artist.len() - 2).collect();
    assert_eq!(gate::real_query(&start), "Daft Punk");
    assert_eq!(
        gate::real_query(&format!("{artist} remix")),
        "Daft Punk remix"
    );
    assert_eq!(gate::real_query("something else"), "something else");
    // Pictures: generated, never the real one, drawn from the name.
    let picture = gate::art(
        gate::ArtKind::Artist,
        Some("4tZwfgrHOc3mvqYlEYSvVi"),
        "Daft Punk",
        Some("https://i.scdn.co/x"),
    )
    .unwrap();
    assert!(picture.starts_with(gate::ART), "{picture}");
    assert!(!picture.contains("scdn"));
    // Your playlist keeps the cover you gave it, but not one Spotify made.
    assert_eq!(
        gate::art(
            gate::ArtKind::Playlist,
            Some("mine"),
            "Road trip",
            Some("https://example.com/mine.jpg")
        )
        .as_deref(),
        Some("https://example.com/mine.jpg")
    );
    assert!(
        gate::art(
            gate::ArtKind::Playlist,
            Some("mine"),
            "Road trip",
            Some("https://mosaic.scdn.co/640/a")
        )
        .unwrap()
        .starts_with("vacuum-")
    );
    // Never an empty hole: no name, then the ID.
    assert!(
        gate::art(
            gate::ArtKind::Album,
            Some("1V8Qu6CwkBfYucnCWxynq8"),
            "",
            None
        )
        .is_some()
    );
    // The generated picture can be drawn again from its address alone.
    let (drawn, round) = gate::generated_picture(&picture).unwrap();
    assert!(round);
    assert_eq!(
        drawn,
        deriver(&Seed::custom("test seed one").unwrap())
            .derive(Kind::Artist, "Daft Punk")
            .unwrap()
            .picture
    );
    gate::set(None);
    assert_eq!(
        gate::artist("Daft Punk"),
        "Daft Punk",
        "off, everything is real again"
    );
    assert_eq!(gate::real_query(&artist), artist);
}

/// The game's level: everything blank, yours included.
#[test]
fn the_blanked_level_hides_everything() {
    let mut blank = test_gate(gate::Options::default());
    blank.level = gate::Level::Blanked;
    gate::set(Some(blank));
    assert_eq!(gate::song("One More Time"), gate::BLANK);
    assert_eq!(gate::playlist("mine", Some("me"), "Road trip"), gate::BLANK);
    assert_eq!(
        gate::art(gate::ArtKind::Album, Some("x"), "y", None).as_deref(),
        Some(gate::BLANK_ART)
    );
    gate::set(None);
}

#[test]
fn generated_and_blank_pictures_draw() {
    let picture = deriver(&Seed::custom("test seed one").unwrap())
        .derive(Kind::Album, "Blade Runner Blues")
        .unwrap()
        .picture;
    let png = art::generated_png(&picture).unwrap();
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']));
    let image = image::load_from_memory(&png).unwrap();
    assert_eq!((image.width(), image.height()), (640, 640));
    // A framed artist picture: the ring colours round a photograph.
    let photo = image::RgbaImage::from_pixel(10, 20, image::Rgba([255, 0, 0, 255]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    photo.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let framed = image::load_from_memory(&art::render(bytes.get_ref(), true, [8, 1]).unwrap())
        .unwrap()
        .to_rgba8();
    assert_eq!(
        framed.get_pixel(200, 200).0,
        [255, 0, 0, 255],
        "the photograph in the middle"
    );
    assert_eq!(
        framed.get_pixel(200, 3).0[..3],
        [0xF0, 0xE4, 0x42],
        "the outer ring"
    );
    assert_eq!(
        art::parse_picture_list(
            "a,https://x.org/1.jpg\n\"https://x.org/2.jpg\";https://x.org/1.jpg\thttp://no"
        ),
        ["https://x.org/1.jpg", "https://x.org/2.jpg"]
    );
}

/// A name inside a message: its stand-in if known, kept if already a
/// stand-in, or a stand-in of its own: never shown as it is. Names inside
/// text are swapped as whole phrases only, however many are known.
#[test]
fn names_in_messages_are_never_shown_real() {
    gate::set(Some(test_gate(gate::Options::default())));
    let artist = gate::artist("Daft Punk").into_owned();
    assert_eq!(gate::name("Daft Punk"), artist);
    assert_eq!(gate::name(&artist), artist, "a stand-in stays");
    let unknown = gate::name("Some Album Nobody Drew").into_owned();
    assert_ne!(unknown, "Some Album Nobody Drew");
    assert_eq!(
        gate::real_query(&unknown),
        "Some Album Nobody Drew",
        "and searches find it"
    );
    assert_eq!(
        gate::text("Daft Punkish"),
        "Daft Punkish",
        "whole phrases only"
    );
    assert_eq!(gate::text("ÜBER Daft Punk!"), format!("ÜBER {artist}!"));
    for index in 0..25_000 {
        gate::album(&format!("album number {index}"));
    }
    let late = gate::album("The Last One").into_owned();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert_eq!(gate::text("The Last One"), late, "no limit on names known");
    gate::set(None);
}
