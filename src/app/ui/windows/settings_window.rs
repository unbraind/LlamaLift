// src/app/ui/windows/settings_window.rs
// Contains the drawing function for the Settings window.

// --- Necessary imports ---
use crate::app::{
    state::UpdateMessage,
    OllamaPullerApp,
};
use chrono_tz::Tz;
use egui::{
    Align2, ComboBox, Context, Grid, TextEdit, Window,
};
use log::{error, info};
use std::str::FromStr;

// --- Window Drawing Function ---

// Draws the "Settings" window and handles its interactions (Save, Cancel, Close).
// Uses app.pending_settings for temporary state management.
//
// # Arguments
//
// * app - Mutable reference to the main application state (OllamaPullerApp).
// * ctx - The egui context (&egui::Context).
pub fn draw_settings_window(app: &mut OllamaPullerApp, ctx: &Context) {
    // Temporary boolean to manage window visibility without borrowing issues
    let mut settings_window_open = app.show_settings_window;
    // Flags to track button clicks within the window closure
    let mut save_and_close_clicked = false;
    let mut cancel_settings_clicked = false;

    Window::new("Settings")
        .open(&mut settings_window_open) // Control visibility with the temporary boolean
        .resizable(true) // Allow resizing
        .default_width(400.0)
        .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO) // Center the window
        .show(ctx, |ui| {
            if app.pending_settings.is_none() {
                error!("Settings window drawn without pending state initialized!");
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Internal error: State not initialized.",
                );
                return;
            }
            let pending = app.pending_settings.as_mut().unwrap();

            ui.heading("Runtime Settings");
            ui.label("These settings override .env/environment variables and are saved persistently.");

            // Display the path to the configuration file, if available
            if let Some(path) = &app.config_path {
                // Use label for potentially long paths, allow wrapping
                ui.label(format!("Config file: {}", path.display()));
            } else {
                ui.label("Config file path not found.");
            }
            ui.separator();

            // Grid layout for settings fields
            Grid::new("settings_grid")
                .num_columns(2)
                .spacing([40.0, 4.0]) // Horizontal and vertical spacing
                .striped(true) // Alternate row background colors
                .show(ui, |ui| {
                    // Ollama Host setting
                    ui.label("Ollama Host:");
                    // Edit the temporary pending state
                    ui.text_edit_singleline(&mut pending.ollama_host);
                    ui.end_row();

                    // Log Level setting
                    ui.label("Log Level:");
                    // Use ComboBox for selecting log level
                    ComboBox::from_label("") // No label needed next to the combo box
                        .selected_text(&pending.log_level) // Show current selection
                        .show_ui(ui, |ui| {
                            // Iterate through possible log level strings
                            for level in ["TRACE", "DEBUG", "INFO", "WARN", "ERROR"] {
                                // Allow selecting a value, updates pending.log_level
                                ui.selectable_value(
                                    &mut pending.log_level,
                                    level.to_string(),
                                    level,
                                );
                            }
                        });
                    ui.end_row();
                    ui.label("Timezone (IANA):");
                    let timezone_edit = TextEdit::singleline(&mut pending.tz)
                        .hint_text("e.g., Europe/Vienna, UTC");
                    ui.add(timezone_edit);
                    ui.end_row();
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Save & Close").clicked() {
                    if Tz::from_str(&pending.tz).is_err() {
                        let error_msg = format!(
                            "Invalid Timezone format: '{}'. Please use IANA format (e.g., 'Europe/Vienna', 'UTC'). Settings not saved.",
                            pending.tz 
                        );
                        error!("{}", error_msg);
                        let _ = app
                            .task_update_sender
                            .send(UpdateMessage::Log(format!("ERROR: {}", error_msg)));
                    } else {
                        save_and_close_clicked = true;
                    }
                }
                if ui.button("Cancel").clicked() {
                    cancel_settings_clicked = true;
                }
            });
            ui.separator();
            ui.label("Note: Log Level and Timezone changes may require an application restart for the log timestamp format to fully update.");
        });

    if save_and_close_clicked {
        if let Some(saved_settings) = app.pending_settings.take() {
            app.settings = saved_settings;
            app.save_settings();
            info!("Settings updated and saved.");
            let _ = app
                .task_update_sender
                .send(UpdateMessage::Log("INFO: Settings updated and saved.".to_string()));
        } else {
            // This case should ideally not happen if the window was open
            error!("Save clicked but pending_settings was None!");
            let _ = app.task_update_sender.send(UpdateMessage::Log(
                "ERROR: Save clicked but pending_settings was None!".to_string(),
            ));
        }
        settings_window_open = false;
    } else if cancel_settings_clicked {
        info!("Settings changes cancelled.");
        let _ = app
            .task_update_sender
            .send(UpdateMessage::Log("INFO: Settings changes cancelled.".to_string()));
        settings_window_open = false;
        app.pending_settings = None;
    }

    if !settings_window_open && app.show_settings_window {
        if !save_and_close_clicked && !cancel_settings_clicked {
            info!("Settings window closed via 'X'. Changes discarded.");
            let _ = app.task_update_sender.send(UpdateMessage::Log(
                "INFO: Settings window closed via 'X'. Changes discarded.".to_string(),
            ));
            app.pending_settings = None;
        }
    }

    app.show_settings_window = settings_window_open;

    // If the window is supposed to be closed, ensure pending state is None
    if !app.show_settings_window {
        app.pending_settings = None;
    }
}
