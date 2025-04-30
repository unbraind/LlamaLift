#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use chrono::{DateTime, Local};
use chrono_tz::Tz;
use confy;
use dotenvy::dotenv;
use eframe::{
    egui::{
        self, Align, CentralPanel, CollapsingHeader, Grid, Layout, ProgressBar, RichText,
        ScrollArea, Separator, TextEdit, TextWrapMode, TopBottomPanel, ViewportCommand, Window,
        ImageData, TextureOptions,
    },
    App, CreationContext,
};
use futures_util::StreamExt;
use image;
use log::{debug, error, info, trace, warn, LevelFilter};
use serde::{Deserialize, Serialize};
use std::{
    env,
    path::PathBuf,
    str::FromStr,
    sync::{
        mpsc::{channel, Receiver, Sender},
        Arc, Mutex, 
    },
    time::Duration,
};
use tokio::runtime::Runtime;

// --- Global Configuration Block ---
const SCRIPT_VERSION: &str = "0.1.0";
const APP_NAME: &str = "LlamaLift";
const MAX_MODEL_INPUTS: usize = 100;
const DEFAULT_TZ: &str = "Europe/Vienna";
const DEFAULT_LOG_LEVEL: &str = "INFO";
const DEFAULT_OLLAMA_HOST: &str = "127.0.0.1:11434";

const LOGO_BYTES: &[u8] = include_bytes!("../assets/LlamaLift.png");


// --- Configuration Structs ---
#[derive(Clone, Debug)]
struct Config {
    ollama_host: String,
    tz: Tz,
}

#[derive(Clone, Debug)]
struct InitialConfig {
    ollama_host: String,
    log_level: LevelFilter,
    tz: Tz,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct AppSettings {
    ollama_host: String,
    log_level: String,
    tz: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        let initial_config = load_initial_config();
        AppSettings {
            ollama_host: initial_config.ollama_host,
            log_level: initial_config.log_level.to_string(),
            tz: initial_config.tz.name().to_string(),
        }
    }
}

fn load_initial_config() -> InitialConfig {
    dotenv().ok();
    let tz_str = env::var("TZ").unwrap_or_else(|_| DEFAULT_TZ.to_string());
    let tz = Tz::from_str(&tz_str).unwrap_or_else(|err| {
        eprintln!(
            "WARN: Invalid TZ '{}' from env/default. Falling back to UTC. Error: {}",
            tz_str, err
        );
        Tz::UTC
    });
    let log_level_str = env::var("LOG_LEVEL").unwrap_or_else(|_| DEFAULT_LOG_LEVEL.to_string());
    let log_level = LevelFilter::from_str(&log_level_str).unwrap_or_else(|err| {
        eprintln!(
            "WARN: Invalid LOG_LEVEL '{}' from env/default. Falling back to {}. Error: {}",
            log_level_str, DEFAULT_LOG_LEVEL, err
        );
        LevelFilter::from_str(DEFAULT_LOG_LEVEL).unwrap()
    });
    let ollama_host = env::var("OLLAMA_HOST").unwrap_or_else(|_| DEFAULT_OLLAMA_HOST.to_string());
    InitialConfig { ollama_host, log_level, tz }
}

