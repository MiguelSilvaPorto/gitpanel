#![forbid(unsafe_code)]

pub mod charts;
pub mod diff_view;
pub mod format;
pub mod icons;
pub mod repo_table;
pub mod theme;
pub mod widgets;

pub use charts::commit_chart;
pub use diff_view::{DiffMode, DiffViewState, diff_view};
pub use format::{compact_number, format_date, language_color, relative_time};
pub use icons::{Icon, icon, icon_button};
pub use repo_table::repo_table;
pub use theme::{Palette, Theme, install_fonts, primary_button, secondary_button};
pub use widgets::{
    badge, card, empty_state, error_banner, info_banner, kv_row, loading, muted, outline_badge,
    page_header, panel_frame, repo_stats, search_input, section_header, stat_card, tabs,
};
