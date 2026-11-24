use std::collections::HashMap;

use egui::{Align, Layout, Sense, Ui};
use egui_extras::{Column, TableBuilder};
use gitpanel_core::Repository;
use gitpanel_i18n::{Translator, keys};

use crate::format::{compact_number, language_color, relative_time};
use crate::icons::{Icon, icon};
use crate::theme::Palette;
use crate::widgets::{badge, outline_badge};

pub fn repo_table(
    ui: &mut Ui,
    repositories: &[Repository],
    translator: &Translator,
    palette: Palette,
    selected: Option<i64>,
    syncing: Option<i64>,
    avatars: &HashMap<String, egui::TextureHandle>,
) -> Option<i64> {
    let mut clicked = None;
    TableBuilder::new(ui)
        .id_salt("repositories")
        .striped(true)
        .resizable(true)
        .sense(Sense::click())
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(Column::initial(300.0).at_least(240.0))
        .column(Column::initial(90.0).at_least(80.0))
        .column(Column::initial(105.0).at_least(80.0))
        .column(Column::initial(65.0).at_least(55.0))
        .column(Column::initial(105.0).at_least(90.0))
        .column(Column::remainder().at_least(130.0))
        .header(30.0, |mut header| {
            header.col(|ui| {
                ui.label(
                    egui::RichText::new(translator.text(keys::COMMON_REPOSITORIES))
                        .size(11.0)
                        .strong()
                        .color(palette.fg_muted),
                );
            });
            header.col(|ui| {
                ui.label(
                    egui::RichText::new(translator.text(keys::COMMON_VISIBILITY))
                        .size(11.0)
                        .strong()
                        .color(palette.fg_muted),
                );
            });
            header.col(|ui| {
                ui.label(
                    egui::RichText::new(translator.text(keys::COMMON_LANGUAGE))
                        .size(11.0)
                        .strong()
                        .color(palette.fg_muted),
                );
            });
            header.col(|ui| {
                ui.label(
                    egui::RichText::new(translator.text(keys::COMMON_STARS))
                        .size(11.0)
                        .strong()
                        .color(palette.fg_muted),
                );
            });
            header.col(|ui| {
                ui.label(
                    egui::RichText::new(translator.text(keys::COMMON_UPDATED))
                        .size(11.0)
                        .strong()
                        .color(palette.fg_muted),
                );
            });
            header.col(|ui| {
                ui.label(
                    egui::RichText::new(translator.text(keys::COMMON_STATUS))
                        .size(11.0)
                        .strong()
                        .color(palette.fg_muted),
                );
            });
        })
        .body(|body| {
            body.rows(44.0, repositories.len(), |mut row| {
                let index = row.index();
                let repository = &repositories[index];
                row.set_selected(selected == Some(repository.github_id));
                row.col(|ui| {
                    ui.horizontal(|ui| {
                        if let Some(texture) = avatars.get(&repository.owner) {
                            ui.add(
                                egui::Image::from_texture(texture)
                                    .fit_to_exact_size(egui::vec2(18.0, 18.0))
                                    .corner_radius(9.0),
                            );
                        } else {
                            icon(ui, Icon::Repo, 17.0, palette.fg_muted);
                        }
                        ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 3.0;
                                ui.label(
                                    egui::RichText::new(&repository.owner)
                                        .size(12.0)
                                        .color(palette.fg_muted),
                                );
                                ui.label(
                                    egui::RichText::new("/").size(12.0).color(palette.fg_subtle),
                                );
                                ui.label(
                                    egui::RichText::new(&repository.name)
                                        .size(13.0)
                                        .strong()
                                        .color(palette.fg_default),
                                );
                            });
                            if let Some(description) = &repository.description {
                                ui.label(
                                    egui::RichText::new(description)
                                        .size(11.0)
                                        .color(palette.fg_muted),
                                );
                            }
                        });
                    });
                });
                row.col(|ui| {
                    if repository.private {
                        badge(
                            ui,
                            translator.text(keys::COMMON_PRIVATE),
                            palette.canvas_inset,
                            palette.fg_muted,
                        );
                    } else {
                        outline_badge(ui, translator.text(keys::COMMON_PUBLIC), palette);
                    }
                });
                row.col(|ui| {
                    if let Some(language) = &repository.language {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new("●")
                                    .size(10.0)
                                    .color(language_color(language)),
                            );
                            ui.label(
                                egui::RichText::new(language)
                                    .size(12.0)
                                    .color(palette.fg_default),
                            );
                        });
                    } else {
                        ui.label(egui::RichText::new("—").size(12.0).color(palette.fg_subtle));
                    }
                });
                row.col(|ui| {
                    ui.horizontal(|ui| {
                        icon(ui, Icon::Star, 14.0, palette.fg_muted);
                        ui.label(
                            egui::RichText::new(compact_number(u64::from(repository.stars)))
                                .size(12.0)
                                .color(palette.fg_default),
                        );
                    });
                });
                row.col(|ui| {
                    ui.label(
                        egui::RichText::new(relative_time(
                            translator,
                            repository.pushed_at.unwrap_or(repository.updated_at),
                            chrono::Utc::now(),
                        ))
                        .size(12.0)
                        .color(palette.fg_muted),
                    );
                });
                row.col(|ui| {
                    if syncing == Some(repository.github_id) {
                        badge(
                            ui,
                            translator.text(keys::REPOS_SYNC_STATUS_SYNCING),
                            palette.selection_bg,
                            palette.accent_fg,
                        );
                    } else if repository.last_sync_error.is_some() {
                        badge(
                            ui,
                            translator.text(keys::REPOS_SYNC_STATUS_ERROR),
                            palette.diff_del_bg,
                            palette.danger_fg,
                        );
                    } else if repository.synced_at.is_some() {
                        badge(
                            ui,
                            translator.text(keys::REPOS_SYNC_STATUS_SYNCED),
                            palette.diff_add_bg,
                            palette.success_fg,
                        );
                    } else {
                        outline_badge(
                            ui,
                            translator.text(keys::REPOS_SYNC_STATUS_PENDING),
                            palette,
                        );
                    }
                });
                if row.response().clicked() {
                    clicked = Some(repository.github_id);
                }
            });
        });
    clicked
}