// --- Ollama API Structures ---
#[derive(Deserialize, Debug, Clone)]
struct OllamaPullStatus { status: String, digest: Option<String>, total: Option<u64>, completed: Option<u64>, error: Option<String>, }
#[derive(Deserialize, Debug, Clone)]
struct OllamaModel { name: String, modified_at: String, size: u64, #[serde(skip)] modified_local: Option<String>, #[serde(skip)] size_human: String, }
#[derive(Deserialize, Debug, Clone)]
struct OllamaTagsResponse { models: Vec<OllamaModel>, }
#[derive(Serialize, Debug, Clone)]
struct OllamaDeleteRequest { name: String, }

// --- Application State Enums ---
#[derive(Clone, Debug, PartialEq)]
enum AppStatus { Idle, Pulling(usize, usize), ListingModels, DeletingModel(String), Success, Error(String), }
#[derive(Clone, Debug, PartialEq)]
enum AppView { Download, ManageModels, }

// --- Main Application Struct ---
struct OllamaPullerApp {
    model_inputs: Vec<String>,
    logs: Arc<Mutex<Vec<String>>>,
    logs_string_cache: String,
    logs_dirty: bool,
    logs_collapsed: bool,
    progress: Arc<Mutex<f32>>,
    status_text: Arc<Mutex<String>>,
    status: Arc<Mutex<AppStatus>>,
    show_settings_window: bool,
    show_about_window: bool,
    settings: AppSettings,
    current_view: AppView,
    listed_models: Arc<Mutex<Vec<OllamaModel>>>,
    model_to_delete: Option<String>,
    copy_logs_requested: bool,
    update_sender: Sender<UpdateMessage>,
    update_receiver: Receiver<UpdateMessage>,
    rt: Arc<Runtime>,
    config_path: Option<PathBuf>,
    logo_texture: Option<egui::TextureHandle>,
}

#[derive(Debug)]
enum UpdateMessage { Log(String), Progress(f32), StatusText(String), Status(AppStatus), ModelList(Vec<OllamaModel>), }

impl OllamaPullerApp {
    fn new(cc: &CreationContext<'_>) -> Self {
        let settings: AppSettings = match confy::load(APP_NAME, None) {
            Ok(cfg) => { info!("Successfully loaded settings from config file."); cfg }
            Err(e) => {
                warn!("Failed to load config file ('{}'), using defaults: {}", APP_NAME, e);
                let default_settings = AppSettings::default();
                if let Err(store_err) = confy::store(APP_NAME, None, &default_settings) {
                    error!("Failed to store default settings: {}", store_err);
                } else { info!("Stored default settings."); }
                default_settings
            }
        };

        let config_path = confy::get_configuration_file_path(APP_NAME, None).ok();
        let (update_sender, update_receiver) = channel();
        let logger_sender = update_sender.clone();

        let app_log_level = LevelFilter::from_str(&settings.log_level).unwrap_or_else(|_| LevelFilter::from_str(DEFAULT_LOG_LEVEL).unwrap());
        let logger_tz_str = settings.tz.clone();
        let logger_tz = Tz::from_str(&logger_tz_str).unwrap_or_else(|_| {
            warn!("Invalid TZ '{}' in settings, logger falling back to UTC.", logger_tz_str);
            Tz::UTC
        });

        // Initialize Logger
        let log_level_to_init = if cfg!(debug_assertions) { LevelFilter::Trace } else { app_log_level };
        env_logger::Builder::new()
            .filter_level(log_level_to_init)
            .format(move |buf, record| {
                use std::io::Write;
                let now = Local::now().with_timezone(&logger_tz);
                let log_msg = format!("[{}] [{}] {}", now.format("%Y-%m-%d %H:%M:%S %Z"), record.level(), record.args());
                // Send INFO and lower logs to the GUI
                if record.level() <= LevelFilter::Info {
                   let _ = logger_sender.send(UpdateMessage::Log(log_msg.clone()));
                }
                writeln!(buf, "{}", log_msg) // Also write to console/default output
            })
            .init();

        info!("--- LlamaLift v{} Starting ---", SCRIPT_VERSION); // Log current version
        if let Some(path) = &config_path { info!("Using config file: {}", path.display()); }
        else { warn!("Could not determine config file path."); }
        info!("--- Effective Configuration (from persistent settings) ---");
        info!("OLLAMA_HOST: {}", settings.ollama_host);
        info!("LOG_LEVEL: {}", settings.log_level);
        info!("TZ: {}", settings.tz);
        info!("---------------------------");

        // Initial connectivity check
        match std::net::TcpStream::connect(&settings.ollama_host) {
            Ok(_) => info!("Successfully connected to OLLAMA_HOST '{}' on startup.", settings.ollama_host),
            Err(e) => {
                let warn_msg = format!("WARN: Could not connect to OLLAMA_HOST '{}' on startup: {}. Check host/port and ensure Ollama is running.", settings.ollama_host, e);
                warn!("{}", warn_msg);
                let _ = update_sender.send(UpdateMessage::Log(warn_msg));
            }
        }

        let rt = Arc::new(tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("Failed to create Tokio runtime"));

        // Load the logo image using the corrected helper function
        let logo_texture = load_image_from_bytes(&cc.egui_ctx, "logo", LOGO_BYTES);
        if logo_texture.is_none() {
             error!("Failed to load embedded logo image from '../assets/LlamaLift.png'.");
             let _ = update_sender.send(UpdateMessage::Log("ERROR: Failed to load embedded logo image.".to_string()));
        } else { info!("Successfully loaded embedded logo image."); }


        Self {
            model_inputs: vec!["".to_string()],
            logs: Arc::new(Mutex::new(Vec::new())),
            logs_string_cache: String::new(),
            logs_dirty: true,
            logs_collapsed: true,
            progress: Arc::new(Mutex::new(0.0)),
            status_text: Arc::new(Mutex::new("Idle".to_string())),
            status: Arc::new(Mutex::new(AppStatus::Idle)),
            show_settings_window: false,
            show_about_window: false,
            settings,
            current_view: AppView::ManageModels, // Start in Manage Models view
            listed_models: Arc::new(Mutex::new(Vec::new())),
            model_to_delete: None,
            copy_logs_requested: false,
            update_sender,
            update_receiver,
            rt,
            config_path,
            logo_texture,
        }
    }

    fn rebuild_log_cache(&mut self) {
        if self.logs_dirty {
            let logs_vec = self.logs.lock().unwrap();
            self.logs_string_cache = logs_vec.join("\n");
            self.logs_dirty = false;
        }
    }

    fn get_current_config(&self) -> Config {
        Config {
            ollama_host: self.settings.ollama_host.clone(),
            tz: Tz::from_str(&self.settings.tz).unwrap_or_else(|_| {
                warn!("Invalid TZ '{}' in settings during runtime config fetch, falling back to UTC.", self.settings.tz);
                Tz::UTC
            }),
        }
    }

    fn save_settings(&self) {
        match confy::store(APP_NAME, None, &self.settings) {
            Ok(_) => { info!("Settings saved successfully."); let _ = self.update_sender.send(UpdateMessage::Log("INFO: Settings saved.".to_string())); }
            Err(e) => { error!("Failed to save settings: {}", e); let _ = self.update_sender.send(UpdateMessage::Log(format!("ERROR: Failed to save settings: {}", e))); }
        }
        info!("--- Updated Configuration Saved ---");
        info!("OLLAMA_HOST: {}", self.settings.ollama_host);
        info!("LOG_LEVEL: {}", self.settings.log_level);
        info!("TZ: {}", self.settings.tz);
        info!("--------------------------------");
    }

