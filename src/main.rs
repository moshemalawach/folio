#![forbid(unsafe_code)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod calc;
mod canvas;
mod clipboard;
mod db;
mod editor;
mod export;
mod find;
mod i18n;
mod icons;
mod images;
mod import_notebook;
mod ink_shapes;
mod math;
mod model;
mod outline;
mod paste;
mod pdf;
mod reading;
mod ribbon;
mod seed;
mod speech;
mod spell;
mod table;
mod tags;
mod templates;
mod theme;
mod util;
mod widgets;

use app::FolioApp;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!(
            "folio {} ({})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        );
        return Ok(());
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "Folio {} — a local-first notebook\n\nUSAGE:\n    folio [OPTIONS]\n\nOPTIONS:\n    -h, --help            Print help\n    -V, --version         Print version\n    --screenshot PATH     Save a window image and exit\n    --screenshot-wait MS  Delay before the screenshot\n    --size WxH            Window size (default 1320x860)\n\nData lives in {}.\nSet FOLIO_DATA_DIR to use another folder.",
            env!("CARGO_PKG_VERSION"),
            db::default_data_dir().display()
        );
        return Ok(());
    }

    let screenshot_path = args
        .iter()
        .position(|arg| arg == "--screenshot")
        .and_then(|index| args.get(index + 1))
        .map(std::path::PathBuf::from);
    let screenshot_wait = args
        .iter()
        .position(|arg| arg == "--screenshot-wait")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(400);
    let window_size = args
        .iter()
        .position(|arg| arg == "--size")
        .and_then(|index| args.get(index + 1))
        .and_then(|value| {
            let (width, height) = value.split_once('x')?;
            Some([width.parse::<f32>().ok()?, height.parse::<f32>().ok()?])
        })
        .unwrap_or([1320.0, 860.0]);

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title("Folio")
        .with_app_id("com.folio.notes")
        .with_inner_size(window_size)
        .with_min_inner_size([980.0, 640.0])
        .with_drag_and_drop(true);
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Folio",
        options,
        Box::new(move |cc| {
            let mut app = FolioApp::new(cc)?;
            app.set_screenshot_path(screenshot_path.clone(), screenshot_wait);
            Ok(Box::new(app))
        }),
    )
}

fn app_icon() -> Option<eframe::egui::IconData> {
    let image = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .ok()?
        .to_rgba8();
    Some(eframe::egui::IconData {
        rgba: image.clone().into_raw(),
        width: image.width(),
        height: image.height(),
    })
}
