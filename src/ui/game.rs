//! The guessing game's page: how to play the next game, the song being
//! guessed, and the reveal. See [`crate::game`] for the rules.

use egui::{Color32, CornerRadius, Rect, Sense, Vec2, vec2};

use super::widgets;
use crate::api::models::pick_image;
use crate::app::App;
use crate::game::{
    CATEGORIES, COLOURS, Category, Colour, GRID_SIZES, Hiding, LIBRARY, Mode, Setup,
};
use crate::i18n::{Locale, gettext};
use crate::model::Action;
use crate::theme::{self, Icon};

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.add_space(8.0);
    theme::text(
        ui,
        gettext(app.locale, "Guessing game"),
        theme::bold(28.0),
        palette.text,
    );
    ui.add_space(4.0);
    if app.game.is_some() {
        playing(app, ui);
    } else {
        setup(app, ui);
    }
}

fn category_label(locale: Locale, category: Category) -> String {
    match category {
        Category::Title => gettext(locale, "Title"),
        Category::Artist => gettext(locale, "Artist"),
        Category::Album => gettext(locale, "Album"),
        Category::Year => gettext(locale, "Year"),
        // Translators: Guessing game: a song's version, such as Live or 2011 Remaster.
        Category::Version => gettext(locale, "Version"),
    }
    .into_owned()
}

fn choice(ui: &mut egui::Ui, app: &App, label: &str, selected: bool) -> bool {
    let response = theme::soft_button(ui, &app.palette, None, label, selected);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::RadioButton, true, selected, label)
    });
    response.clicked() && !selected
}