    fn refresh_model_list(&self) {
        let config = self.get_current_config();
        let sender = self.update_sender.clone();
        let rt_handle = self.rt.clone();
        let status_arc = self.status.clone();

        let mut current_status = status_arc.lock().unwrap();
        if *current_status == AppStatus::ListingModels {
            warn!("Model list refresh already in progress."); return;
        }
        // Allow refresh if idle, success, or error. Prevent during pull/delete.
        if !matches!(*current_status, AppStatus::Idle | AppStatus::Success | AppStatus::Error(_)) {
             warn!("Cannot refresh model list while another operation ({:?}) is in progress.", *current_status);
             let _ = sender.send(UpdateMessage::Log(format!("WARN: Cannot refresh model list during {:?}.", *current_status)));
             return;
        }
        *current_status = AppStatus::ListingModels;
        drop(current_status); // Release lock

        let _ = sender.send(UpdateMessage::StatusText("Listing models...".to_string()));
        info!("Refreshing model list...");
        rt_handle.spawn(async move {
            match list_models_async(&config, sender.clone()).await {
                Ok(models) => {
                    info!("Successfully listed {} models.", models.len());
                    let _ = sender.send(UpdateMessage::ModelList(models));
                    let _ = sender.send(UpdateMessage::StatusText("Model list updated.".to_string()));
                    let _ = sender.send(UpdateMessage::Status(AppStatus::Idle)); // Set back to Idle after success
                }
                Err(e) => {
                    error!("Failed to list models: {}", e);
                    let error_message = format!("Error listing models: {}", e);
                    let _ = sender.send(UpdateMessage::StatusText(error_message.clone()));
                    let _ = sender.send(UpdateMessage::Status(AppStatus::Error(e.to_string()))); // Set Error status
                }
            }
        });
    }

    fn trigger_delete_model(&self, model_name: &str) {
        let config = self.get_current_config();
        let sender = self.update_sender.clone();
        let rt_handle = self.rt.clone();
        let status_arc = self.status.clone();
        let model_name_clone = model_name.to_string();

        let mut current_status = status_arc.lock().unwrap();
        // Allow delete if idle, success, or error. Prevent during pull/list/another delete.
        if !matches!(*current_status, AppStatus::Idle | AppStatus::Success | AppStatus::Error(_)) {
             warn!("Cannot delete model while another operation ({:?}) is in progress.", *current_status);
             let _ = sender.send(UpdateMessage::Log(format!("WARN: Cannot delete model during {:?}.", *current_status)));
             return;
        }
        *current_status = AppStatus::DeletingModel(model_name_clone.clone());
        drop(current_status); // Release lock

        let _ = sender.send(UpdateMessage::StatusText(format!("Deleting model {}...", model_name_clone)));
        info!("Attempting to delete model {}...", model_name_clone);

        rt_handle.spawn(async move {
            match delete_model_async(&model_name_clone, &config, sender.clone()).await {
                Ok(_) => {
                    info!("Successfully deleted model '{}'.", model_name_clone);
                    let _ = sender.send(UpdateMessage::Log(format!("INFO: Successfully deleted model '{}'.", model_name_clone)));
                    let _ = sender.send(UpdateMessage::StatusText("Model deleted successfully.".to_string()));
                    let _ = sender.send(UpdateMessage::Status(AppStatus::Success)); // Set Success status
                }
                Err(e) => {
                    error!("Failed to delete model '{}': {}", model_name_clone, e);
                    let error_message = format!("Error deleting model: {}", e);
                    let _ = sender.send(UpdateMessage::Log(format!("ERROR: Failed to delete model '{}': {}", model_name_clone, e)));
                    let _ = sender.send(UpdateMessage::StatusText(error_message.clone()));
                    let _ = sender.send(UpdateMessage::Status(AppStatus::Error(e.to_string()))); // Set Error status
                }
            }
        });
    }

    // Helper function to draw the collapsible log view content
    fn draw_log_view_content(&mut self, ui: &mut egui::Ui) {
        ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false]) // Allow vertical growth, prevent horizontal shrink
            .show(ui, |ui| {
                // Ensure the label uses the full available width and doesn't center text
                ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                    ui.add(
                        // Use label with RichText for monospace styling
                        egui::Label::new(RichText::new(&self.logs_string_cache).monospace())
                            .wrap_mode(TextWrapMode::Extend) // Prevent wrapping
                    );
                });
            });
    }

    // *** Helper function dedicated to drawing the About window ***
    fn draw_about_window(&mut self, ctx: &egui::Context) {
        // Use a temporary boolean to track if the window should be open *next* frame.
        let mut about_window_open = self.show_about_window;
        let mut close_button_clicked = false;

        Window::new("About LlamaLift")
            .open(&mut about_window_open) // Pass the mutable boolean here
            .collapsible(false)
            .resizable(false)
            .default_size(egui::vec2(350.0, 380.0)) // Keep desired size
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO) // Center the window
            .show(ctx, |ui| {
                // Use vertical_centered layout for the main content block
                ui.vertical_centered(|ui| {
                    ui.add_space(20.0); // Top padding

                    // Display Logo
                    if let Some(texture) = &self.logo_texture {
                        ui.add(egui::Image::new(texture).max_size(egui::vec2(128.0, 128.0)).maintain_aspect_ratio(true));
                    } else {
                        ui.label("[Logo Load Failed]");
                        ui.add_space(128.0); // Reserve space if logo failed
                    }
                    ui.add_space(10.0); // Space below logo

                    // App Name and Version
                    ui.heading(APP_NAME);
                    ui.label(format!("Version: {}", SCRIPT_VERSION));
                    ui.add_space(15.0); // More space before links

                    // Links Section (using standard horizontal layout within the vertical center)
                    ui.horizontal(|ui| {
                        // Center the content *within* this horizontal layout
                        ui.centered_and_justified(|ui| {
                            ui.label("made with <3 by ");
                            ui.hyperlink_to("unbrained", "https://links.unbrained.dev/");
                        });
                    });
                    ui.horizontal(|ui| {
                        // Center the content *within* this horizontal layout
                        ui.centered_and_justified(|ui| {
                            ui.label("Source code on");
                            ui.hyperlink_to("GitHub", "https://github.com/unbraind/LlamaLift");
                        });
                    });

                    ui.add_space(30.0); // More space before close button

                    // Close Button (centered automatically by vertical_centered)
                    if ui.button("Close").clicked() {
                        // Signal that the close button was clicked, don't modify borrowed variable
                        close_button_clicked = true;
                    }
                }); // End vertical_centered

                // Add some padding at the very bottom if needed
                ui.add_space(10.0);
            }); // Window::show ends here, releasing the borrow on about_window_open

        // If the close button was clicked inside the window, update the state *after* the borrow ended
        if close_button_clicked {
            about_window_open = false;
        }

        // Update the application state based on whether the window is still open
        // This handles the case where the user closes the window via the 'X' button OR the 'Close' button.
        self.show_about_window = about_window_open;
    }
}

