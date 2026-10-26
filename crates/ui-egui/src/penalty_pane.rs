//! The penalty-calculation pane: the values of the marked block around the caret.
//!
//! The task-pane host draws the title and the close button. Editing a field writes that text
//! into every linked place in the sentence and the table. Nothing is calculated here.

use serde_json::json;

use crate::WordApp;
use crate::theme::{Tokens, regular, semibold};

/// The pane body. The host only asks for it while the caret is inside a calculation whose pane
/// the user has not closed.
pub fn body(app: &mut WordApp, ui: &mut egui::Ui) {
    let Some(block) = wordcraft_engine::cmd::penalty::block_at(&app.session) else { return };
    let t = Tokens::get(ui.ctx());
    ui.label(
        egui::RichText::new(tl!("Changing a value writes it into every marked place in the text and the table. Nothing is calculated yet."))
            .font(regular(12.0))
            .color(t.text_dim),
    );
    ui.add_space(8.0);
    egui::ScrollArea::vertical().show(ui, |ui| {
        for field in &block.fields {
            ui.label(egui::RichText::new(tl!(field.label)).font(semibold(12.5)));
            let width = ui.available_width();
            edit_field(app, ui, "pane", &block.group, field.key, &field.value, width);
            if field.key == "penalty" {
                ui.label(egui::RichText::new(tl!("Entered by hand for now")).font(regular(11.0)).color(t.text_dim));
            }
            ui.add_space(8.0);
        }
    });
}

/// Calculation tab: the same values as the pane, shown while the caret is in the block.
pub fn ribbon(app: &mut WordApp, ui: &mut egui::Ui) {
    let Some(block) = wordcraft_engine::cmd::penalty::block_at(&app.session) else { return };
    let group = block.group.clone();
    let fields = block.fields.clone();
    crate::widgets::group(ui, "Calculation values", None, app, |ui, app| {
        for field in &fields {
            ui.vertical(|ui| {
                ui.set_width(128.0);
                ui.label(egui::RichText::new(tl!(field.label)).font(regular(11.0)));
                let resp = edit_field(app, ui, "ribbon", &group, field.key, &field.value, 124.0);
                if field.key == "penalty" {
                    resp.on_hover_text(tl!("Entered by hand for now"));
                }
            });
        }
    });
}

fn edit_field(app: &mut WordApp, ui: &mut egui::Ui, place: &str, group: &str, key: &str, value: &str, width: f32) -> egui::Response {
    let id = ui.make_persistent_id(("penalty-field", place, group, key));
    let mut draft = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| value.to_string());
    if !ui.memory(|m| m.has_focus(id)) && draft != value {
        draft = value.to_string();
    }
    let resp = ui.add(egui::TextEdit::singleline(&mut draft).id(id).desired_width(width));
    let commit = (resp.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter) && resp.has_focus())) && draft != value;
    ui.data_mut(|d| d.insert_temp(id, draft.clone()));
    if commit {
        let _ = app.run("penalty.set", json!({"group": group, "key": key, "value": draft}));
    }
    resp
}

#[cfg(test)]
mod tests {
    use crate::i18n::{self, Lang};

    #[test]
    fn russian_catalog_includes_penalty_labels() {
        if !i18n::catalogs_embedded() {
            return;
        }
        let Some(ru) = Lang::from_code("ru") else {
            return;
        };
        assert_eq!(i18n::tr(ru, "Calculation"), "Расчёт");
        assert_eq!(i18n::tr(ru, "Penalty calculation"), "Расчёт пеней");
        assert_eq!(i18n::tr(ru, "Penalty\ncalculation"), "Расчёт\nпеней");
    }
}
