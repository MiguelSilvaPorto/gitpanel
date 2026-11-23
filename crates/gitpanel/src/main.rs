mod app;
mod jobs;

use app::{FatalApp, GitPanelApp};
use tracing_subscriber::EnvFilter;

fn main() -> eframe::Result {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("gitpanel=info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([980.0, 620.0])
            .with_title("GitPanel"),
        ..Default::default()
    };

    eframe::run_native(
        "GitPanel",
        options,
        Box::new(|cc| {
            let app: Box<dyn eframe::App> = match GitPanelApp::new(cc) {
                Ok(app) => Box::new(app),
                Err(error) => Box::new(FatalApp::new(error.to_string())),
            };
            Ok(app)
        }),
    )
}