impl App for OllamaPullerApp {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        info!("Shutting down {}.", APP_NAME);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut trigger_refresh_after_delete = false;
        while let Ok(msg) = self.update_receiver.try_recv() {
            match msg {
                UpdateMessage::Log(log_line) => {
                    let mut logs = self.logs.lock().unwrap();
                    logs.push(log_line);
                    self.logs_dirty = true;
                }
                UpdateMessage::Progress(p) => *self.progress.lock().unwrap() = p,
                UpdateMessage::StatusText(s) => *self.status_text.lock().unwrap() = s,
                UpdateMessage::Status(new_status) => {
                    let mut current_status_lock = self.status.lock().unwrap();
                    if matches!(*current_status_lock, AppStatus::DeletingModel(_)) &&
                       matches!(new_status, AppStatus::Success) &&
                       self.current_view == AppView::ManageModels {
                        trigger_refresh_after_delete = true;
                    }
                    *current_status_lock = new_status;
                },
                UpdateMessage::ModelList(models) => {
                    *self.listed_models.lock().unwrap() = models;
                }
            }
        }

        if trigger_refresh_after_delete {
             info!("Delete succeeded, triggering model list refresh.");
             self.refresh_model_list();
             *self.status_text.lock().unwrap() = "Model list updated.".to_string();
        }

        self.rebuild_log_cache();
        let current_status = self.status.lock().unwrap().clone();
        let is_busy = !matches!(current_status, AppStatus::Idle | AppStatus::Success | AppStatus::Error(_));

