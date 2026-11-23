use chrono::{DateTime, Local, Utc};
use egui::Ui;
use egui_plot::{Bar, BarChart, Plot};
use gitpanel_i18n::{Translator, keys};
use gitpanel_metrics::{CommitPoint, Granularity};

use crate::theme::Palette;

pub fn commit_chart(
    ui: &mut Ui,
    points: &[CommitPoint],
    granularity: Granularity,
    translator: &Translator,
    palette: Palette,
) {
    if points.is_empty() {
        ui.label(
            egui::RichText::new(translator.text(keys::CHART_NO_DATA))
                .size(12.0)
                .color(palette.fg_muted),
        );
        return;
    }
    let width = match granularity {
        Granularity::Minute => 35.0,
        Granularity::Hour => 2_400.0,
        Granularity::Day => 60_000.0,
        Granularity::Week => 420_000.0,
        Granularity::Month => 1_800_000.0,
    };
    let bars: Vec<Bar> = points
        .iter()
        .map(|point| {
            Bar::new(point.start.timestamp() as f64, f64::from(point.count))
                .width(width)
                .name(format!(
                    "{}: {}",
                    format_bucket(translator, point.start.timestamp() as f64, granularity),
                    point.count
                ))
        })
        .collect();
    let title = translator.text(keys::CHART_COMMITS_AXIS);
    Plot::new("gitpanel-commit-chart")
        .height(260.0)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .allow_double_click_reset(false)
        .show_grid(true)
        .x_axis_formatter(move |mark, _| format_bucket(translator, mark.value, granularity))
        .y_axis_formatter(|mark, _| format!("{}", mark.value.max(0.0) as i64))
        .show(ui, |plot_ui| {
            plot_ui.bar_chart(
                BarChart::new(title, bars)
                    .color(palette.accent_fg)
                    .width(width),
            );
        });
}

fn format_bucket(translator: &Translator, timestamp: f64, granularity: Granularity) -> String {
    let Some(utc) = DateTime::<Utc>::from_timestamp(timestamp as i64, 0) else {
        return String::new();
    };
    let local = utc.with_timezone(&Local);
    let pattern = match granularity {
        Granularity::Minute | Granularity::Hour => translator.locale().date_time(),
        Granularity::Day | Granularity::Week | Granularity::Month => {
            translator.locale().date_short()
        }
    };
    local.format(pattern).to_string()
}