/// How to play the next game, and the stats so far.
fn setup(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    theme::text(
        ui,
        gettext(
            locale,
            "Songs play from where you choose, in a shuffled order. Name what you can before the first lyric: title 1 point, artist 2, album 3, year 4, version 5.",
        ),
        theme::regular(13.5),
        palette.secondary,
    );
    ui.add_space(16.0);
    // Where the songs come from.
    let liked = app
        .user_id()
        .map(|id| format!("spotify:user:{id}:collection"))
        .unwrap_or_default();
    let source_id = egui::Id::new("game-source");
    let mut source: String = ui
        .data(|data| data.get_temp(source_id))
        .or_else(|| app.game_source.clone())
        .unwrap_or_else(|| liked.clone());
    let mut sources: Vec<(String, String)> = Vec::new();
    if let Some(chosen) = &app.game_source
        && *chosen != liked
        && chosen != LIBRARY
    {
        sources.push((chosen.clone(), source_name(app, chosen)));
    }
    sources.push((liked.clone(), gettext(locale, "Liked Songs").into_owned()));
    // Translators: Guessing game source: Liked Songs and every playlist opened.
    sources.push((
        LIBRARY.to_string(),
        gettext(locale, "Your library").into_owned(),
    ));
    section_label(ui, app, &gettext(locale, "Play from"));
    ui.horizontal_wrapped(|ui| {
        for (uri, name) in &sources {
            if choice(ui, app, name, *uri == source) {
                source.clone_from(uri);
                app.actions.push(Action::LoadGameSongs(uri.clone()));
            }
        }
    });
    ui.data_mut(|data| data.insert_temp(source_id, source.clone()));
    ui.add_space(12.0);
    let mut setup: Setup = app.game_record.setup;
    section_label(ui, app, &gettext(locale, "Mode"));
    ui.horizontal_wrapped(|ui| {
        for (mode, label) in [
            (Mode::Honor, gettext(locale, "Name it")),
            // Translators: Guessing game mode: name the song by typing; only these count in the stats.
            (Mode::Typed, gettext(locale, "Name it, typed")),
            (Mode::CoverGrid, gettext(locale, "Cover grid")),
            (Mode::MainColour, gettext(locale, "Main colour")),
        ] {
            if choice(ui, app, &label, setup.mode == mode) {
                setup.mode = mode;
            }
        }
    });
    theme::text(
        ui,
        match setup.mode {
            Mode::Honor => gettext(locale, "Say your answers aloud, then tick what you got."),
            Mode::Typed => gettext(
                locale,
                "Type your answers. Close enough counts; these rounds make the stats.",
            ),
            Mode::CoverGrid => gettext(locale, "Pick the playing song's cover."),
            Mode::MainColour => gettext(
                locale,
                "Say what colour the playing song's cover mostly is.",
            ),
        },
        theme::regular(12.5),
        palette.dim,
    );
    if setup.mode == Mode::CoverGrid {
        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            for size in GRID_SIZES {
                // Translators: Guessing game: how many covers the grid shows. {count} is a number.
                let label = gettext(locale, "{count} covers").replace("{count}", &size.to_string());
                if choice(ui, app, &label, setup.grid == size) {
                    setup.grid = size;
                }
            }
        });
    }
    ui.add_space(12.0);
    section_label(ui, app, &gettext(locale, "While you guess"));
    ui.horizontal_wrapped(|ui| {
        for (hiding, label) in [
            // Translators: Guessing game: show the real names and covers while guessing.
            (Hiding::Real, gettext(locale, "Real names")),
            // Translators: Guessing game: show Vacuum's stand-ins while guessing.
            (Hiding::Disguised, gettext(locale, "Stand-ins")),
            // Translators: Guessing game: show ??? and black squares while guessing.
            (Hiding::Blanked, gettext(locale, "Blanked")),
        ] {
            if choice(ui, app, &label, setup.hiding == hiding) {
                setup.hiding = hiding;
            }
        }
    });
    if setup != app.game_record.setup {
        app.actions.push(Action::SetGameSetup(setup));
    }
    ui.add_space(20.0);
    let songs = app.game_songs(&source);
    let ready = songs.as_ref().is_some_and(|songs| !songs.is_empty());
    ui.horizontal(|ui| {
        if ui
            .add_enabled_ui(ready, |ui| {
                theme::pill_button(ui, &palette, &gettext(locale, "Start"), true)
            })
            .inner
            .clicked()
        {
            app.actions.push(Action::StartGame(source.clone()));
        }
        let note = match &songs {
            None => {
                app.actions.push(Action::LoadGameSongs(source.clone()));
                gettext(locale, "Loading the songs…").into_owned()
            }
            Some(songs) => {
                let now = jiff::Timestamp::now().as_second();
                let due = songs
                    .iter()
                    .filter(|song| app.game_record.due(&song.uri, now))
                    .count();
                // Translators: Guessing game: {count} songs to play, {due} of them missed before and due again.
                gettext(locale, "{count} songs, {due} due to come back")
                    .replace("{count}", &songs.len().to_string())
                    .replace("{due}", &due.to_string())
            }
        };
        theme::text(ui, note, theme::regular(12.5), palette.secondary);
    });
    ui.add_space(28.0);
    stats(app, ui);
}

fn section_label(ui: &mut egui::Ui, app: &App, label: &str) {
    theme::text(ui, label, theme::semibold(13.0), app.palette.text);
    ui.add_space(4.0);
}

/// The name of a playlist or album to play from, as the screen shows it.
fn source_name(app: &App, uri: &str) -> String {
    if let Some(id) = uri.strip_prefix("spotify:playlist:") {
        return app
            .playlist_pages
            .get(id)
            .and_then(|page| page.playlist.get())
            .or_else(|| app.known_playlist(id))
            .map(|playlist| playlist.shown_name().into_owned())
            .unwrap_or_else(|| gettext(app.locale, "Playlist").into_owned());
    }
    if let Some(id) = uri.strip_prefix("spotify:album:") {
        return app
            .album_pages
            .get(id)
            .and_then(|page| page.album.get())
            .or_else(|| app.known_album(id))
            .map(|album| album.shown_name().into_owned())
            .unwrap_or_else(|| gettext(app.locale, "Album").into_owned());
    }
    uri.to_string()
}