        if is_busy || self.logs_dirty {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        if self.copy_logs_requested {
            if !self.logs_string_cache.is_empty() {
                ctx.copy_text(self.logs_string_cache.clone());
                info!("Logs copied to clipboard.");
                let _ = self.update_sender.send(UpdateMessage::Log("INFO: Logs copied to clipboard.".to_string()));
            } else {
                warn!("Log buffer is empty, nothing to copy.");
                let _ = self.update_sender.send(UpdateMessage::Log("WARN: Log buffer is empty, nothing to copy.".to_string()));
            }
            self.copy_logs_requested = false;
        }

        // --- Delete Confirmation Window ---
        let mut cancel_delete = false;
        let mut confirmed_delete = false;
        let mut trigger_actual_delete = None;

        if let Some(model_name) = &self.model_to_delete {
             let mut open = true;
             let model_name_display = model_name.clone();

             Window::new("Confirm Deletion")
                 .collapsible(false)
                 .resizable(false)
                 .open(&mut open)
                 .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                 .show(ctx, |ui| {
                     ui.label(format!("Are you sure you want to permanently delete the model '{}'?", model_name_display));
                     ui.add_space(10.0);
                     ui.horizontal(|ui| {
                         ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                             ui.add_space(10.0);
                             if ui.button(RichText::new("Delete").color(egui::Color32::RED)).clicked() {
                                 confirmed_delete = true;
                             }
                             ui.add_space(10.0);
                             if ui.button("Cancel").clicked() {
                                 cancel_delete = true;
                             }
                         });
                     });
                 });
             if !open { cancel_delete = true; }
        }

        if cancel_delete { self.model_to_delete = None; info!("Model deletion cancelled by user."); }
        else if confirmed_delete {
            if let Some(model_to_delete_name) = self.model_to_delete.take() {
                 trigger_actual_delete = Some(model_to_delete_name);
            }
        }
        if let Some(model_name) = trigger_actual_delete { self.trigger_delete_model(&model_name); }


        // --- Settings Window ---
        let mut settings_window_open = self.show_settings_window;
        // --- CHANGE START: Add flags for settings window buttons ---
        let mut save_and_close_clicked = false;
        let mut cancel_settings_clicked = false;
        // --- CHANGE END ---

        if settings_window_open {
            Window::new("Settings")
                .open(&mut settings_window_open) // Borrow settings_window_open mutably
                .resizable(true)
                .default_width(400.0)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.heading("Runtime Settings");
                    ui.label("These settings override .env/environment variables and are saved persistently.");
                    if let Some(path) = &self.config_path { ui.label(format!("Config file: {}", path.display())); }
                    else { ui.label("Config file path not found."); }
                    ui.separator();
                    Grid::new("settings_grid").num_columns(2).spacing([40.0, 4.0]).striped(true).show(ui, |ui| {
                        ui.label("Ollama Host:"); ui.text_edit_singleline(&mut self.settings.ollama_host); ui.end_row();
                        ui.label("Log Level:"); egui::ComboBox::from_label("").selected_text(&self.settings.log_level).show_ui(ui, |ui| { for level in ["TRACE", "DEBUG", "INFO", "WARN", "ERROR"] { ui.selectable_value(&mut self.settings.log_level, level.to_string(), level); } }); ui.end_row();
                        ui.label("Timezone (IANA):"); ui.text_edit_singleline(&mut self.settings.tz); ui.end_row();
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Save & Close").clicked() {
                            if Tz::from_str(&self.settings.tz).is_err() {
                                error!("Invalid Timezone format: '{}'. Please use IANA format (e.g., 'Europe/Vienna', 'UTC'). Settings not saved.", self.settings.tz);
                                let _ = self.update_sender.send(UpdateMessage::Log(format!("ERROR: Invalid Timezone format: '{}'. Settings not saved.", self.settings.tz)));
                            } else {
                                self.save_settings();
                                // --- CHANGE START: Signal save & close ---
                                save_and_close_clicked = true;
                                // Remove: self.show_settings_window = false;
                                // --- CHANGE END ---
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            // Perform reload immediately
                            match confy::load::<AppSettings>(APP_NAME, None) {
                                Ok(loaded_settings) => { self.settings = loaded_settings; info!("Settings changes cancelled and reloaded from file."); let _ = self.update_sender.send(UpdateMessage::Log("INFO: Settings changes cancelled.".to_string())); }
                                Err(e) => { warn!("Failed to reload settings on cancel: {}. Using current state.", e); let _ = self.update_sender.send(UpdateMessage::Log(format!("WARN: Failed to reload settings on cancel: {}. Keeping current edits.", e))); }
                            }
                            // --- CHANGE START: Signal cancel ---
                            cancel_settings_clicked = true;
                            // Remove: self.show_settings_window = false;
                            // --- CHANGE END ---
                        }
                    });
                    ui.separator();
                    ui.label("Note: Log Level and Timezone changes may require an application restart for the log timestamp format to fully update.");
                }); // Window::show ends here, releasing borrow of settings_window_open

             // --- CHANGE START: Process settings window button signals ---
             if save_and_close_clicked || cancel_settings_clicked {
                 settings_window_open = false;
             }
             // --- CHANGE END ---

             // Handle closing via 'X' button - this needs to run *after* the button flags are checked
             if !settings_window_open && self.show_settings_window {
                 // Only reload if it wasn't closed by Save or Cancel buttons
                 if !save_and_close_clicked && !cancel_settings_clicked {
                     info!("Settings window closed via 'X'. Changes discarded.");
                     if let Ok(loaded_settings) = confy::load(APP_NAME, None) { self.settings = loaded_settings; }
                 }
                 // Always update the state variable if the window is now closed
                 self.show_settings_window = false;
             } else {
                 // Keep the window open if it wasn't closed by any means
                 self.show_settings_window = settings_window_open;
             }
        } // End of `if settings_window_open` block


        // --- About Window (Now calls the helper function) ---
        if self.show_about_window {
            self.draw_about_window(ctx);
        }
        // --- End About Window Section ---


        // --- Top Panel (Menu and View Selection) ---
        TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Settings").clicked() { self.show_settings_window = true; ui.close_menu(); }
                    ui.separator();
                    if ui.button("Quit").clicked() { ctx.send_viewport_cmd(ViewportCommand::Close); }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Copy Logs").clicked() { self.copy_logs_requested = true; ui.close_menu();}
                    ui.separator();
                    if ui.button("About").clicked() {
                        self.show_about_window = true;
                        info!("About button clicked - {} v{}", APP_NAME, SCRIPT_VERSION);
                        ui.close_menu();
                    }
                });
            });
            ui.add_space(4.0);
             ui.horizontal(|ui| {
                 ui.selectable_value(&mut self.current_view, AppView::Download, "Download Models");
                 ui.selectable_value(&mut self.current_view, AppView::ManageModels, "Manage Models");
             });
            ui.add_space(4.0);
            ui.add(Separator::default().spacing(0.0));
        });

        // --- Bottom Panel (Logs) ---
        TopBottomPanel::bottom("log_panel")
            .resizable(true)
            .show_separator_line(true)
            .show(ctx, |ui| {
                let header_response = CollapsingHeader::new("Logs")
                    .default_open(!self.logs_collapsed)
                    .show(ui, |ui| {
                        self.draw_log_view_content(ui);
                    });

                // Keep E0600 fix
                if header_response.header_response.clicked() {
                    self.logs_collapsed = header_response.body_returned.is_none();
                }
                header_response.header_response.on_hover_text("Click to expand/collapse logs");
            });

        // --- Central Panel (Main Content Area) ---
        CentralPanel::default().show(ctx, |ui| {
            match self.current_view {
                 AppView::Download => self.draw_download_view(ui, &current_status),
                 AppView::ManageModels => self.draw_manage_models_view(ui, &current_status),
             }
        });
    }
}

