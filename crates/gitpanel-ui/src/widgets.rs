use egui::{Color32, CornerRadius, Frame, Margin, Response, RichText, Stroke, Ui, Vec2, vec2};
use gitpanel_i18n::{Translator, keys};

use crate::format::compact_number;
use crate::icons::{Icon, icon};
use crate::theme::Palette;

pub fn badge(
    ui: &mut Ui,
    text: impl Into<String>,
    background: Color32,
    foreground: Color32,
) -> Response {
    Frame::NONE
        .fill(background)
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text.into())
                    .size(11.5)
                    .color(foreground)
                    .strong(),
            );
        })
        .response
}

pub fn outline_badge(ui: &mut Ui, text: impl Into<String>, palette: Palette) -> Response {
    Frame::NONE
        .fill(palette.canvas_subtle)
        .stroke(Stroke::new(1.0, palette.border_default))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text.into())
                    .size(11.5)
                    .color(palette.fg_muted),
            );
        })
        .response
}

pub fn card<R>(ui: &mut Ui, palette: Palette, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::NONE
        .fill(palette.canvas_default)
        .stroke(Stroke::new(1.0, palette.border_default))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(16))
        .show(ui, add_contents)
        .inner
}

pub fn panel_frame(palette: Palette) -> Frame {
    Frame::NONE
        .fill(palette.canvas_default)
        .inner_margin(Margin::same(20))
}

pub fn section_header(ui: &mut Ui, title: &str, subtitle: Option<&str>, palette: Palette) {
    ui.label(
        RichText::new(title)
            .size(20.0)
            .strong()
            .color(palette.fg_default),
    );
    if let Some(subtitle) = subtitle {
        ui.label(RichText::new(subtitle).size(13.0).color(palette.fg_muted));
    }
    ui.add_space(8.0);
}

pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str, palette: Palette) {
    ui.horizontal_wrapped(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(title)
                    .size(24.0)
                    .strong()
                    .color(palette.fg_default),
            );
            ui.label(RichText::new(subtitle).size(13.0).color(palette.fg_muted));
        });
    });
    ui.add_space(14.0);
}

pub fn tabs(ui: &mut Ui, labels: &[&str], selected: &mut usize, palette: Palette) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (index, label) in labels.iter().enumerate() {
            let is_selected = index == *selected;
            let response = ui.add(
                egui::Button::new(
                    RichText::new(*label)
                        .size(13.5)
                        .color(if is_selected {
                            palette.fg_default
                        } else {
                            palette.fg_muted
                        })
                        .strong(),
                )
                .frame(false)
                .min_size(vec2(0.0, 34.0)),
            );
            if is_selected {
                ui.painter().line_segment(
                    [
                        egui::Pos2::new(response.rect.left(), response.rect.bottom() - 2.0),
                        egui::Pos2::new(response.rect.right(), response.rect.bottom() - 2.0),
                    ],
                    Stroke::new(2.0, palette.accent_fg),
                );
            }
            if response.clicked() {
                *selected = index;
                changed = true;
            }
        }
    });
    ui.separator();
    changed
}

pub fn empty_state(
    ui: &mut Ui,
    title: &str,
    body: &str,
    palette: Palette,
    icon_kind: Option<Icon>,
) {
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        if let Some(icon_kind) = icon_kind {
            icon(ui, icon_kind, 34.0, palette.fg_subtle);
            ui.add_space(8.0);
        }
        ui.label(
            RichText::new(title)
                .size(17.0)
                .strong()
                .color(palette.fg_default),
        );
        ui.label(RichText::new(body).size(13.0).color(palette.fg_muted));
        ui.add_space(16.0);
    });
}

pub fn error_banner(ui: &mut Ui, title: &str, detail: &str, palette: Palette) {
    Frame::NONE
        .fill(palette.diff_del_bg)
        .stroke(Stroke::new(1.0, palette.danger_fg))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                icon(ui, Icon::Warning, 18.0, palette.danger_fg);
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).strong().color(palette.danger_fg));
                    if !detail.is_empty() {
                        ui.label(RichText::new(detail).size(12.0).color(palette.fg_muted));
                    }
                });
            });
        });
}

pub fn info_banner(ui: &mut Ui, text: &str, palette: Palette) {
    Frame::NONE
        .fill(palette.canvas_subtle)
        .stroke(Stroke::new(1.0, palette.border_default))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(12.0).color(palette.fg_muted));
        });
}

pub fn stat_card(
    ui: &mut Ui,
    label: &str,
    value: impl Into<String>,
    hint: Option<&str>,
    palette: Palette,
) {
    Frame::NONE
        .fill(palette.canvas_subtle)
        .stroke(Stroke::new(1.0, palette.border_default))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(11.0).color(palette.fg_muted));
            ui.label(
                RichText::new(value.into())
                    .size(20.0)
                    .strong()
                    .color(palette.fg_default),
            );
            if let Some(hint) = hint {
                ui.label(RichText::new(hint).size(11.0).color(palette.fg_subtle));
            }
        });
}

pub fn repo_stats(
    ui: &mut Ui,
    translator: &Translator,
    commits: u32,
    additions: u64,
    deletions: u64,
    palette: Palette,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        stat_card(
            ui,
            &translator.text(keys::CHART_TOTAL_COMMITS),
            compact_number(u64::from(commits)),
            None,
            palette,
        );
        stat_card(
            ui,
            &translator.text(keys::COMMON_ADDITIONS),
            compact_number(additions),
            None,
            palette,
        );
        stat_card(
            ui,
            &translator.text(keys::COMMON_DELETIONS),
            compact_number(deletions),
            None,
            palette,
        );
    });
}

pub fn kv_row(ui: &mut Ui, key: &str, value: &str, palette: Palette) {
    ui.horizontal(|ui| {
        ui.add_sized(
            Vec2::new(150.0, 20.0),
            egui::Label::new(RichText::new(key).size(12.0).color(palette.fg_muted)),
        );
        ui.label(RichText::new(value).size(12.0).color(palette.fg_default));
    });
}

pub fn search_input(ui: &mut Ui, value: &mut String, hint: &str, palette: Palette) -> Response {
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(220.0)
            .text_color(palette.fg_default)
            .background_color(palette.canvas_default)
            .margin(Margin::symmetric(8, 6)),
    )
}

pub fn loading(ui: &mut Ui, label: &str, palette: Palette) {
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label(RichText::new(label).size(12.5).color(palette.fg_muted));
    });
}

pub fn muted(ui: &mut Ui, text: impl Into<String>, palette: Palette) {
    ui.label(
        RichText::new(text.into())
            .size(12.0)
            .color(palette.fg_muted),
    );
}
