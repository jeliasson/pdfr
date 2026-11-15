//! pdfr — a desktop PDF viewer and form filler.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod app;
mod assist_ui;
mod draw;
mod icons;
mod system;
mod updates;
mod theme;
mod worker;

use app::{App, CliOptions};
use eframe::egui;

const USAGE: &str = "pdfr — view and fill PDF forms

Usage: pdfr [FILE.pdf]

Options:
  -h, --help       Show this help
  -V, --version    Show the version
  --register       Add pdfr to the app menu, \"Open with\" and the terminal
                   (for installs from an archive)
  --unregister     Undo --register
";

fn parse_args() -> Result<CliOptions, String> {
    let mut o = CliOptions::default();
    let mut args = std::env::args_os().skip(1);
    while let Some(a) = args.next() {
        let s = a.to_string_lossy().to_string();
        match s.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "-V" | "--version" => {
                println!("pdfr {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--register" | "--unregister" => {
                let r = if s == "--register" {
                    system::register()
                } else {
                    system::unregister()
                };
                match r {
                    Ok(m) => {
                        println!("{m}");
                        std::process::exit(0)
                    }
                    Err(e) => {
                        eprintln!("pdfr: {e}");
                        std::process::exit(1)
                    }
                }
            }
            // Hidden flags used for verification.
            "--demo" => o.demo = true,
            "--no-ai" => o.no_ai = true,
            "--save" => o.save = Some(args.next().ok_or("--save needs a path")?.into()),
            "--screenshot" => {
                o.screenshot = Some(args.next().ok_or("--screenshot needs a path")?.into())
            }
            "--hover" => {
                let v = args
                    .next()
                    .ok_or("--hover needs x,y")?
                    .to_string_lossy()
                    .to_string();
                let (x, y) = v.split_once(',').ok_or("--hover needs x,y")?;
                o.hover = Some((
                    x.parse().map_err(|_| "bad --hover")?,
                    y.parse().map_err(|_| "bad --hover")?,
                ));
            }
            "--record" => o.record = Some(args.next().ok_or("--record needs a folder")?.into()),
            "--script" => {
                o.script = Some(
                    args.next()
                        .ok_or("--script needs a name")?
                        .to_string_lossy()
                        .into(),
                )
            }
            "--ask-remember" => o.ask_remember = true,
            "--settings" => {
                o.settings = Some(
                    args.next()
                        .ok_or("--settings needs a section")?
                        .to_string_lossy()
                        .into(),
                )
            }
            "--data-dir" => o.data_dir = Some(args.next().ok_or("--data-dir needs a path")?.into()),
            "--zoom" => {
                o.zoom = Some(
                    args.next()
                        .ok_or("--zoom needs a number")?
                        .to_string_lossy()
                        .parse()
                        .map_err(|_| "bad --zoom")?,
                )
            }
            _ if s.starts_with("--") => return Err(format!("unknown option {s}")),
            _ => o.file = Some(a.into()),
        }
    }
    // Automated runs never call the AI.
    if o.demo || o.save.is_some() || o.screenshot.is_some() || o.record.is_some() {
        o.no_ai = true;
    }
    Ok(o)
}

fn main() -> eframe::Result<()> {
    let cli = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("pdfr: {e}\n\n{USAGE}");
            std::process::exit(2);
        }
    };
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("pdfr")
        .with_app_id(system::APP_ID)
            .with_inner_size([1100.0, 900.0])
            .with_min_inner_size([480.0, 360.0])
            .with_drag_and_drop(true);
    if let Some(icon) = system::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        event_loop_builder: Some(Box::new(|_| system::before_event_loop())),
        ..Default::default()
    };
    eframe::run_native(
        "pdfr",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(cc, cli)))),
    )
}