impl OllamaPullerApp {
    // Draws the content for the Download view
    fn draw_download_view(&mut self, ui: &mut egui::Ui, current_status: &AppStatus) {
        let is_pulling = matches!(current_status, AppStatus::Pulling(_, _));
        ui.heading("Download Models"); ui.separator(); ui.label("Enter model identifiers (e.g., 'llama3:latest', 'mistral'):");

        let mut add_new_input = false; let mut remove_index = None; let num_inputs = self.model_inputs.len();

        ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            for i in 0..num_inputs {
                ui.horizontal(|ui| {
                    let text_edit = TextEdit::singleline(&mut self.model_inputs[i]).hint_text("model:tag or model").desired_width(f32::INFINITY);
                    ui.add_enabled(!is_pulling, text_edit);
                    if num_inputs > 1 { if ui.add_enabled(!is_pulling, egui::Button::new("➖").small()).clicked() { remove_index = Some(i); } }
                    if i == num_inputs - 1 && num_inputs < MAX_MODEL_INPUTS {
                        if ui.add_enabled(!is_pulling, egui::Button::new("➕").small()).on_hover_text("Add another model input field").clicked() {
                            add_new_input = true;
                        }
                    }
                });
            }
        });

       if let Some(index) = remove_index { self.model_inputs.remove(index); if self.model_inputs.is_empty() { self.model_inputs.push("".to_string()); } }
       if add_new_input { self.model_inputs.push("".to_string()); }

        ui.add_space(10.0);

        if ui.add_enabled(!is_pulling, egui::Button::new("Download Models")).clicked() {
            let models_to_pull: Vec<String> = self.model_inputs.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| s.to_string()).collect();
            if models_to_pull.is_empty() {
                 error!("No valid model identifiers entered.");
                 let _ = self.update_sender.send(UpdateMessage::Log("ERROR: No valid model identifiers entered.".to_string()));
                 *self.status_text.lock().unwrap() = "Error: No models entered.".to_string();
                 *self.status.lock().unwrap() = AppStatus::Error("No models entered".to_string());
            } else {
                 let num_models = models_to_pull.len();
                 info!("Starting batch pull for {} models.", num_models);
                 let _ = self.update_sender.send(UpdateMessage::Log(format!("INFO: Starting batch pull for {} models.", num_models)));
                 let current_config = self.get_current_config();
                 let sender = self.update_sender.clone();
                 let rt_handle = self.rt.clone();
                 let status_arc = self.status.clone();
                 *status_arc.lock().unwrap() = AppStatus::Pulling(0, num_models);
                 *self.progress.lock().unwrap() = 0.0;
                 rt_handle.spawn(async move {
                     let mut overall_success = true;
                     let mut last_error_msg = String::new();
                     for (index, model_id) in models_to_pull.iter().enumerate() {
                         let status_msg = format!("Pulling model {}/{} ({})", index + 1, num_models, model_id);
                         info!("{}", status_msg);
                         let _ = sender.send(UpdateMessage::Status(AppStatus::Pulling(index + 1, num_models)));
                         let _ = sender.send(UpdateMessage::StatusText(status_msg));
                         let _ = sender.send(UpdateMessage::Progress(0.0));

                         match pull_model_async(model_id, &current_config, sender.clone()).await {
                             Ok(_) => {
                                 info!("Successfully pulled model '{}'.", model_id);
                                 let _ = sender.send(UpdateMessage::Log(format!("INFO: Successfully pulled model '{}'.", model_id)));
                                 tokio::time::sleep(Duration::from_millis(300)).await;
                             }
                             Err(e) => {
                                 error!("Failed to pull model '{}': {}", model_id, e);
                                 let err_log = format!("ERROR: Failed to pull model '{}': {}", model_id, e);
                                 let _ = sender.send(UpdateMessage::Log(err_log));
                                 overall_success = false;
                                 last_error_msg = e.to_string();
                             }
                         }
                     }
                     if overall_success {
                         info!("Batch pull completed successfully.");
                         let _ = sender.send(UpdateMessage::StatusText("Batch pull completed successfully.".to_string()));
                         let _ = sender.send(UpdateMessage::Status(AppStatus::Success));
                         let _ = sender.send(UpdateMessage::Progress(1.0));
                     } else {
                         error!("Batch pull finished with errors.");
                         let final_status_text = format!("Batch pull finished with errors. Last error: {}", last_error_msg);
                         let _ = sender.send(UpdateMessage::StatusText(final_status_text));
                         let _ = sender.send(UpdateMessage::Status(AppStatus::Error(last_error_msg)));
                         let _ = sender.send(UpdateMessage::Progress(0.0));
                     }
                 });
            }
        }
        ui.separator();

        // Show progress bar or status text
        match current_status {
            // Keep unused variable fixes
            AppStatus::Pulling(_current, _total) => {
                let progress_val = *self.progress.lock().unwrap();
                let status_txt = self.status_text.lock().unwrap().clone();
                 let progress_bar_text = format!("{} - {:.1}%", status_txt, progress_val * 100.0);
                 let progress_bar = ProgressBar::new(progress_val).show_percentage().text(progress_bar_text);
                ui.add(progress_bar);
            }
            AppStatus::Error(e) => {
                 ui.colored_label(egui::Color32::RED, format!("Error: {}", e));
            }
            _ => {
                 ui.label(self.status_text.lock().unwrap().clone());
            }
        }
    }

    // Draws the content for the Manage Models view
    fn draw_manage_models_view(&mut self, ui: &mut egui::Ui, current_status: &AppStatus) {
        let is_busy_listing = *current_status == AppStatus::ListingModels;
        let is_busy_deleting = matches!(current_status, AppStatus::DeletingModel(_));
        let is_otherwise_busy = matches!(current_status, AppStatus::Pulling(_, _));
        let is_busy = is_busy_listing || is_busy_deleting || is_otherwise_busy;

        ui.heading("Manage Downloaded Models");
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Models currently available on the server:");
            ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                 if ui.add_enabled(!is_busy, egui::Button::new("🔄 Refresh List")).clicked() { self.refresh_model_list(); }
                 if is_busy_listing { ui.spinner(); ui.label("Listing..."); }
                 else if is_busy_deleting { ui.spinner(); if let AppStatus::DeletingModel(name) = current_status { ui.label(format!("Deleting {}...", name)); } }
                 else if is_otherwise_busy { ui.spinner(); ui.label("Busy downloading..."); }
                 else if matches!(current_status, AppStatus::Error(_)) { ui.colored_label(ui.visuals().error_fg_color, "!"); }
            });
        });
        ui.separator();

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let models = self.listed_models.lock().unwrap();
                if models.is_empty() {
                    if matches!(current_status, AppStatus::Error(_)) { ui.colored_label(ui.visuals().error_fg_color, "Error fetching model list. Check logs and settings."); }
                    else if is_busy_listing { ui.label("Refreshing list..."); }
                    else { ui.label("No models found on the server or list not refreshed yet. Click 'Refresh List'."); }
                } else {
                    Grid::new("models_list_grid").num_columns(4).spacing([20.0, 4.0]).striped(true).show(ui, |ui| {
                        ui.label(RichText::new("Name").strong());
                        ui.label(RichText::new("Size").strong());
                        ui.label(RichText::new("Modified (Local TZ)").strong());
                        ui.label("");
                        ui.end_row();

                        for model in models.iter() {
                            ui.label(&model.name);
                            ui.label(&model.size_human);
                            ui.label(model.modified_local.as_deref().unwrap_or("N/A"));
                            if ui.add_enabled(!is_busy, egui::Button::new("🗑 Delete").small()).clicked() {
                                 self.model_to_delete = Some(model.name.clone());
                                 info!("User initiated delete for model '{}'. Showing confirmation.", model.name);
                             }
                            ui.end_row();
                        }
                    });
                }
            });
    }
}


