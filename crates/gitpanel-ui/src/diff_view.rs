use egui::{
    Align, Color32, FontId, Frame, Label, Layout, Margin, RichText, ScrollArea, TextFormat,
    TextWrapMode, Ui, vec2,
};
use gitpanel_diff::{
    Diff, DiffFile, DiffLine, DiffLineKind, FileStatus, HighlightSpan, Highlighter,
};
use gitpanel_i18n::{Translator, keys};

use crate::theme::Palette;
use crate::widgets::{badge, empty_state, info_banner};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffMode {
    Unified,
    SideBySide,
}

#[derive(Clone, Copy, Debug)]
pub struct DiffViewState {
    pub mode: DiffMode,
    pub selected_file: usize,
}

impl Default for DiffViewState {
    fn default() -> Self {
        Self {
            mode: DiffMode::Unified,
            selected_file: 0,
        }
    }
}

pub fn diff_view(
    ui: &mut Ui,
    diff: &Diff,
    translator: &Translator,
    palette: Palette,
    highlighter: &Highlighter,
    state: &mut DiffViewState,
) {
    if diff.files.is_empty() {
        empty_state(
            ui,
            &translator.text(keys::DIFF_EMPTY_TITLE),
            &translator.text(keys::DIFF_EMPTY_BODY),
            palette,
            None,
        );
        return;
    }
    if state.selected_file >= diff.files.len() {
        state.selected_file = 0;
    }

    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            vec2(245.0, ui.available_height().max(200.0)),
            Layout::top_down(Align::Min),
            |ui| file_tree(ui, diff, translator, palette, state),
        );
        ui.separator();
        ui.vertical(|ui| {
            let file = &diff.files[state.selected_file];
            file_header(ui, file, translator, palette, state);
            if file.binary {
                info_banner(ui, &translator.text(keys::DIFF_BINARY), palette);
                return;
            }
            if file.truncated {
                info_banner(ui, &translator.text(keys::DIFF_TOO_LARGE), palette);
            }
            if file.hunks.is_empty() {
                empty_state(
                    ui,
                    &translator.text(keys::DIFF_NO_CHANGES),
                    &translator.text(keys::DIFF_EMPTY_BODY),
                    palette,
                    None,
                );
                return;
            }
            if file.line_count() > 20_000 {
                info_banner(ui, &translator.text(keys::DIFF_TOO_LARGE), palette);
                return;
            }
            let highlights = highlighter.highlight_file(file);
            match state.mode {
                DiffMode::Unified => {
                    render_unified(ui, file, &highlights, palette, state.selected_file)
                }
                DiffMode::SideBySide => {
                    render_side_by_side(ui, file, &highlights, palette, state.selected_file)
                }
            }
        });
    });
}

fn file_tree(
    ui: &mut Ui,
    diff: &Diff,
    translator: &Translator,
    palette: Palette,
    state: &mut DiffViewState,
) {
    ui.label(
        RichText::new(translator.text(keys::DIFF_FILE_TREE))
            .size(12.0)
            .strong()
            .color(palette.fg_muted),
    );
    ui.add_space(4.0);
    ScrollArea::vertical()
        .id_salt("diff-file-tree")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (index, file) in diff.files.iter().enumerate() {
                let selected = index == state.selected_file;
                let text = format!(
                    "{}   +{} −{}",
                    file.display_path(),
                    file.additions,
                    file.deletions
                );
                let response = ui.add(
                    egui::Button::new(RichText::new(text).size(11.5).color(if selected {
                        palette.accent_fg
                    } else {
                        palette.fg_default
                    }))
                    .frame(selected)
                    .fill(if selected {
                        palette.selection_bg
                    } else {
                        Color32::TRANSPARENT
                    })
                    .min_size(vec2(0.0, 26.0)),
                );
                if response.clicked() {
                    state.selected_file = index;
                }
            }
        });
}

fn file_header(
    ui: &mut Ui,
    file: &DiffFile,
    translator: &Translator,
    palette: Palette,
    state: &mut DiffViewState,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(file.display_path())
                .monospace()
                .size(13.0)
                .strong()
                .color(palette.fg_default),
        );
        badge(
            ui,
            status_label(translator, file.status),
            palette.canvas_inset,
            palette.fg_muted,
        );
        ui.label(
            RichText::new(format!("+{}", file.additions))
                .size(12.0)
                .color(palette.success_fg),
        );
        ui.label(
            RichText::new(format!("−{}", file.deletions))
                .size(12.0)
                .color(palette.danger_fg),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.selectable_value(
                &mut state.mode,
                DiffMode::SideBySide,
                RichText::new(translator.text(keys::DIFF_MODE_SIDE_BY_SIDE)).size(12.0),
            );
            ui.selectable_value(
                &mut state.mode,
                DiffMode::Unified,
                RichText::new(translator.text(keys::DIFF_MODE_UNIFIED)).size(12.0),
            );
        });
    });
    ui.add_space(6.0);
}

fn render_unified(
    ui: &mut Ui,
    file: &DiffFile,
    highlights: &[Vec<HighlightSpan>],
    palette: Palette,
    selected_file: usize,
) {
    ScrollArea::both()
        .id_salt(("diff-unified", selected_file))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut flat_index = 0usize;
            for hunk in &file.hunks {
                hunk_header(ui, hunk, palette);
                for line in &hunk.lines {
                    let spans = highlights.get(flat_index).map(Vec::as_slice).unwrap_or(&[]);
                    unified_line(ui, line, spans, palette);
                    flat_index += 1;
                }
            }
        });
}

