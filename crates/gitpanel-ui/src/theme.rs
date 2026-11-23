use std::collections::BTreeMap;
use std::sync::Arc;

use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Visuals, vec2,
};
use gitpanel_config::ThemePreference;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub canvas_default: Color32,
    pub canvas_subtle: Color32,
    pub canvas_inset: Color32,
    pub border_default: Color32,
    pub border_muted: Color32,
    pub fg_default: Color32,
    pub fg_muted: Color32,
    pub fg_subtle: Color32,
    pub accent_fg: Color32,
    pub accent_emphasis: Color32,
    pub success_fg: Color32,
    pub danger_fg: Color32,
    pub attention_fg: Color32,
    pub done_fg: Color32,
    pub btn_primary_bg: Color32,
    pub btn_primary_hover: Color32,
    pub btn_primary_text: Color32,
    pub neutral_bg: Color32,
    pub neutral_hover: Color32,
    pub neutral_border: Color32,
    pub selection_bg: Color32,
    pub diff_add_bg: Color32,
    pub diff_add_line: Color32,
    pub diff_del_bg: Color32,
    pub diff_del_line: Color32,
    pub code_bg: Color32,
}

impl Palette {
    pub const LIGHT: Self = Self {
        canvas_default: Color32::from_rgb(255, 255, 255),
        canvas_subtle: Color32::from_rgb(246, 248, 250),
        canvas_inset: Color32::from_rgb(234, 238, 242),
        border_default: Color32::from_rgb(208, 215, 222),
        border_muted: Color32::from_rgb(234, 238, 242),
        fg_default: Color32::from_rgb(31, 35, 40),
        fg_muted: Color32::from_rgb(101, 109, 118),
        fg_subtle: Color32::from_rgb(139, 148, 158),
        accent_fg: Color32::from_rgb(9, 105, 218),
        accent_emphasis: Color32::from_rgb(9, 105, 218),
        success_fg: Color32::from_rgb(26, 127, 55),
        danger_fg: Color32::from_rgb(207, 34, 46),
        attention_fg: Color32::from_rgb(154, 103, 0),
        done_fg: Color32::from_rgb(130, 80, 223),
        btn_primary_bg: Color32::from_rgb(31, 136, 61),
        btn_primary_hover: Color32::from_rgb(26, 127, 55),
        btn_primary_text: Color32::WHITE,
        neutral_bg: Color32::from_rgb(246, 248, 250),
        neutral_hover: Color32::from_rgb(243, 244, 246),
        neutral_border: Color32::from_rgb(208, 215, 222),
        selection_bg: Color32::from_rgb(221, 244, 255),
        diff_add_bg: Color32::from_rgb(230, 255, 236),
        diff_add_line: Color32::from_rgb(204, 255, 216),
        diff_del_bg: Color32::from_rgb(255, 235, 233),
        diff_del_line: Color32::from_rgb(255, 215, 213),
        code_bg: Color32::from_rgb(246, 248, 250),
    };

    pub const DARK: Self = Self {
        canvas_default: Color32::from_rgb(13, 17, 23),
        canvas_subtle: Color32::from_rgb(22, 27, 34),
        canvas_inset: Color32::from_rgb(1, 4, 9),
        border_default: Color32::from_rgb(48, 54, 61),
        border_muted: Color32::from_rgb(33, 38, 45),
        fg_default: Color32::from_rgb(230, 237, 243),
        fg_muted: Color32::from_rgb(139, 148, 158),
        fg_subtle: Color32::from_rgb(110, 118, 129),
        accent_fg: Color32::from_rgb(47, 129, 247),
        accent_emphasis: Color32::from_rgb(31, 111, 235),
        success_fg: Color32::from_rgb(63, 185, 80),
        danger_fg: Color32::from_rgb(248, 81, 73),
        attention_fg: Color32::from_rgb(210, 153, 34),
        done_fg: Color32::from_rgb(163, 113, 247),
        btn_primary_bg: Color32::from_rgb(35, 134, 54),
        btn_primary_hover: Color32::from_rgb(46, 160, 67),
        btn_primary_text: Color32::WHITE,
        neutral_bg: Color32::from_rgb(33, 38, 45),
        neutral_hover: Color32::from_rgb(48, 54, 61),
        neutral_border: Color32::from_rgb(48, 54, 61),
        selection_bg: Color32::from_rgb(18, 45, 75),
        diff_add_bg: Color32::from_rgb(18, 38, 30),
        diff_add_line: Color32::from_rgb(3, 58, 22),
        diff_del_bg: Color32::from_rgb(37, 23, 28),
        diff_del_line: Color32::from_rgb(76, 29, 36),
        code_bg: Color32::from_rgb(22, 27, 34),
    };
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub mode: ThemePreference,
    pub dark: bool,
    pub palette: Palette,
}

