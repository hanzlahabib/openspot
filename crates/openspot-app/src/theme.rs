//! Appearance: theme choice, high-contrast palette and UI scale.

use eframe::egui::{self, Color32, Stroke, Theme, ThemePreference, Visuals};
use serde::{Deserialize, Serialize};

/// UI scale presets in percent. Larger steps help low-vision users and high-DPI projectors;
/// anything above 150 % starts to crowd the table on a 1080p screen.
pub const SCALE_PRESETS_PERCENT: [u16; 4] = [100, 115, 130, 150];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemeChoice {
    /// Follow the operating system.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeChoice {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "Follow system",
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    fn preference(self) -> ThemePreference {
        match self {
            Self::System => ThemePreference::System,
            Self::Light => ThemePreference::Light,
            Self::Dark => ThemePreference::Dark,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    pub theme: ThemeChoice,
    pub high_contrast: bool,
    /// One of [`SCALE_PRESETS_PERCENT`].
    pub scale_percent: u16,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { theme: ThemeChoice::System, high_contrast: false, scale_percent: 100 }
    }
}

impl Appearance {
    /// Replaces an unsupported scale (hand-edited or from a newer version) with the nearest preset.
    pub fn sanitized(mut self) -> Self {
        self.scale_percent = *SCALE_PRESETS_PERCENT
            .iter()
            .min_by_key(|p| p.abs_diff(self.scale_percent))
            .unwrap_or(&SCALE_PRESETS_PERCENT[0]);
        self
    }

    pub fn zoom_factor(&self) -> f32 {
        f32::from(self.scale_percent) / 100.0
    }

    /// Pushes the appearance into egui. Cheap, but only call it when the value changed.
    pub fn apply(&self, ctx: &egui::Context) {
        ctx.set_theme(self.theme.preference());
        ctx.set_zoom_factor(self.zoom_factor());
        for theme in [Theme::Dark, Theme::Light] {
            let dark = theme == Theme::Dark;
            ctx.set_visuals_of(theme, if self.high_contrast { high_contrast_visuals(dark) } else if dark { Visuals::dark() } else { Visuals::light() });
        }
    }
}

/// Colours of the high-contrast theme. Every text/background pair meets WCAG 2.1 AAA (7:1),
/// which the tests below check.
struct Palette {
    background: Color32,
    text: Color32,
    selection_bg: Color32,
    selection_text: Color32,
    hover_bg: Color32,
    link: Color32,
    error: Color32,
    warning: Color32,
}

fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            background: Color32::BLACK,
            text: Color32::WHITE,
            selection_bg: Color32::from_rgb(255, 221, 0),
            selection_text: Color32::BLACK,
            hover_bg: Color32::from_rgb(48, 48, 48),
            link: Color32::from_rgb(110, 200, 255),
            error: Color32::from_rgb(255, 120, 120),
            warning: Color32::from_rgb(255, 210, 90),
        }
    } else {
        Palette {
            background: Color32::WHITE,
            text: Color32::BLACK,
            selection_bg: Color32::from_rgb(0, 40, 160),
            selection_text: Color32::WHITE,
            hover_bg: Color32::from_rgb(222, 222, 222),
            link: Color32::from_rgb(0, 40, 160),
            error: Color32::from_rgb(160, 0, 0),
            warning: Color32::from_rgb(110, 60, 0),
        }
    }
}