// --- Async Operations ---
async fn pull_model_async( model_id: &str, config: &Config, sender: Sender<UpdateMessage>,) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let host = if config.ollama_host.starts_with("http://") || config.ollama_host.starts_with("https://") { config.ollama_host.clone() } else { format!("http://{}", config.ollama_host) };
    let url = format!("{}/api/pull", host);
    let request_body = serde_json::json!({ "name": model_id, "stream": true });

    debug!("Sending pull request to {} for model '{}'", url, model_id);
    let _ = sender.send(UpdateMessage::Log(format!("DEBUG: Sending pull request to {} for model '{}'", url, model_id)));

    let res = client.post(&url).json(&request_body).send().await.map_err(|e| { let err_msg = format!("Network request failed for {}: {}", url, e); error!("{}", err_msg); let _ = sender.send(UpdateMessage::Log(format!("ERROR: {}", err_msg))); err_msg })?;

    let status_code = res.status();
    if !status_code.is_success() {
        let error_body = res.text().await.unwrap_or_else(|_| "Unknown server error".to_string());
        error!("Ollama server at {} returned error status {}: {}", host, status_code, error_body);
        let log_msg = format!("ERROR: Ollama server returned error status {}: {}", status_code, error_body);
        let _ = sender.send(UpdateMessage::Log(log_msg.clone()));
        return Err(format!("Server error ({}) from {}: {}", status_code, host, error_body).into());
    }

    let mut stream = res.bytes_stream();
    let mut last_digest = String::new();
    let mut current_total: Option<u64> = None;
    let mut layer_completed: Option<u64> = None;

    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| format!("Stream error while pulling {}: {}", model_id, e))?;
        let lines = String::from_utf8_lossy(&chunk);

        for line in lines.lines() {
            if line.trim().is_empty() { continue; }
            trace!("Raw line from {}: {}", model_id, line);

            match serde_json::from_str::<OllamaPullStatus>(line) {
                Ok(status) => {
                    trace!("[{}] Parsed: {:?}", model_id, status);
                    let log_msg = format!("[{}] {}", model_id, status.status);
                    debug!("{}", log_msg);

                    let _ = sender.send(UpdateMessage::StatusText(status.status.clone()));

                    if status.status.len() < 100 {
                        let _ = sender.send(UpdateMessage::Log(format!("INFO: {}", log_msg)));
                    }

                    if let Some(err_msg) = status.error {
                        error!("Stream error reported for {}: {}", model_id, err_msg);
                        let _ = sender.send(UpdateMessage::Log(format!("ERROR: Stream error: {}", err_msg)));
                    }

                    if let Some(digest) = &status.digest {
                         if *digest != last_digest {
                             last_digest = digest.clone();
                             current_total = status.total;
                             layer_completed = status.completed;
                             debug!("[{}] Starting layer {} (Total: {:?}, Completed: {:?})", model_id, digest, current_total, layer_completed);
                             let _ = sender.send(UpdateMessage::Log(format!("DEBUG: [{}] Starting layer {}...", model_id, digest)));
                             let _ = sender.send(UpdateMessage::Progress(0.0));
                         } else {
                             layer_completed = status.completed;
                             if status.total.is_some() { current_total = status.total; }
                         }
                    } else {
                        if !last_digest.is_empty() {
                            last_digest.clear();
                            current_total = None;
                            layer_completed = None;
                        }
                        let progress = if status.status.contains("success") { 1.0 } else { 0.0 };
                        let _ = sender.send(UpdateMessage::Progress(progress));
                    }

                    if let (Some(completed), Some(total)) = (layer_completed, current_total) {
                        if total > 0 {
                            let progress = completed as f32 / total as f32;
                            trace!("[{}] Layer progress: {} / {} = {}", model_id, completed, total, progress);
                            let _ = sender.send(UpdateMessage::Progress(progress.min(1.0)));
                        } else {
                            let progress = if status.status.contains("pulling") || status.status.contains("downloading") { 0.0 } else { 1.0 };
                            let _ = sender.send(UpdateMessage::Progress(progress));
                        }
                    } else if status.status.contains("success") {
                        trace!("[{}] Step success, progress 1.0", model_id);
                        let _ = sender.send(UpdateMessage::Progress(1.0));
                    }

                }
                Err(e) => {
                    warn!("JSON parse failed for line from {}: '{}'. Error: {}", model_id, line, e);
                    let _ = sender.send(UpdateMessage::Log(format!("WARN: Failed to parse line: {}", line)));
                }
            }
        }
    }
    debug!("Stream finished for model '{}'.", model_id);
    let _ = sender.send(UpdateMessage::Log(format!("DEBUG: Stream finished for model '{}'.", model_id)));
    Ok(())
}

