//! The colour editor under Settings → Appearance: every colour of the
//! palette in use, changed with a picker or a hex value and shown at once,
//! then saved as a theme of the user's own, with no palette file to write.

use egui::{Align, Layout, Vec2};

use crate::app::App;
use crate::i18n::{Locale, gettext};
use crate::model::Action;
use crate::theme::{self, Icon, Palette};

use super::widgets;

/// What each colour is called in the editor, by its key in a palette file.
fn label(locale: Locale, key: &str) -> std::borrow::Cow<'static, str> {
    match key {
        // Translators: A colour in the theme editor: the window's background.
        "window" => gettext(locale, "Background"),
        // Translators: A colour in the theme editor.
        "panel" => gettext(locale, "Side panels"),
        // Translators: A colour in the theme editor.
        "surface" => gettext(locale, "Cards and fields"),
        // Translators: A colour in the theme editor.
        "surface_hover" => gettext(locale, "Items under the pointer"),
        // Translators: A colour in the theme editor.
        "surface_active" => gettext(locale, "Pressed items"),
        // Translators: A colour in the theme editor.
        "outline" => gettext(locale, "Lines"),
        // Translators: A colour in the theme editor.
        "text" => gettext(locale, "Text"),
        // Translators: A colour in the theme editor.
        "secondary" => gettext(locale, "Secondary text"),
        // Translators: A colour in the theme editor.
        "dim" => gettext(locale, "Faint text"),
        // Translators: A colour in the theme editor: buttons, the playing song and other highlights.
        "accent" => gettext(locale, "Accent"),
        // Translators: A colour in the theme editor.
        "accent_hover" => gettext(locale, "Accent under the pointer"),
        // Translators: A colour in the theme editor.
        "on_accent" => gettext(locale, "Text on the accent"),
        // Translators: A colour in the theme editor.
        "danger" => gettext(locale, "Errors"),
        // Translators: A colour in the theme editor.
        "warning" => gettext(locale, "Warnings"),
        // Translators: A colour in the theme editor.
        "overlay" => gettext(locale, "Menus and popups"),
        // Translators: A colour in the theme editor.
        "shadow" => gettext(locale, "Shadows"),
        // Translators: A colour in the theme editor: the wash at the top of a page without cover art.
        "tint" => gettext(locale, "Page tint"),
        _ => gettext(locale, "Colour"),
    }
}

/// The editor, below the Theme row: a button to begin, then every colour,
/// the light or dark base, a name, and Save or Discard.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    let locale = app.locale;
    let Some(draft) = app.theme_draft else {
        return;
    };
    let mut edited = draft;
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
        for (dark, text) in [
            (true, gettext(locale, "Dark")),
            (false, gettext(locale, "Light")),
        ] {
            if theme::soft_button(ui, &palette, None, &text, edited.dark == dark).clicked()
                && edited.dark != dark
            {
                // A new base starts from that base's own colours.
                let mut based = if dark {
                    Palette::dark()
                } else {
                    Palette::light()
                };
                based.accent = edited.accent;
                based.accent_hover = edited.accent_hover;
                edited = based;
            }
        }
    });
    ui.add_space(6.0);
    // Two columns where both fit whole: picker, name and hex each.
    let columns = if ui.available_width() >= 680.0 { 2 } else { 1 };
    let keys = theme::PALETTE_KEYS;
    egui::Grid::new("theme-editor-colours")
        .num_columns(columns * 3)
        .spacing(Vec2::new(10.0, 6.0))
        .show(ui, |ui| {
            for (index, key) in keys.iter().enumerate() {
                let name = label(locale, key);
                let mut color = theme::palette_color(&edited, key);
                // Every colour keeps its alpha: a palette may make any of
                // them translucent.
                let alpha = egui::color_picker::Alpha::OnlyBlend;
                let picked = egui::color_picker::color_edit_button_srgba(ui, &mut color, alpha);
                picked.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, true, name.as_ref())
                });
                if picked.changed() {
                    theme::set_palette_color(&mut edited, key, color);
                }
                ui.allocate_ui_with_layout(
                    Vec2::new(170.0, 22.0),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_width(170.0);
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(name.as_ref())
                                    .font(theme::regular(13.0))
                                    .color(palette.text),
                            )
                            .truncate(),
                        );
                    },
                );
                // The hex field keeps what is typed until it reads as a colour.
                let id = ui.make_persistent_id(("theme-editor-hex", key));
                let shown = theme::hex(color);
                let mut text = ui
                    .data(|data| data.get_temp::<String>(id))
                    .unwrap_or_else(|| shown.clone());
                let field = ui.add_sized(
                    Vec2::new(96.0, 22.0),
                    egui::TextEdit::singleline(&mut text).font(theme::regular(13.0)),
                );
                if field.changed()
                    && let Some(typed) = theme::parse_hex(&text)
                {
                    theme::set_palette_color(&mut edited, key, typed);
                }
                if field.has_focus() {
                    ui.data_mut(|data| data.insert_temp(id, text));
                } else {
                    ui.data_mut(|data| data.remove::<String>(id));
                }
                if (index + 1) % columns == 0 {
                    ui.end_row();
                }
            }
        });
    if edited != draft {
        app.actions.push(Action::SetThemeDraft(Some(edited)));
    }
    ui.add_space(10.0);
    let name_id = ui.make_persistent_id("theme-editor-name");
    let mut name = ui
        .data(|data| data.get_temp::<String>(name_id))
        .unwrap_or_default();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::splat(8.0);
        widgets::text_edit(
            ui,
            locale,
            egui::TextEdit::singleline(&mut name)
                .hint_text(gettext(locale, "Theme name"))
                .font(theme::regular(14.0))
                .desired_width(180.0),
        );
        let ready = theme::theme_filename(&name).is_some();
        let save = ui
            .add_enabled_ui(ready, |ui| {
                theme::soft_button(
                    ui,
                    &palette,
                    Some(Icon::Check),
                    &gettext(locale, "Save theme"),
                    false,
                )
            })
            .inner;
        if ready && save.clicked() {
            app.actions
                .push(Action::SaveThemeDraft(name.trim().to_string()));
            name.clear();
        }
        if theme::soft_button(ui, &palette, None, &gettext(locale, "Discard"), false).clicked() {
            app.actions.push(Action::SetThemeDraft(None));
            name.clear();
        }
    });
    ui.data_mut(|data| data.insert_temp(name_id, name));
}

/// The button that opens the editor, starting from the colours in use.
pub fn start_button(app: &mut App, ui: &mut egui::Ui) {
    let palette = app.palette;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let editing = app.theme_draft.is_some();
        if !editing
            && theme::soft_button(
                ui,
                &palette,
                Some(Icon::Pencil),
                &gettext(app.locale, "Customize"),
                false,
            )
            .clicked()
        {
            app.actions.push(Action::SetThemeDraft(Some(app.palette)));
        }
    });
}