fn high_contrast_visuals(dark: bool) -> Visuals {
    let p = palette(dark);
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    v.override_text_color = Some(p.text);
    v.panel_fill = p.background;
    v.window_fill = p.background;
    v.extreme_bg_color = p.background;
    v.faint_bg_color = p.hover_bg;
    v.hyperlink_color = p.link;
    v.error_fg_color = p.error;
    v.warn_fg_color = p.warning;
    v.selection.bg_fill = p.selection_bg;
    v.selection.stroke = Stroke::new(1.0, p.selection_text);
    v.window_stroke = Stroke::new(1.5, p.text);

    let outline = Stroke::new(1.5, p.text);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.background;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    for state in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        state.bg_stroke = outline;
        state.fg_stroke = outline;
    }
    w.inactive.bg_fill = p.background;
    w.inactive.weak_bg_fill = p.background;
    for state in [&mut w.hovered, &mut w.open] {
        state.bg_fill = p.hover_bg;
        state.weak_bg_fill = p.hover_bg;
    }
    w.active.bg_fill = p.selection_bg;
    w.active.weak_bg_fill = p.selection_bg;
    w.active.fg_stroke = Stroke::new(1.5, p.selection_text);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG 2.1 relative luminance of an sRGB colour.
    fn luminance(c: Color32) -> f64 {
        let lin = |v: u8| {
            let s = f64::from(v) / 255.0;
            if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
    }

    /// WCAG 2.1 contrast ratio between two colours, 1.0 (identical) to 21.0 (black on white).
    fn contrast_ratio(a: Color32, b: Color32) -> f64 {
        let (la, lb) = (luminance(a), luminance(b));
        (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
    }

    /// AAA for normal text.
    const AAA: f64 = 7.0;
    /// WCAG 1.4.11 minimum for UI component boundaries.
    const NON_TEXT_MIN: f64 = 3.0;

    #[test]
    fn contrast_ratio_matches_published_values() {
        assert!((contrast_ratio(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 1e-9);
        // #767676 on white is the well-known 4.54:1 AA threshold example.
        let grey = Color32::from_rgb(0x76, 0x76, 0x76);
        assert!((contrast_ratio(grey, Color32::WHITE) - 4.54).abs() < 0.01);
    }

    #[test]
    fn high_contrast_text_pairs_meet_aaa_in_both_themes() {
        for dark in [true, false] {
            let p = palette(dark);
            let v = high_contrast_visuals(dark);
            let text = v.override_text_color.unwrap();
            let pairs = [
                ("text on panel", text, v.panel_fill),
                ("text on window", text, v.window_fill),
                ("text on faint rows", text, v.faint_bg_color),
                ("text on hover", text, v.widgets.hovered.weak_bg_fill),
                ("selected text", v.selection.stroke.color, v.selection.bg_fill),
                ("pressed widget", v.widgets.active.fg_stroke.color, v.widgets.active.weak_bg_fill),
                ("link", v.hyperlink_color, v.panel_fill),
                ("error", v.error_fg_color, v.panel_fill),
                ("warning", v.warn_fg_color, v.panel_fill),
                ("hover palette", p.text, p.hover_bg),
            ];
            for (name, fg, bg) in pairs {
                let ratio = contrast_ratio(fg, bg);
                assert!(ratio >= AAA, "{name} (dark={dark}) is only {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn high_contrast_widget_outlines_are_visible_against_the_background() {
        for dark in [true, false] {
            let v = high_contrast_visuals(dark);
            for (name, w) in [("inactive", &v.widgets.inactive), ("hovered", &v.widgets.hovered), ("active", &v.widgets.active)] {
                let ratio = contrast_ratio(w.bg_stroke.color, v.panel_fill);
                assert!(ratio >= NON_TEXT_MIN, "{name} outline (dark={dark}) is only {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn scale_is_snapped_to_a_preset_and_converted_to_a_zoom_factor() {
        let a = Appearance { scale_percent: 112, ..Appearance::default() }.sanitized();
        assert_eq!(a.scale_percent, 115);
        assert_eq!(Appearance { scale_percent: 5000, ..Appearance::default() }.sanitized().scale_percent, 150);
        assert_eq!(Appearance { scale_percent: 0, ..Appearance::default() }.sanitized().scale_percent, 100);
        assert!((Appearance { scale_percent: 130, ..Appearance::default() }.zoom_factor() - 1.3).abs() < 1e-6);
    }

    #[test]
    fn appearance_loads_from_partial_json_and_round_trips() {
        let a: Appearance = serde_json::from_str(r#"{"theme":"dark"}"#).unwrap();
        assert_eq!((a.theme, a.high_contrast, a.scale_percent), (ThemeChoice::Dark, false, 100));
        let full = Appearance { theme: ThemeChoice::Light, high_contrast: true, scale_percent: 150 };
        assert_eq!(serde_json::from_str::<Appearance>(&serde_json::to_string(&full).unwrap()).unwrap(), full);
    }
}