async fn list_models_async( config: &Config, sender: Sender<UpdateMessage>,) -> Result<Vec<OllamaModel>, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let host = if config.ollama_host.starts_with("http://") || config.ollama_host.starts_with("https://") { config.ollama_host.clone() } else { format!("http://{}", config.ollama_host) };
    let url = format!("{}/api/tags", host);
    debug!("Sending list request to {}", url);

    let res = client.get(&url).send().await.map_err(|e| format!("Network request failed for {}: {}", url, e))?;

    let status_code = res.status();
    if !status_code.is_success() {
        let error_body = res.text().await.unwrap_or_else(|_| "Unknown server error".to_string());
        error!("Ollama server at {} returned error status {}: {}", host, status_code, error_body);
        let log_msg = format!("ERROR listing models: Server returned error status {}: {}", status_code, error_body);
        let _ = sender.send(UpdateMessage::Log(log_msg.clone()));
        return Err(format!("Server error ({}) from {}: {}", status_code, host, error_body).into());
    }

    let mut response_body: OllamaTagsResponse = res.json().await.map_err(|e| format!("Failed to parse JSON response from {}: {}", url, e))?;

    let local_tz = config.tz;
    for model in response_body.models.iter_mut() {
        model.size_human = format_size(model.size);
        match DateTime::parse_from_rfc3339(&model.modified_at) {
            Ok(utc_dt) => {
                let local_dt = utc_dt.with_timezone(&local_tz);
                model.modified_local = Some(local_dt.format("%Y-%m-%d %H:%M:%S").to_string());
            }
            Err(e) => {
                warn!("Failed to parse model modified_at date '{}' for model '{}': {}. Using original.", model.modified_at, model.name, e);
                model.modified_local = Some(format!("{} (Parse Failed)", model.modified_at));
            }
        }
    }
    Ok(response_body.models)
}

async fn delete_model_async( model_name: &str, config: &Config, sender: Sender<UpdateMessage>,) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    let host = if config.ollama_host.starts_with("http://") || config.ollama_host.starts_with("https://") { config.ollama_host.clone() } else { format!("http://{}", config.ollama_host) };
    let url = format!("{}/api/delete", host);
    let request_body = OllamaDeleteRequest { name: model_name.to_string() };

    debug!("Sending delete request to {} for model '{}'", url, model_name);
    let _ = sender.send(UpdateMessage::Log(format!("DEBUG: Sending delete request for '{}'", model_name)));

    let res = client.delete(&url).json(&request_body).send().await.map_err(|e| format!("Network request failed for {}: {}", url, e))?;

    let status_code = res.status();
    if status_code.is_success() {
        debug!("Successfully received response for deleting model '{}'.", model_name);
        Ok(())
    } else if status_code == reqwest::StatusCode::NOT_FOUND {
        warn!("Model '{}' not found on server {} during deletion attempt.", model_name, host);
        let _ = sender.send(UpdateMessage::Log(format!("WARN: Model '{}' not found on server.", model_name)));
        Ok(())
    } else {
        let error_body = res.text().await.unwrap_or_else(|_| "Unknown server error".to_string());
        error!("Ollama server at {} returned error status {} deleting model '{}': {}", host, status_code, model_name, error_body);
        let log_msg = format!("ERROR deleting model '{}': Server returned error status {}: {}", model_name, status_code, error_body);
        let _ = sender.send(UpdateMessage::Log(log_msg.clone()));
        Err(format!("Server error ({}) deleting {}: {}", status_code, model_name, error_body).into())
    }
}

// --- Utility Functions ---
fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    const TIB: u64 = GIB * 1024;

    if bytes >= TIB { format!("{:.2} TiB", bytes as f64 / TIB as f64) }
    else if bytes >= GIB { format!("{:.2} GiB", bytes as f64 / GIB as f64) }
    else if bytes >= MIB { format!("{:.2} MiB", bytes as f64 / MIB as f64) }
    else if bytes >= KIB { format!("{:.2} KiB", bytes as f64 / KIB as f64) }
    else { format!("{} B", bytes) }
}

fn load_image_from_bytes(ctx: &egui::Context, name: &str, bytes: &'static [u8]) -> Option<egui::TextureHandle> {
    match image::load_from_memory(bytes) {
        Ok(image) => {
            let size = [image.width() as _, image.height() as _];
            let image_buffer = image.to_rgba8();
            let pixels_u8 = image_buffer.into_raw();

            let pixels_color32: Vec<egui::Color32> = pixels_u8
                .chunks_exact(4)
                .map(|rgba| egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]))
                .collect();

            let color_image = egui::ColorImage {
                size,
                pixels: pixels_color32,
            };

            let image_data = ImageData::Color(Arc::new(color_image));
            let texture_options = TextureOptions::LINEAR;

            Some(ctx.load_texture(name, image_data, texture_options))
        }
        Err(err) => {
            error!("Failed to decode image '{}' from bytes using image crate: {:?}", name, err);
            None
        }
    }
}


// --- Main Function ---
fn main() -> Result<(), eframe::Error> {
    let native_options = eframe::NativeOptions {
         viewport: egui::ViewportBuilder::default()
             .with_inner_size([800.0, 600.0])
             .with_min_inner_size([600.0, 400.0]),
         ..Default::default()
    };

    eframe::run_native(
        APP_NAME,
        native_options,
        Box::new(|cc| Ok(Box::new(OllamaPullerApp::new(cc)))),
    )
}
