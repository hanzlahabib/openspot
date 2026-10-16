use eframe::egui::{self, Align2, Vec2};
use openspot_wlan::MacAddress;

use super::dialog::escape_pressed;
use crate::aliases::Aliases;

/// Modal-style window to edit one alias. `edit` holds the BSSID and the text being typed;
/// it is cleared when the user saves or cancels. Blank text removes the alias.
pub fn dialog(ctx: &egui::Context, aliases: &mut Aliases, edit: &mut Option<(MacAddress, String)>) {
    let Some((bssid, text)) = edit.as_mut() else { return };
    let (mut save, mut cancel) = (false, escape_pressed(ctx));
    egui::Window::new(format!("Alias for {bssid}"))
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
        .show(ctx, |ui| {
            let field = ui.add(egui::TextEdit::singleline(text).hint_text("Friendly name").desired_width(260.0));
            field.request_focus();
            let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            ui.horizontal(|ui| {
                save = ui.button("Save").clicked() || enter;
                cancel |= ui.button("Cancel").clicked();
                if ui.button("Remove alias").clicked() {
                    text.clear();
                    save = true;
                }
            });
        });
    if save {
        aliases.set(bssid, text);
        *edit = None;
    } else if cancel {
        *edit = None;
    }
}