impl Theme {
    pub fn resolve(mode: ThemePreference, system_dark: Option<bool>) -> Self {
        let dark = match mode {
            ThemePreference::Light => false,
            ThemePreference::Dark => true,
            ThemePreference::System => system_dark.unwrap_or(true),
        };
        Self {
            mode,
            dark,
            palette: if dark { Palette::DARK } else { Palette::LIGHT },
        }
    }

    pub fn apply(&self, ctx: &egui::Context) {
        let mut visuals = if self.dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        visuals.dark_mode = self.dark;
        visuals.panel_fill = self.palette.canvas_default;
        visuals.window_fill = self.palette.canvas_default;
        visuals.extreme_bg_color = self.palette.canvas_subtle;
        visuals.faint_bg_color = self.palette.canvas_subtle;
        visuals.code_bg_color = self.palette.code_bg;
        visuals.override_text_color = Some(self.palette.fg_default);
        visuals.hyperlink_color = self.palette.accent_fg;
        visuals.warn_fg_color = self.palette.attention_fg;
        visuals.error_fg_color = self.palette.danger_fg;
        visuals.selection.bg_fill = self.palette.selection_bg;
        visuals.selection.stroke = Stroke::new(1.0, self.palette.accent_fg);
        visuals.window_corner_radius = CornerRadius::same(10);
        visuals.menu_corner_radius = CornerRadius::same(8);
        visuals.window_stroke = Stroke::new(1.0, self.palette.border_default);
        visuals.widgets.noninteractive.bg_fill = self.palette.canvas_default;
        visuals.widgets.noninteractive.weak_bg_fill = self.palette.canvas_subtle;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.palette.border_default);
        visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.palette.fg_default);
        visuals.widgets.inactive.bg_fill = self.palette.neutral_bg;
        visuals.widgets.inactive.weak_bg_fill = self.palette.neutral_bg;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, self.palette.neutral_border);
        visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, self.palette.fg_default);
        visuals.widgets.hovered.bg_fill = self.palette.neutral_hover;
        visuals.widgets.hovered.weak_bg_fill = self.palette.neutral_hover;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, self.palette.border_default);
        visuals.widgets.hovered.fg_stroke = Stroke::new(1.0, self.palette.fg_default);
        visuals.widgets.active.bg_fill = self.palette.neutral_hover;
        visuals.widgets.active.weak_bg_fill = self.palette.neutral_hover;
        visuals.widgets.active.bg_stroke = Stroke::new(1.0, self.palette.accent_fg);
        visuals.widgets.active.fg_stroke = Stroke::new(1.0, self.palette.fg_default);
        visuals.widgets.open.bg_fill = self.palette.neutral_bg;
        visuals.widgets.open.weak_bg_fill = self.palette.neutral_bg;
        visuals.widgets.open.bg_stroke = Stroke::new(1.0, self.palette.border_default);
        visuals.widgets.open.fg_stroke = Stroke::new(1.0, self.palette.fg_default);
        visuals.button_frame = true;
        visuals.striped = true;

        let egui_theme = if self.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        ctx.set_theme(if self.dark {
            egui::ThemePreference::Dark
        } else {
            egui::ThemePreference::Light
        });
        let mut style = (*ctx.style_of(egui_theme)).clone();
        style.visuals = visuals;
        style.spacing.item_spacing = vec2(8.0, 6.0);
        style.spacing.button_padding = vec2(12.0, 6.0);
        style.spacing.indent = 16.0;
        style.text_styles = BTreeMap::from([
            (TextStyle::Small, FontId::proportional(11.5)),
            (TextStyle::Body, FontId::proportional(13.5)),
            (TextStyle::Button, FontId::proportional(13.5)),
            (TextStyle::Heading, FontId::proportional(20.0)),
            (TextStyle::Monospace, FontId::monospace(12.0)),
        ]);
        ctx.set_style_of(egui_theme, style);
    }
}

pub fn install_fonts(ctx: &egui::Context) {
    const INTER: &[u8] = include_bytes!("../../../assets/fonts/InterVariable.ttf");
    const JETBRAINS: &[u8] = include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf");

    let mut fonts = FontDefinitions::default();
    fonts
        .font_data
        .insert("inter".to_owned(), Arc::new(FontData::from_static(INTER)));
    fonts.font_data.insert(
        "jetbrains-mono".to_owned(),
        Arc::new(FontData::from_static(JETBRAINS)),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".to_owned());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "jetbrains-mono".to_owned());
    ctx.set_fonts(fonts);
}

pub fn primary_button(
    text: impl Into<egui::WidgetText>,
    palette: Palette,
) -> egui::Button<'static> {
    egui::Button::new(text)
        .fill(palette.btn_primary_bg)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(6))
        .min_size(vec2(0.0, 32.0))
}

pub fn secondary_button(
    text: impl Into<egui::WidgetText>,
    palette: Palette,
) -> egui::Button<'static> {
    egui::Button::new(text)
        .fill(palette.neutral_bg)
        .stroke(Stroke::new(1.0, palette.neutral_border))
        .corner_radius(CornerRadius::same(6))
        .min_size(vec2(0.0, 32.0))
}
