//! Native egui shell. Everything below `ui` depends on a GUI toolkit; the core engine does not.

mod app;
mod background;
mod context_menu;
mod ext_view;
mod palette;
mod panels;
mod session;
mod toolbar;
mod tree_cells;
mod tree_model;
mod tree_view;

use std::path::PathBuf;

use eframe::egui;

pub fn run(initial_path: Option<PathBuf>) -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 760.0])
            .with_app_id("inodetree"),
        ..Default::default()
    };
    eframe::run_native(
        "inodetree",
        options,
        Box::new(move |creation| {
            Ok(Box::new(app::App::new(
                &creation.egui_ctx,
                initial_path.as_deref(),
            )))
        }),
    )
}
