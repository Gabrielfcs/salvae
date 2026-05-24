//! A shadcn-inspired dark "zinc" theme for egui: neutral palette, subtle 1px
//! borders, rounded corners, airy spacing, and a clear type hierarchy.

use eframe::egui::{
    self, Color32, FontFamily, FontId, Margin, Rounding, Stroke, TextStyle, Visuals,
};

// Zinc palette (Tailwind zinc), the shadcn default neutral.
const ZINC_950: Color32 = Color32::from_rgb(9, 9, 11); // app background
const ZINC_900: Color32 = Color32::from_rgb(24, 24, 27); // cards / inputs
const ZINC_800: Color32 = Color32::from_rgb(39, 39, 42); // borders / hover
const ZINC_700: Color32 = Color32::from_rgb(63, 63, 70); // active / strong border
const ZINC_400: Color32 = Color32::from_rgb(161, 161, 170); // muted foreground
const ZINC_50: Color32 = Color32::from_rgb(250, 250, 250); // foreground
/// Accent used sparingly for selection (a restrained blue-500).
const ACCENT: Color32 = Color32::from_rgb(59, 130, 246);

/// Muted secondary-text colour (e.g. ids, hints).
pub const MUTED: Color32 = ZINC_400;

/// Apply the theme to the whole context. Call once at startup.
pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();

    let rounding = Rounding::same(8.0);
    let widget_rounding = Rounding::same(6.0);
    let border = Stroke::new(1.0, ZINC_800);

    let mut visuals = Visuals::dark();
    visuals.dark_mode = true;
    visuals.override_text_color = Some(ZINC_50);
    visuals.panel_fill = ZINC_950;
    visuals.window_fill = ZINC_950;
    visuals.window_stroke = border;
    visuals.window_rounding = rounding;
    visuals.faint_bg_color = ZINC_900;
    visuals.extreme_bg_color = ZINC_900; // text-edit background
    visuals.hyperlink_color = ACCENT;
    visuals.selection = egui::style::Selection {
        bg_fill: ACCENT.linear_multiply(0.35),
        stroke: Stroke::new(1.0, ACCENT),
    };

    // Widget states: neutral fills, subtle borders, white-ish text.
    let w = &mut visuals.widgets;
    w.noninteractive.bg_fill = ZINC_950;
    w.noninteractive.weak_bg_fill = ZINC_950;
    w.noninteractive.bg_stroke = border;
    w.noninteractive.fg_stroke = Stroke::new(1.0, ZINC_400);
    w.noninteractive.rounding = widget_rounding;

    w.inactive.bg_fill = ZINC_900;
    w.inactive.weak_bg_fill = ZINC_900;
    w.inactive.bg_stroke = border;
    w.inactive.fg_stroke = Stroke::new(1.0, ZINC_50);
    w.inactive.rounding = widget_rounding;

    w.hovered.bg_fill = ZINC_800;
    w.hovered.weak_bg_fill = ZINC_800;
    w.hovered.bg_stroke = Stroke::new(1.0, ZINC_700);
    w.hovered.fg_stroke = Stroke::new(1.0, ZINC_50);
    w.hovered.rounding = widget_rounding;

    w.active.bg_fill = ZINC_700;
    w.active.weak_bg_fill = ZINC_700;
    w.active.bg_stroke = Stroke::new(1.0, ZINC_700);
    w.active.fg_stroke = Stroke::new(1.0, ZINC_50);
    w.active.rounding = widget_rounding;

    w.open.bg_fill = ZINC_900;
    w.open.weak_bg_fill = ZINC_900;
    w.open.bg_stroke = border;
    w.open.fg_stroke = Stroke::new(1.0, ZINC_50);
    w.open.rounding = widget_rounding;

    style.visuals = visuals;

    // Airy spacing.
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.menu_margin = Margin::same(6.0);
    style.spacing.window_margin = Margin::same(12.0);
    style.spacing.indent = 16.0;
    style.spacing.interact_size.y = 28.0;

    // Type hierarchy (~14px body, à la shadcn).
    use FontFamily::{Monospace, Proportional};
    style.text_styles = [
        (TextStyle::Heading, FontId::new(20.0, Proportional)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Small, FontId::new(12.0, Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, Monospace)),
    ]
    .into();

    // Pin our dark theme regardless of the OS theme. egui 0.29 follows the
    // system theme by default and re-applies the matching `Style` each frame —
    // on a machine in Windows "light mode" that would overwrite our visuals
    // (light panels + dark cards = unreadable). Pinning Dark and registering
    // our style as *both* the dark and light style keeps it consistent for
    // everyone.
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_style_of(egui::Theme::Dark, style.clone());
    ctx.set_style_of(egui::Theme::Light, style.clone());
    ctx.set_style(style);
}

/// A shadcn-style "card": rounded, 1px border, padded, on the faint surface.
pub fn card_frame() -> egui::Frame {
    egui::Frame::none()
        .fill(ZINC_900)
        .stroke(Stroke::new(1.0, ZINC_800))
        .rounding(Rounding::same(8.0))
        .inner_margin(Margin::same(12.0))
}

/// A filled accent button for primary actions.
pub fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(label).color(ZINC_50))
            .fill(ACCENT)
            .rounding(Rounding::same(6.0)),
    )
}

/// A square, icon-only accent button with rounded corners.
pub fn primary_icon_button(ui: &mut egui::Ui, icon: egui::Image<'_>) -> egui::Response {
    ui.add(
        egui::Button::image(icon)
            .min_size(egui::vec2(44.0, 44.0))
            .fill(ACCENT)
            .rounding(Rounding::same(8.0)),
    )
}

/// The accent colour (e.g. for tinting link/affordance icons).
pub const fn accent() -> egui::Color32 {
    ACCENT
}
