use super::*;
use crate::api::models::{Album, ArtistRef};

fn track(name: &str, artists: &[&str], album: &str, date: &str) -> Track {
    Track {
        name: name.into(),
        uri: format!("spotify:track:{}", name.len()),
        artists: artists
            .iter()
            .map(|name| ArtistRef {
                name: (*name).into(),
                ..Default::default()
            })
            .collect(),
        album: Some(Album {
            name: album.into(),
            release_date: Some(date.into()),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn versions_are_read_from_titles_and_never_guessed() {
    assert_eq!(
        split_version("Heroes - 2017 Remaster"),
        ("Heroes".into(), Some("2017 Remaster".into()))
    );
    assert_eq!(
        split_version("Song (Live)"),
        ("Song".into(), Some("Live".into()))
    );
    assert_eq!(
        split_version("Song [Mono Version]"),
        ("Song".into(), Some("Mono Version".into()))
    );
    assert_eq!(
        split_version("Song (feat. Someone)"),
        ("Song (feat. Someone)".into(), None)
    );
    assert_eq!(
        split_version("Hey - You"),
        ("Hey - You".into(), None),
        "a dash alone is no version"
    );
    let truth = Truth::of(&track(
        "Heroes - 2017 Remaster",
        &["David Bowie"],
        "\"Heroes\" (2017 Remaster)",
        "1977-10-14",
    ));
    assert_eq!(truth.title, "Heroes");
    assert_eq!(truth.year.as_deref(), Some("1977"));
    assert_eq!(truth.version.as_deref(), Some("2017 Remaster"));
}

#[test]
fn typed_answers_forgive_case_punctuation_credits_and_slips() {
    assert!(matches("dont stop me now", "Don't Stop Me Now"));
    assert!(matches("Beyonce", "Beyoncé"));
    assert!(matches("Simon and Garfunkel", "Simon & Garfunkel"));
    assert!(
        matches("Bohemian Rapsody", "Bohemian Rhapsody"),
        "one letter off"
    );
    assert!(matches("Get Lucky", "Get Lucky (feat. Pharrell Williams)"));
    assert!(matches("Stay", "STAY (with Justin Bieber)"));
    assert!(matches("2011 remastered", "2011 Remaster"));
    assert!(!matches("Lucky", "Get Lucky"));
    assert!(!matches("", "Anything"));
    assert!(!matches("Tea", "Tee"), "short names must be exact");
}

#[test]
fn numbers_must_be_exact_and_versions_take_any_word_order() {
    assert!(!matches("1998", "1999"));
    assert!(!matches("1984 album", "1989 album"));
    assert!(!matches_version("2012 Remaster", "2011 Remaster"));
    assert!(!matches_version("2009 Remaster", "2011 Remaster"));
    assert!(matches_version("Remastered 2011", "2011 Remaster"));
    assert!(matches_version("live", "Live Version"));
    assert!(matches_version(
        "2011 remaster live",
        "Live - 2011 Remaster"
    ));
    assert_eq!(
        split_version("Song - Live - 2011 Remaster"),
        ("Song".into(), Some("Live - 2011 Remaster".into())),
        "two versions are both the version"
    );
    assert_eq!(split_version("(Live)"), ("(Live)".into(), None));
}

#[test]
fn each_category_scores_on_its_own() {
    let truth = Truth::of(&track(
        "Get Lucky - Radio Edit",
        &["Daft Punk", "Pharrell Williams"],
        "Random Access Memories",
        "2013-05-17",
    ));
    let answers: Answers = [
        (Category::Title, "get lucky".to_string()),
        (Category::Artist, "Pharrell Williams".to_string()),
        (Category::Album, "wrong".to_string()),
        (Category::Year, "2013".to_string()),
        (Category::Version, "radio edit".to_string()),
    ]
    .into();
    let got = score(&truth, &answers);
    assert!(got[&Category::Title]);
    assert!(got[&Category::Artist], "any of the artists");
    assert!(!got[&Category::Album]);
    assert_eq!(
        points(&got),
        1 + 2 + 4 + 5,
        "the year counts without the album"
    );
    // Spotify names no version: there is nothing to score.
    let plain = Truth::of(&track("Song", &["A"], "B", "2001"));
    assert!(!score(&plain, &Answers::new()).contains_key(&Category::Version));
}

#[test]
fn the_deadline_is_the_first_lyric_or_a_fifth_of_the_song() {
    assert_eq!(deadline_ms(Some(12_400), 200_000), 12_400);
    assert_eq!(deadline_ms(None, 240_000), 48_000);
    assert_eq!(deadline_ms(Some(0), 200_000), 1_000, "never at once");
}

fn round(uri: &str, at: i64, title: bool, points: u32) -> Round {
    Round {
        at,
        uri: uri.into(),
        mode: Mode::Typed,
        got: [(Category::Title, title)].into(),
        answer_ms: Some(10_000),
        points,
    }
}

#[test]
fn misses_come_back_after_one_three_and_seven_days() {
    let mut record = Record::default();
    let day = 24 * 60 * 60;
    record.keep(round("a", 0, false, 0));
    assert!(!record.due("a", day - 1));
    assert!(record.due("a", day));
    record.keep(round("a", day / 2, false, 0));
    assert!(
        record.due("a", day),
        "a miss before it is due does not put it off"
    );
    record.keep(round("a", day, false, 0));
    assert!(record.due("a", 2 * day), "a miss when due starts over");
    record.keep(round("a", 2 * day, true, 1));
    assert!(!record.due("a", 5 * day - 1));
    assert!(record.due("a", 5 * day), "right, three days on");
    record.keep(round("a", 5 * day, true, 1));
    assert!(record.due("a", 12 * day), "right once more, seven days on");
    record.keep(round("a", 12 * day, true, 1));
    assert!(!record.reviews.contains_key("a"), "learnt");
    record.keep(round("b", 0, true, 1));
    assert!(
        !record.reviews.contains_key("b"),
        "a song got right is not reviewed"
    );
}

#[test]
fn stats_wait_for_ten_typed_songs() {
    let mut record = Record::default();
    for index in 0..9 {
        record.keep(round(&index.to_string(), index, index % 2 == 0, 3));
    }
    record.rounds.push(Round {
        mode: Mode::Honor,
        ..round("honor", 9, true, 15)
    });
    assert!(record.stats().is_none(), "honor rounds do not count");
    record.keep(round("last", 10, true, 3));
    let stats = record.stats().unwrap();
    assert_eq!(stats.songs, 10);
    assert_eq!(stats.points_per_song, 3.0);
    assert_eq!(stats.accuracy, vec![(Category::Title, 0.6)]);
    assert_eq!(stats.average_answer_ms, Some(10_000));
}

#[test]
fn covers_are_read_for_their_main_colour() {
    assert_eq!(Colour::nearest([200, 30, 30]), Colour::Red);
    assert_eq!(Colour::nearest([40, 80, 200]), Colour::Blue);
    assert_eq!(Colour::nearest([10, 10, 10]), Colour::Black);
    assert_eq!(Colour::nearest([250, 248, 245]), Colour::White);
    assert_eq!(Colour::nearest([100, 60, 30]), Colour::Brown);
    let half = image::RgbImage::from_fn(40, 40, |x, _| {
        if x < 20 {
            image::Rgb([30, 60, 210])
        } else {
            image::Rgb([240, 210, 50])
        }
    });
    let mut bytes = std::io::Cursor::new(Vec::new());
    half.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let colours = cover_colours(bytes.get_ref());
    assert!(
        colours.contains(&Colour::Blue) && colours.contains(&Colour::Yellow),
        "{colours:?}"
    );
}
