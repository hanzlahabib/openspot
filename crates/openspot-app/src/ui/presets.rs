//! Shared "pick one of these durations" control used by the Inspector and the preferences window.

use eframe::egui;

/// "Never" for 0, otherwise the largest whole unit: "30 seconds", "1 minute", "24 hours".
pub fn duration_label(secs: u64) -> String {
    let unit = |n: u64, name: &str| format!("{n} {name}{}", if n == 1 { "" } else { "s" });
    match secs {
        0 => "Never".to_owned(),
        s if s % 3600 == 0 => unit(s / 3600, "hour"),
        s if s % 60 == 0 => unit(s / 60, "minute"),
        s => unit(s, "second"),
    }
}

/// Combo box over `presets`; returns true when the user picked a different value.
pub fn duration_combo(ui: &mut egui::Ui, id: &str, value: &mut u64, presets: &[u64]) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id).selected_text(duration_label(*value)).show_ui(ui, |ui| {
        for &p in presets {
            changed |= ui.selectable_value(value, p, duration_label(p)).changed();
        }
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::duration_label;

    #[test]
    fn labels_use_the_largest_exact_unit() {
        assert_eq!(duration_label(0), "Never");
        assert_eq!(duration_label(1), "1 second");
        assert_eq!(duration_label(30), "30 seconds");
        assert_eq!(duration_label(60), "1 minute");
        assert_eq!(duration_label(900), "15 minutes");
        assert_eq!(duration_label(3600), "1 hour");
        assert_eq!(duration_label(86_400), "24 hours");
        assert_eq!(duration_label(90), "90 seconds");
    }
}
