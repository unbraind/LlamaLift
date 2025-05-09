// src/main.rs
// Main entry point for LlamaLift. Sets up logging, initializes the eframe window (including icon), and runs the main application loop.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Declare the main application module
mod app;

// Use necessary crates and modules
use crate::app::{
    config::{load_initial_config, APP_NAME, SCRIPT_VERSION},
    state::UpdateMessage,
    utils::LOGO_BYTES,
};
use chrono::Local;
use eframe::egui;
use image::GenericImageView;
use log::{error, info, LevelFilter};
use std::io::Write;
use std::sync::mpsc::channel;

fn main() -> Result<(), eframe::Error> {
    // --- Logger Setup ---
    let initial_config = load_initial_config();

    // --- Channel Setup ---
    let (update_sender, update_receiver) = channel::<UpdateMessage>();
    let logger_sender = update_sender.clone();
    let app_sender = update_sender.clone();

    let logger_tz_str = initial_config.tz.name().to_string();
    let logger_tz = initial_config.tz;

    let log_level_to_init = if cfg!(debug_assertions) {
        LevelFilter::Trace // Always use Trace in debug builds for max verbosity
    } else {
        initial_config.log_level
    };

    // Initialize env_logger
    env_logger::Builder::new()
        .filter_level(log_level_to_init)
        .format(move |buf, record| {
            let now = Local::now().with_timezone(&logger_tz);
            let log_msg = format!(
                "[{}] [{}] {}",
                now.format("%Y-%m-%d %H:%M:%S %Z"),
                record.level(),
                record.args()
            );
            // Send INFO and lower logs to the GUI via the channel
            if record.level() <= LevelFilter::Info {
                if let Err(e) = logger_sender.send(UpdateMessage::Log(log_msg.clone())) {
                    eprintln!("ERROR: Failed to send log message to UI thread: {}", e);
                }
            }
            writeln!(buf, "{}", log_msg)
        })
        .init();

    // --- Initial Log Messages ---
    info!("--- {} v{} Starting ---", APP_NAME, SCRIPT_VERSION);
    info!("--- Initial Configuration ---");
    info!("OLLAMA_HOST (Initial): {}", initial_config.ollama_host);
    info!("LOG_LEVEL (Effective Init): {}", log_level_to_init);
    info!("TZ (Effective Init): {}", logger_tz_str);
    info!("---------------------------");

    // --- Load Icon Data ---
    let icon = match image::load_from_memory(LOGO_BYTES) {
        Ok(image) => {
            info!("Successfully decoded icon from embedded PNG bytes.");
            let (width, height) = image.dimensions();
            // Convert to RGBA8 format which egui::IconData expects
            let rgba_data = image.to_rgba8().into_raw();
            Some(egui::IconData { // Use egui::IconData directly
                rgba: rgba_data,
                width,
                height,
            })
        }
        Err(err) => {
            error!("Failed to decode icon from embedded PNG bytes: {}", err);
            None
        }
    };

    // --- eframe Setup ---
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([600.0, 400.0])
            .with_icon(icon.unwrap_or_default()),
        ..Default::default()
    };

    // Run the eframe application loop
    eframe::run_native(
        APP_NAME, // Window title
        native_options,
        Box::new(|cc| Ok(Box::new(app::OllamaPullerApp::new(cc, app_sender, update_receiver)))),
    )
}