fn stats(app: &App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    theme::section_title(ui, &palette, &gettext(locale, "Your stats"));
    ui.add_space(4.0);
    let Some(stats) = app.game_record.stats() else {
        let typed = app
            .game_record
            .rounds
            .iter()
            .filter(|round| round.mode == Mode::Typed)
            .count();
        theme::text(
            ui,
            // Translators: {count} is how many typed rounds were played so far.
            gettext(
                locale,
                "Stats show after 10 songs named by typing. {count} so far.",
            )
            .replace("{count}", &typed.to_string()),
            theme::regular(13.0),
            palette.secondary,
        );
        return;
    };
    let line = |ui: &mut egui::Ui, label: String, value: String| {
        ui.horizontal(|ui| {
            ui.set_min_width(260.0);
            theme::text(ui, label, theme::regular(13.0), palette.secondary);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                theme::text(ui, value, theme::semibold(13.0), palette.text);
            });
        });
    };
    ui.vertical(|ui| {
        ui.set_max_width(320.0);
        line(
            ui,
            gettext(locale, "Songs").into_owned(),
            stats.songs.to_string(),
        );
        line(
            ui,
            gettext(locale, "Points per song").into_owned(),
            format!("{:.1}", stats.points_per_song),
        );
        if let Some(ms) = stats.average_answer_ms {
            line(
                ui,
                gettext(locale, "Average time to answer").into_owned(),
                crate::util::format_total_ms(locale, u64::from(ms)),
            );
        }
        for (category, share) in &stats.accuracy {
            line(
                ui,
                category_label(locale, *category),
                format!("{:.0}%", share * 100.0),
            );
        }
        if stats.recent_points.len() > 1 {
            ui.add_space(6.0);
            theme::text(
                ui,
                gettext(locale, "Points per song, week by week"),
                theme::regular(12.5),
                palette.dim,
            );
            let weeks: Vec<String> = stats
                .recent_points
                .iter()
                .map(|points| format!("{points:.1}"))
                .collect();
            theme::text(ui, weeks.join("  →  "), theme::semibold(13.0), palette.text);
        }
    });
}

/// The song being guessed, or revealed.
fn playing(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let Some(session) = &app.game else {
        return;
    };
    let (mode, played, total_points) = (session.setup.mode, session.played, session.points);
    ui.horizontal(|ui| {
        theme::text(
            ui,
            // Translators: Guessing game: {count} songs played so far and {points} points scored.
            gettext(locale, "{count} songs · {points} points")
                .replace("{count}", &played.to_string())
                .replace("{points}", &total_points.to_string()),
            theme::regular(13.0),
            palette.secondary,
        );
        ui.add_space(12.0);
        if theme::soft_button(ui, &palette, None, &gettext(locale, "End game"), false).clicked() {
            app.actions.push(Action::EndGame);
        }
    });
    ui.add_space(16.0);
    let position = app.now_playing().map_or(0, |now| now.position_ms);
    let Some(round) = app.game.as_ref().and_then(|session| session.round.as_ref()) else {
        between_songs(app, ui);
        return;
    };
    let revealed = round.revealed;
    if !revealed {
        countdown(app, ui, position);
        ui.add_space(14.0);
        match mode {
            Mode::Honor => {
                theme::text(
                    ui,
                    gettext(
                        locale,
                        "Name what you can: title, artist, album, year, version.",
                    ),
                    theme::regular(14.0),
                    palette.text,
                );
                ui.add_space(12.0);
                if theme::pill_button(ui, &palette, &gettext(locale, "Reveal"), true).clicked() {
                    app.actions.push(Action::RevealRound);
                }
            }
            Mode::Typed => typed_answers(app, ui),
            Mode::CoverGrid => cover_grid(app, ui),
            Mode::MainColour => colour_swatches(app, ui),
        }
        return;
    }
    reveal(app, ui, mode);
}

