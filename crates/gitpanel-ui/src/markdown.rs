use egui::Ui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

pub fn markdown(ui: &mut Ui, cache: &mut CommonMarkCache, source: &str) {
    CommonMarkViewer::new().show(ui, cache, source);
}
