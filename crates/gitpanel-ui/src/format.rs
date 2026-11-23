use chrono::{DateTime, Local, Utc};
use egui::Color32;
use gitpanel_i18n::{Arg, Translator, keys};

pub fn relative_time(translator: &Translator, at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - at).num_seconds();
    if seconds < 0 {
        let seconds = -seconds;
        if seconds < 3600 {
            return translator.text_args(
                keys::TIME_IN_MINUTES,
                &[Arg::int("count", (seconds / 60).max(1))],
            );
        }
        if seconds < 86_400 {
            return translator.text_args(
                keys::TIME_IN_HOURS,
                &[Arg::int("count", (seconds / 3600).max(1))],
            );
        }
        return format_date(translator, at, false);
    }
    if seconds < 60 {
        return translator.text(keys::TIME_NOW);
    }
    if seconds < 3600 {
        let count = (seconds / 60).max(1);
        return translator.text_args(
            if count == 1 {
                keys::TIME_MINUTE_AGO
            } else {
                keys::TIME_MINUTES_AGO
            },
            &[Arg::int("count", count)],
        );
    }
    if seconds < 86_400 {
        let count = (seconds / 3600).max(1);
        return translator.text_args(
            if count == 1 {
                keys::TIME_HOUR_AGO
            } else {
                keys::TIME_HOURS_AGO
            },
            &[Arg::int("count", count)],
        );
    }
    if seconds < 2_592_000 {
        let count = (seconds / 86_400).max(1);
        return translator.text_args(
            if count == 1 {
                keys::TIME_DAY_AGO
            } else {
                keys::TIME_DAYS_AGO
            },
            &[Arg::int("count", count)],
        );
    }
    if seconds < 31_536_000 {
        let count = (seconds / 2_592_000).max(1);
        return translator.text_args(
            if count == 1 {
                keys::TIME_MONTH_AGO
            } else {
                keys::TIME_MONTHS_AGO
            },
            &[Arg::int("count", count)],
        );
    }
    let count = (seconds / 31_536_000).max(1);
    translator.text_args(
        if count == 1 {
            keys::TIME_YEAR_AGO
        } else {
            keys::TIME_YEARS_AGO
        },
        &[Arg::int("count", count)],
    )
}

pub fn format_date(translator: &Translator, at: DateTime<Utc>, with_time: bool) -> String {
    let local = at.with_timezone(&Local);
    let pattern = if with_time {
        translator.locale().date_time()
    } else {
        translator.locale().date_short()
    };
    local.format(pattern).to_string()
}

pub fn compact_number(value: u64) -> String {
    if value < 1_000 {
        value.to_string()
    } else if value < 1_000_000 {
        format!("{:.1}k", value as f64 / 1_000.0)
    } else {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    }
}

pub fn language_color(language: &str) -> Color32 {
    match language {
        "Rust" => Color32::from_rgb(222, 165, 132),
        "JavaScript" => Color32::from_rgb(241, 224, 90),
        "TypeScript" => Color32::from_rgb(49, 120, 198),
        "Python" => Color32::from_rgb(53, 114, 165),
        "Go" => Color32::from_rgb(0, 173, 216),
        "Java" => Color32::from_rgb(176, 114, 25),
        "C" => Color32::from_rgb(85, 85, 85),
        "C++" => Color32::from_rgb(243, 75, 125),
        "C#" => Color32::from_rgb(23, 134, 0),
        "Ruby" => Color32::from_rgb(112, 21, 22),
        "PHP" => Color32::from_rgb(79, 93, 149),
        "Swift" => Color32::from_rgb(240, 81, 56),
        "Kotlin" => Color32::from_rgb(169, 123, 255),
        "Shell" => Color32::from_rgb(137, 224, 81),
        "HTML" => Color32::from_rgb(227, 76, 38),
        "CSS" => Color32::from_rgb(86, 61, 124),
        "Dockerfile" => Color32::from_rgb(56, 132, 255),
        _ => Color32::from_rgb(139, 148, 158),
    }
}