/// No song is being guessed: the game has not begun, another song plays,
/// or every song has been played.
fn between_songs(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let Some(session) = &app.game else {
        return;
    };
    if session.done.is_empty() {
        theme::text(
            ui,
            gettext(locale, "Waiting for the first song…"),
            theme::regular(14.0),
            palette.secondary,
        );
        return;
    }
    let over = session
        .songs
        .iter()
        .all(|track| session.done.contains(&track.uri));
    if over {
        theme::text(
            ui,
            // Translators: Guessing game: every song of the game has been played.
            gettext(locale, "Game over"),
            theme::bold(20.0),
            palette.text,
        );
        ui.add_space(12.0);
        let source = session.source.clone();
        if theme::pill_button(ui, &palette, &gettext(locale, "Play again"), true).clicked() {
            app.actions.push(Action::StartGame(source));
        }
        return;
    }
    theme::text(
        ui,
        // Translators: Guessing game: a song from elsewhere, or one already guessed, is playing.
        gettext(locale, "The playing song is not one to guess."),
        theme::regular(14.0),
        palette.secondary,
    );
    ui.add_space(12.0);
    if theme::pill_button(ui, &palette, &gettext(locale, "Continue the game"), true).clicked() {
        app.actions.push(Action::ContinueGame);
    }
}

fn countdown(app: &App, ui: &mut egui::Ui, position: u32) {
    let palette = app.palette;
    let Some(round) = app.game.as_ref().and_then(|session| session.round.as_ref()) else {
        return;
    };
    let left = round.deadline_ms.saturating_sub(position);
    let share = (position as f32 / round.deadline_ms.max(1) as f32).clamp(0.0, 1.0);
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().min(520.0), 6.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(3), palette.surface);
    let filled = Rect::from_min_size(rect.min, vec2(rect.width() * share, rect.height()));
    ui.painter()
        .rect_filled(filled, CornerRadius::same(3), palette.accent);
    ui.add_space(6.0);
    let seconds = left.div_ceil(1000).to_string();
    theme::text(
        ui,
        if round.timed_by_lyrics {
            // Translators: Guessing game countdown. {seconds} is a number.
            gettext(app.locale, "{seconds} s until the first lyric").replace("{seconds}", &seconds)
        } else {
            // Translators: Guessing game countdown, for a song without synced lyrics. {seconds} is a number.
            gettext(app.locale, "{seconds} s left").replace("{seconds}", &seconds)
        },
        theme::regular(12.5),
        palette.secondary,
    );
}

fn typed_answers(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let mut submit = false;
    egui::Grid::new("game-answers")
        .num_columns(2)
        .min_col_width(90.0)
        .spacing(vec2(12.0, 8.0))
        .show(ui, |ui| {
            let field = |category: Category| egui::Id::new(("game-answer", category));
            for (index, category) in CATEGORIES.into_iter().enumerate() {
                theme::text(
                    ui,
                    category_label(locale, category),
                    theme::regular(13.5),
                    palette.secondary,
                );
                let Some(round) = app.game.as_mut().and_then(|session| session.round.as_mut())
                else {
                    return;
                };
                let answer = round.answers.entry(category).or_default();
                let response = ui.add(
                    egui::TextEdit::singleline(answer)
                        .id(field(category))
                        .desired_width(320.0),
                );
                // Enter moves to the next answer; on the last, it is done.
                if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter)) {
                    match CATEGORIES.get(index + 1) {
                        Some(next) => ui.memory_mut(|memory| memory.request_focus(field(*next))),
                        None => submit = true,
                    }
                }
                ui.end_row();
            }
        });
    ui.add_space(12.0);
    if theme::pill_button(ui, &palette, &gettext(locale, "Done"), true).clicked() || submit {
        app.actions.push(Action::RevealRound);
    }
}

