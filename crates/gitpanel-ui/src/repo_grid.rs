use egui::{
    Align, CornerRadius, Label, Layout, RichText, Sense, Stroke, StrokeKind, TextWrapMode, Ui,
    UiBuilder, vec2,
};
use gitpanel_core::Repository;
use gitpanel_i18n::{Translator, keys};

use crate::format::{compact_number, language_color, relative_time};
use crate::icons::{Icon, icon};
use crate::theme::Palette;
use crate::widgets::{badge, outline_badge};

pub fn repo_grid(
    ui: &mut Ui,
    repositories: &[Repository],
    translator: &Translator,
    palette: Palette,
    selected: Option<i64>,
    syncing: Option<i64>,
) -> Option<i64> {
    let available = ui.available_width().max(280.0);
    let gap = 12.0;
    let columns = (((available + gap) / 400.0).floor() as usize).max(1);
    let card_width =
        ((available - gap * columns.saturating_sub(1) as f32) / columns as f32).max(260.0);
    let mut clicked = None;
    ui.spacing_mut().item_spacing = vec2(gap, gap);
    ui.horizontal_wrapped(|ui| {
        for repository in repositories {
            if repository_card(
                ui, repository, card_width, translator, palette, selected, syncing,
            ) {
                clicked = Some(repository.github_id);
            }
        }
    });
    clicked
}

#[allow(clippy::too_many_arguments)]
fn repository_card(
    ui: &mut Ui,
    repository: &Repository,
    width: f32,
    translator: &Translator,
    palette: Palette,
    selected: Option<i64>,
    syncing: Option<i64>,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(width, 124.0), Sense::click());
    let is_selected = selected == Some(repository.github_id);
    let background = if is_selected {
        palette.selection_bg
    } else if response.hovered() {
        palette.canvas_subtle
    } else {
        palette.canvas_default
    };
    let border = if is_selected || response.hovered() {
        palette.accent_fg
    } else {
        palette.border_default
    };
    ui.painter()
        .rect_filled(rect, CornerRadius::same(8), background);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(8),
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );

    let content_rect = rect.shrink2(vec2(12.0, 10.0));
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(content_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    child.set_clip_rect(content_rect);
    child.spacing_mut().item_spacing.y = 4.0;

    child.horizontal(|ui| {
        icon(ui, Icon::Repo, 18.0, palette.fg_muted);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                ui.label(
                    RichText::new(&repository.owner)
                        .size(11.5)
                        .color(palette.fg_muted),
                );
                ui.label(RichText::new("/").size(11.5).color(palette.fg_subtle));
                ui.label(
                    RichText::new(&repository.name)
                        .size(13.0)
                        .strong()
                        .color(palette.fg_default),
                );
            });
            if let Some(description) = &repository.description {
                ui.add(
                    Label::new(
                        RichText::new(description)
                            .size(11.0)
                            .color(palette.fg_muted),
                    )
                    .wrap_mode(TextWrapMode::Truncate),
                );
            } else {
                ui.label(
                    RichText::new(translator.text(keys::REPOS_DETAIL_NO_DESCRIPTION))
                        .size(11.0)
                        .color(palette.fg_subtle),
                );
            }
        });
    });

    child.add_space(2.0);
    child.horizontal(|ui| {
        if let Some(language) = &repository.language {
            ui.label(RichText::new("●").size(9.0).color(language_color(language)));
            ui.label(RichText::new(language).size(11.0).color(palette.fg_muted));
        } else {
            ui.label(RichText::new("—").size(11.0).color(palette.fg_subtle));
        }
        ui.add_space(6.0);
        icon(ui, Icon::Star, 12.0, palette.fg_muted);
        ui.label(
            RichText::new(compact_number(u64::from(repository.stars)))
                .size(11.0)
                .color(palette.fg_muted),
        );
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
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            sync_badge(ui, repository, translator, palette, syncing);
            ui.label(
                RichText::new(relative_time(
                    translator,
                    repository.pushed_at.unwrap_or(repository.updated_at),
                    chrono::Utc::now(),
                ))
                .size(10.5)
                .color(palette.fg_subtle),
            );
        });
    });

    drop(child);
    response.clicked()
}

fn sync_badge(
    ui: &mut Ui,
    repository: &Repository,
    translator: &Translator,
    palette: Palette,
    syncing: Option<i64>,
) {
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
}
