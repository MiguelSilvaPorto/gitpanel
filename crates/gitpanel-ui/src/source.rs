use egui::{CornerRadius, Frame, Margin, RichText, Ui};
use gitpanel_i18n::{Translator, keys};

use crate::icons::{Icon, icon};
use crate::theme::Palette;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataSource {
    Remote,
    Local,
    Loading,
}

pub fn source_badge(ui: &mut Ui, source: DataSource, translator: &Translator, palette: Palette) {
    Frame::NONE
        .fill(palette.canvas_subtle)
        .stroke(egui::Stroke::new(1.0, palette.border_default))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                match source {
                    DataSource::Remote => {
                        icon(ui, Icon::Cloud, 13.0, palette.fg_muted);
                        ui.label(
                            RichText::new(translator.text(keys::COMMON_REMOTE))
                                .size(10.5)
                                .color(palette.fg_muted),
                        );
                    }
                    DataSource::Local => {
                        icon(ui, Icon::Local, 13.0, palette.success_fg);
                        ui.label(
                            RichText::new(translator.text(keys::COMMON_LOCAL))
                                .size(10.5)
                                .color(palette.success_fg),
                        );
                    }
                    DataSource::Loading => {
                        icon(ui, Icon::Cloud, 13.0, palette.accent_fg);
                        ui.label(
                            RichText::new(translator.text(keys::COMMON_LOADING))
                                .size(10.5)
                                .color(palette.accent_fg),
                        );
                    }
                }
            });
        });
}
