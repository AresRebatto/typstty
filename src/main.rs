mod app;
mod text_buffer;

use app::TypsttyApp;
use std::{env, path::PathBuf, process::exit};

fn main() -> eframe::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: typstty <filename.typ>");
        exit(1);
    }

    let file_path = PathBuf::from(&args[1]);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("typstty")
            .with_inner_size([900.0, 600.0])
            .with_min_inner_size([400.0, 300.0]),
        ..Default::default()
    };

    eframe::run_native(
        "typstty",
        native_options,
        Box::new(|cc| Box::new(TypsttyApp::new(cc, file_path)) as Box<dyn eframe::App>),
    )
}