fn cover_grid(app: &mut App, ui: &mut egui::Ui) {
    let Some(round) = app.game.as_ref().and_then(|session| session.round.as_ref()) else {
        return;
    };
    let cells = round.grid.clone();
    let side = (cells.len() as f32).sqrt().ceil().max(1.0) as usize;
    let size =
        ((ui.available_width().min(640.0) - 8.0 * side as f32) / side as f32).clamp(56.0, 160.0);
    let mut picked = None;
    for row in cells.chunks(side) {
        ui.horizontal(|ui| {
            for (index, (uri, image)) in row.iter().enumerate() {
                let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
                widgets::paint_cover(
                    ui,
                    &app.palette,
                    image.as_deref(),
                    rect,
                    6.0,
                    Icon::Music,
                    Some(app.backend.art()),
                );
                if response.hovered() {
                    ui.painter().rect_stroke(
                        rect,
                        CornerRadius::same(6),
                        egui::Stroke::new(2.0, app.palette.accent),
                        egui::StrokeKind::Inside,
                    );
                }
                if response.clicked() {
                    picked = cells
                        .iter()
                        .position(|(cell, _)| cell == uri)
                        .or(Some(index));
                }
            }
        });
    }
    if let Some(index) = picked {
        app.actions.push(Action::PickCover(index));
    }
}

fn swatch(ui: &mut egui::Ui, colour: Colour, size: f32) -> egui::Response {
    let [r, g, b] = colour.rgb();
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), Color32::from_rgb(r, g, b));
    if response.hovered() {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(8),
            egui::Stroke::new(2.0, Color32::WHITE),
            egui::StrokeKind::Inside,
        );
    }
    response
}

fn colour_name(locale: Locale, colour: Colour) -> String {
    match colour {
        Colour::Red => gettext(locale, "Red"),
        Colour::Orange => gettext(locale, "Orange"),
        Colour::Yellow => gettext(locale, "Yellow"),
        Colour::Green => gettext(locale, "Green"),
        Colour::Teal => gettext(locale, "Teal"),
        Colour::Blue => gettext(locale, "Blue"),
        Colour::Purple => gettext(locale, "Purple"),
        Colour::Pink => gettext(locale, "Pink"),
        Colour::Brown => gettext(locale, "Brown"),
        Colour::Black => gettext(locale, "Black"),
        Colour::Grey => gettext(locale, "Grey"),
        Colour::White => gettext(locale, "White"),
    }
    .into_owned()
}

fn colour_swatches(app: &mut App, ui: &mut egui::Ui) {
    let locale = app.locale;
    // An answer counts only once the cover has been read.
    let ready = app
        .game
        .as_ref()
        .and_then(|session| session.round.as_ref())
        .is_some_and(|round| round.scoreable(Mode::MainColour));
    let mut chosen = None;
    ui.add_enabled_ui(ready, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
            for colour in COLOURS {
                let name = colour_name(locale, colour);
                let response = swatch(ui, colour, 44.0).on_hover_text(&name);
                response.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, ready, &name)
                });
                if response.clicked() {
                    chosen = Some(colour);
                }
            }
        });
    });
    if !ready {
        ui.add_space(6.0);
        theme::text(
            ui,
            // Translators: Guessing game: the cover is being read for its colours.
            gettext(locale, "Reading the cover…"),
            theme::regular(12.5),
            app.palette.dim,
        );
    }
    if let Some(colour) = chosen {
        app.actions.push(Action::PickColour(colour));
    }
}