fn hunk_header(ui: &mut Ui, hunk: &gitpanel_diff::DiffHunk, palette: Palette) {
    Frame::NONE
        .fill(palette.canvas_subtle)
        .inner_margin(Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!(
                    "@@ -{},{} +{},{} @@ {}",
                    hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines, hunk.header
                ))
                .monospace()
                .size(11.0)
                .color(palette.fg_muted),
            );
        });
}

fn unified_line(ui: &mut Ui, line: &DiffLine, spans: &[HighlightSpan], palette: Palette) {
    let (background, sign, sign_color) = match line.kind {
        DiffLineKind::Added => (palette.diff_add_bg, '+', palette.success_fg),
        DiffLineKind::Removed => (palette.diff_del_bg, '-', palette.danger_fg),
        DiffLineKind::Context => (palette.canvas_default, ' ', palette.fg_subtle),
    };
    Frame::NONE
        .fill(background)
        .inner_margin(Margin::symmetric(4, 1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                line_number(ui, line.old_lineno, palette);
                line_number(ui, line.new_lineno, palette);
                ui.add_sized(
                    [14.0, 18.0],
                    Label::new(
                        RichText::new(sign.to_string())
                            .monospace()
                            .size(12.0)
                            .color(sign_color),
                    ),
                );
                ui.add(
                    Label::new(line_job(&line.content, spans, palette))
                        .wrap_mode(TextWrapMode::Extend),
                );
            });
        });
}

fn render_side_by_side(
    ui: &mut Ui,
    file: &DiffFile,
    highlights: &[Vec<HighlightSpan>],
    palette: Palette,
    selected_file: usize,
) {
    ScrollArea::both()
        .id_salt(("diff-side", selected_file))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for view in file.side_by_side_hunks() {
                hunk_header(ui, view.hunk, palette);
                for (left, right) in &view.rows {
                    ui.columns(2, |columns| {
                        side_line(&mut columns[0], *left, true, highlights, palette);
                        side_line(&mut columns[1], *right, false, highlights, palette);
                    });
                }
            }
        });
}

fn side_line(
    ui: &mut Ui,
    entry: Option<(usize, &DiffLine)>,
    old_side: bool,
    highlights: &[Vec<HighlightSpan>],
    palette: Palette,
) {
    let Some((flat_index, line)) = entry else {
        ui.allocate_space(vec2(ui.available_width(), 18.0));
        return;
    };
    let background = match line.kind {
        DiffLineKind::Added => palette.diff_add_bg,
        DiffLineKind::Removed => palette.diff_del_bg,
        DiffLineKind::Context => palette.canvas_default,
    };
    Frame::NONE
        .fill(background)
        .inner_margin(Margin::symmetric(4, 1))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let number = if old_side {
                    line.old_lineno
                } else {
                    line.new_lineno
                };
                line_number(ui, number, palette);
                let sign = match line.kind {
                    DiffLineKind::Added => '+',
                    DiffLineKind::Removed => '-',
                    DiffLineKind::Context => ' ',
                };
                let color = match line.kind {
                    DiffLineKind::Added => palette.success_fg,
                    DiffLineKind::Removed => palette.danger_fg,
                    DiffLineKind::Context => palette.fg_subtle,
                };
                ui.add_sized(
                    [14.0, 18.0],
                    Label::new(
                        RichText::new(sign.to_string())
                            .monospace()
                            .size(12.0)
                            .color(color),
                    ),
                );
                let spans = highlights.get(flat_index).map(Vec::as_slice).unwrap_or(&[]);
                ui.add(
                    Label::new(line_job(&line.content, spans, palette))
                        .wrap_mode(TextWrapMode::Extend),
                );
            });
        });
}

fn line_number(ui: &mut Ui, number: Option<u32>, palette: Palette) {
    ui.add_sized(
        [36.0, 18.0],
        Label::new(
            RichText::new(number.map(|value| value.to_string()).unwrap_or_default())
                .monospace()
                .size(11.0)
                .color(palette.fg_subtle),
        ),
    );
}

fn line_job(content: &str, spans: &[HighlightSpan], palette: Palette) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    if spans.is_empty() {
        job.append(
            content,
            0.0,
            TextFormat {
                font_id: FontId::monospace(12.0),
                color: palette.fg_default,
                ..Default::default()
            },
        );
        return job;
    }
    for span in spans {
        let Some(text) = content.get(span.start..span.end) else {
            continue;
        };
        let mut format = TextFormat {
            font_id: FontId::monospace(12.0),
            color: Color32::from_rgb(span.color.0, span.color.1, span.color.2),
            ..Default::default()
        };
        format.italics = span.italic;
        if span.underline {
            format.underline = egui::Stroke::new(1.0, format.color);
        }
        job.append(text, 0.0, format);
    }
    job
}

fn status_label(translator: &Translator, status: FileStatus) -> String {
    match status {
        FileStatus::Added => translator.text(keys::DIFF_ADDED_FILE),
        FileStatus::Deleted => translator.text(keys::DIFF_DELETED_FILE),
        FileStatus::Renamed => translator.text(keys::DIFF_RENAMED),
        FileStatus::Modified => translator.text(keys::DIFF_MODIFIED_FILE),
        FileStatus::Copied => translator.text(keys::DIFF_COPIED_FILE),
        FileStatus::Unknown => translator.text(keys::DIFF_UNKNOWN_FILE),
    }
}