/// What the song is: the real song, as the game is for knowing it.
fn reveal(app: &mut App, ui: &mut egui::Ui, mode: Mode) {
    let palette = app.palette;
    let locale = app.locale;
    let Some(round) = app.game.as_ref().and_then(|session| session.round.clone()) else {
        return;
    };
    let truth = &round.truth;
    ui.horizontal(|ui| {
        let cover = round
            .track
            .album
            .as_ref()
            .and_then(|album| pick_image(&album.images, app.art_px(140.0)));
        widgets::cover(ui, &palette, cover, 140.0, 8.0, Icon::Music);
        ui.add_space(16.0);
        ui.vertical(|ui| {
            theme::text(ui, &truth.title, theme::bold(22.0), palette.text);
            theme::text(
                ui,
                truth.artists.join(", "),
                theme::regular(15.0),
                palette.secondary,
            );
            theme::text(ui, &truth.album, theme::regular(14.0), palette.secondary);
            let year = truth
                .year
                .clone()
                .unwrap_or_else(|| gettext(locale, "Year unknown").into_owned());
            let version = truth
                .version
                .clone()
                .unwrap_or_else(|| gettext(locale, "No version named").into_owned());
            theme::text(
                ui,
                format!("{year} · {version}"),
                theme::regular(13.0),
                palette.dim,
            );
        });
    });
    ui.add_space(14.0);
    match mode {
        Mode::Honor if !round.recorded => {
            theme::text(
                ui,
                gettext(locale, "Tick what you got:"),
                theme::regular(13.5),
                palette.text,
            );
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                for category in CATEGORIES {
                    let known = match category {
                        Category::Year => truth.year.is_some(),
                        Category::Version => truth.version.is_some(),
                        _ => true,
                    };
                    if !known {
                        continue;
                    }
                    let ticked = round.ticked.contains(&category);
                    let label = format!(
                        "{}  +{}",
                        category_label(locale, category),
                        category.points()
                    );
                    let response = theme::soft_button(
                        ui,
                        &palette,
                        ticked.then_some(Icon::Check),
                        &label,
                        ticked,
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, ticked, &label)
                    });
                    if response.clicked() {
                        app.actions.push(Action::TickRound(category));
                    }
                }
            });
        }
        Mode::Typed | Mode::Honor => {
            ui.horizontal_wrapped(|ui| {
                for category in CATEGORIES {
                    let Some(got) = App::round_got(&round, category) else {
                        continue;
                    };
                    let mark = if got { "✓" } else { "✗" };
                    theme::text(
                        ui,
                        format!("{mark} {}", category_label(locale, category)),
                        theme::semibold(13.5),
                        if got { palette.accent } else { palette.danger },
                    );
                    ui.add_space(10.0);
                }
            });
        }
        Mode::CoverGrid | Mode::MainColour if round.unscored => {
            theme::text(
                ui,
                // Translators: Guessing game: the cover could not be read, or too few covers were found.
                gettext(locale, "This song could not be scored."),
                theme::regular(14.0),
                palette.secondary,
            );
        }
        Mode::CoverGrid | Mode::MainColour => {
            let right = App::round_got(&round, Category::Title).unwrap_or(false);
            theme::text(
                ui,
                if right {
                    gettext(locale, "Right!")
                } else {
                    gettext(locale, "Not this time.")
                },
                theme::bold(16.0),
                if right {
                    palette.accent
                } else {
                    palette.danger
                },
            );
            if mode == Mode::MainColour {
                let colours = round.colours_read().unwrap_or_default();
                if !colours.is_empty() {
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        theme::text(
                            ui,
                            gettext(locale, "The cover is mostly"),
                            theme::regular(13.0),
                            palette.secondary,
                        );
                        for colour in colours {
                            swatch(ui, colour, 22.0).on_hover_text(colour_name(locale, colour));
                        }
                    });
                }
            }
        }
    }
    if round.recorded && !round.unscored {
        ui.add_space(6.0);
        {
            theme::text(
                ui,
                // Translators: Guessing game: points scored for this song. {points} is a number.
                gettext(locale, "+{points} points").replace("{points}", &round.points.to_string()),
                theme::semibold(14.0),
                palette.text,
            );
        }
    }
    ui.add_space(16.0);
    // The last song of the game, revealed: nothing comes next.
    let over = app.game.as_ref().is_some_and(|session| {
        session
            .songs
            .iter()
            .all(|track| session.done.contains(&track.uri))
    });
    if over {
        theme::text(
            ui,
            gettext(locale, "Game over"),
            theme::bold(20.0),
            palette.text,
        );
        ui.add_space(12.0);
        if let Some(source) = app.game.as_ref().map(|session| session.source.clone())
            && theme::pill_button(ui, &palette, &gettext(locale, "Play again"), true).clicked()
        {
            app.actions.push(Action::StartGame(source));
        }
    } else if theme::pill_button(ui, &palette, &gettext(locale, "Next song"), true).clicked() {
        app.actions.push(Action::NextGameSong);
    }
}
