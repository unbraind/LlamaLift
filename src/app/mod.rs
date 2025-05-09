// src/app/mod.rs
// Main application logic for LlamaLift. Defines the App struct, implements the eframe::App trait, and coordinates UI, state, configuration, and Ollama interactions, including persistent column widths and temporary settings state.

// Declare sibling modules within the `app` module
pub mod config;
pub mod state;
pub mod ollama;
pub mod ui;
pub mod utils;

// Use necessary external crates
use chrono_tz::Tz;
use confy;
use eframe::{
    egui::{
        self, CentralPanel, CollapsingHeader, Context, Separator, TopBottomPanel, ViewportCommand,
    },
    App, CreationContext,
};
use log::{debug, error, info, warn};
use std::{
    collections::HashSet,
    path::PathBuf,
    str::FromStr,
    sync::{
        mpsc::{Receiver, Sender},
        Arc,
        Mutex,
    },
};
use tokio::runtime::Runtime; // Async runtime

// Use types defined in sibling modules
use self::{
    config::{AppSettings, Config, APP_NAME, SCRIPT_VERSION}, // Import AppSettings and Config
    ollama::OllamaModel,
    state::{
        AppStatus, AppView, ColumnState, ModelColumn, SortDirection, SortState, UpdateMessage,
    },
    ui::{views, windows, widgets},
    utils::{load_image_from_bytes, LOGO_BYTES},
};

// --- Main Application Struct ---

/// Holds the state and logic for the LlamaLift application.
pub struct OllamaPullerApp {
    // --- UI State ---
    model_inputs: Vec<String>,
    logs: Arc<Mutex<Vec<String>>>,
    logs_string_cache: String,
    logs_dirty: bool,
    logs_collapsed: bool,
    show_settings_window: bool,
    show_about_window: bool,
    show_select_columns_window: bool,
    current_view: AppView,
    model_to_delete: Option<String>,
    copy_logs_requested: bool,

    // --- Application State & Data ---
    progress: Arc<Mutex<f32>>,
    status_text: Arc<Mutex<String>>,
    status: Arc<Mutex<AppStatus>>,
    listed_models: Arc<Mutex<Vec<OllamaModel>>>,

    // --- Configuration & Resources ---
    settings: AppSettings,
    config_path: Option<PathBuf>,
    logo_texture: Option<egui::TextureHandle>,

    // --- Table State & Cache ---
    model_column_states: Vec<ColumnState>,
    model_sort_state: SortState,
    manage_view_cache: Vec<OllamaModel>,
    manage_view_cache_dirty: bool,

    // --- Temporary State for Windows ---
    pending_column_states: Option<Vec<ColumnState>>,
    pending_settings: Option<AppSettings>,

    // --- Communication & Async ---
    task_update_sender: Sender<UpdateMessage>, // Sender clone passed from main.rs
    update_receiver: Receiver<UpdateMessage>, // Receiver passed from main.rs
    rt: Arc<Runtime>,
}

// --- Application Implementation ---

impl OllamaPullerApp {
    /// Creates a new instance of LlamaLift.
    pub fn new(
        cc: &CreationContext<'_>,
        task_update_sender: Sender<UpdateMessage>,
        update_receiver: Receiver<UpdateMessage>,
    ) -> Self {
        info!("Running OllamaPullerApp::new - v{}", SCRIPT_VERSION);
        // --- Load Settings ---
        let (mut settings, config_path) = match confy::load::<AppSettings>(APP_NAME, None) {
            Ok(cfg) => {
                info!("Successfully loaded settings from config file.");
                // --- ADDED DEBUG LOG ---
                debug!("Loaded settings from file. Sort State: {:?}", cfg.model_sort_state);
                // --- END DEBUG LOG ---
                (
                    cfg,
                    confy::get_configuration_file_path(APP_NAME, None).ok(),
                )
            }
            Err(e) => {
                warn!(
                    "Failed to load config file ('{}'), using defaults: {}",
                    APP_NAME, e
                );
                let default_settings = AppSettings::default();
                let config_path = confy::get_configuration_file_path(APP_NAME, None).ok();
                // Attempt to store defaults, log error if it fails
                if let Err(store_err) = confy::store(APP_NAME, None, &default_settings) {
                    error!("Failed to store default settings: {}", store_err);
                } else {
                    info!("Stored default settings.");
                }
                (default_settings, config_path)
            }
        };

        // --- Ensure Column States Match Available Columns ---
        // This handles cases where new columns are added to the app
        // but are not yet in the saved config.
        let all_cols_enum: HashSet<ModelColumn> =
            ModelColumn::all().into_iter().collect();
        let current_cols_enum: HashSet<ModelColumn> =
            settings
                .model_column_states
                .iter()
                .map(|cs| cs.column.clone())
                .collect();

        let mut needs_resave = false;
        if all_cols_enum != current_cols_enum {
            warn!("Mismatch between available columns and saved column states. Updating configuration.");
            let mut new_states = Vec::new();
            // Use the public default function from the config module
            let default_states = config::default_column_states();

            // Keep existing states and add new ones with default visibility/width
            // Ensure the order from default_states is maintained
            for default_state in default_states {
                if let Some(existing_state) = settings
                    .model_column_states
                    .iter()
                    .find(|cs| cs.column == default_state.column)
                {
                    new_states.push(existing_state.clone());
                } else {
                    info!("Adding new column state for: {:?}", default_state.column);
                    new_states.push(default_state);
                }
            }
            settings.model_column_states = new_states;
            needs_resave = true;
        }
        // --- End Column State Check ---

        // Log the effective configuration path and settings being used
        if let Some(path) = &config_path {
            info!("Using config file: {}", path.display());
        } else {
            warn!("Could not determine config file path.");
        }
        info!("--- Loaded Persistent Settings ---");
        info!("OLLAMA_HOST: {}", settings.ollama_host);
        info!("LOG_LEVEL: {}", settings.log_level);
        info!("TZ: {}", settings.tz);
        debug!("Column States: {:?}", settings.model_column_states);
        debug!("Sort State (loaded into settings struct): {:?}", settings.model_sort_state);
        info!("--------------------------------");

        // Create the Tokio runtime
        let rt = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to create Tokio runtime"),
        );

        // Load the logo image for the About window
        let logo_texture = load_image_from_bytes(&cc.egui_ctx, "logo", LOGO_BYTES);
        if logo_texture.is_none() {
            error!("Failed to load embedded logo image from '../assets/LlamaLift.png'.");
        } else {
            info!("Successfully loaded embedded logo image.");
        }

        // Perform initial connectivity check
        match std::net::TcpStream::connect(&settings.ollama_host) {
            Ok(_) => info!(
                "Successfully connected to OLLAMA_HOST '{}' on startup.",
                settings.ollama_host
            ),
            Err(e) => {
                let warn_msg = format!("WARN: Could not connect to OLLAMA_HOST '{}' on startup: {}. Check host/port and ensure Ollama is running.", settings.ollama_host, e);
                warn!("{}", warn_msg);
                // Send warning to UI log via the passed sender
                let _ = task_update_sender.send(UpdateMessage::Log(warn_msg));
            }
        }

        // Create the app instance
        let mut app = Self {
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
            show_select_columns_window: false,
            current_view: AppView::ManageModels, // Start on Manage view
            listed_models: Arc::new(Mutex::new(Vec::new())),
            model_to_delete: None,
            copy_logs_requested: false,
            // Load table state from settings
            model_column_states: settings.model_column_states.clone(),
            model_sort_state: settings.model_sort_state.clone(),
            manage_view_cache: Vec::new(), // Initialize cache
            manage_view_cache_dirty: true,  // Mark cache dirty initially
            pending_column_states: None, // Initialize new field
            pending_settings: None, // Initialize pending settings state (NEW)
            settings, // Move settings into the struct
            task_update_sender,
            update_receiver,
            rt,
            config_path,
            logo_texture,
        };
        debug!("Initialized app state with Sort State: {:?}", app.model_sort_state);

        // Resave settings immediately if column states were updated
        if needs_resave {
            app.save_settings();
        }

        // Trigger initial model list refresh if starting on Manage view
        if app.current_view == AppView::ManageModels {
            app.refresh_model_list();
        }

        app // Return the initialized app
    }

    /// Rebuilds the cached log string if the logs are marked as dirty.
    fn rebuild_log_cache(&mut self) {
        if self.logs_dirty {
            let logs_vec = self.logs.lock().unwrap();
            self.logs_string_cache = logs_vec.join("\n");
            self.logs_dirty = false;
        }
    }

    /// Rebuilds the cached and sorted model list for the Manage view if dirty.
    fn rebuild_manage_view_cache(&mut self) {
        // This function is called when the cache is marked dirty
        debug!(
            "Rebuilding manage view cache (Sort: {:?}, Direction: {:?})",
            self.model_sort_state.column, self.model_sort_state.direction
        );

        let mut models = self.listed_models.lock().unwrap().clone();
        let sort_col = &self.model_sort_state.column;
        let sort_dir = &self.model_sort_state.direction;

        models.sort_unstable_by(|a, b| {
            // Use cmp() which returns Ordering directly
            let ordering = match sort_col {
                ModelColumn::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
                ModelColumn::Size => a.size.cmp(&b.size),
                ModelColumn::Modified => a.modified_dt.cmp(&b.modified_dt), // Compare Option<DateTime>
                ModelColumn::Digest => a.digest.cmp(&b.digest),
                ModelColumn::Format => a.details.format.cmp(&b.details.format), // Compare Option<String>
                ModelColumn::Family => a.details.family.cmp(&b.details.family),
                ModelColumn::Families => a.details.families.cmp(&b.details.families), // Compare Option<Vec<String>>
                ModelColumn::ParameterSize => a.details.parameter_size.cmp(&b.details.parameter_size),
                ModelColumn::QuantizationLevel => a.details.quantization_level.cmp(&b.details.quantization_level),
            };

            // Apply direction
            match sort_dir {
                SortDirection::Ascending => ordering,
                SortDirection::Descending => ordering.reverse(),
            }
        });

        self.manage_view_cache = models;
        self.manage_view_cache_dirty = false; // Mark cache as clean after rebuild
        debug!(
            "Manage view cache rebuild complete. Dirty status: {}",
            self.manage_view_cache_dirty
        );
    }

    /// Gets the current runtime configuration based on loaded settings.
    fn get_current_config(&self) -> Config {
        Config {
            ollama_host: self.settings.ollama_host.clone(),
            tz: Tz::from_str(&self.settings.tz).unwrap_or_else(|_| {
                warn!(
                    "Invalid TZ '{}' in settings during runtime config fetch, falling back to UTC.",
                    self.settings.tz
                );
                // Send warning to UI log
                let _ = self.task_update_sender.send(UpdateMessage::Log(format!(
                    "WARN: Invalid TZ '{}', falling back to UTC.",
                    self.settings.tz
                )));
                Tz::UTC
            }),
        }
    }

    /// Saves the current `self.settings` to the persistent configuration file using confy.
    fn save_settings(&mut self) {
        // Changed to &mut self
        // Update the settings struct with the current app state before saving
        self.settings.model_column_states = self.model_column_states.clone();
        self.settings.model_sort_state = self.model_sort_state.clone(); // Ensure latest sort state is copied

        // --- ADDED DEBUG LOG ---
        debug!("Attempting to save settings. Sort State to be saved: {:?}", self.settings.model_sort_state);
        // --- END DEBUG LOG ---

        match confy::store(APP_NAME, None, &self.settings) {
            Ok(_) => {
                info!("Settings saved successfully.");
                let _ = self
                    .task_update_sender
                    .send(UpdateMessage::Log("INFO: Settings saved.".to_string()));
            }
            Err(e) => {
                error!("Failed to save settings: {}", e);
                let _ = self.task_update_sender.send(UpdateMessage::Log(format!(
                    "ERROR: Failed to save settings: {}",
                    e
                )));
            }
        }
        info!("--- Updated Configuration Saved ---");
        info!("OLLAMA_HOST: {}", self.settings.ollama_host);
        info!("LOG_LEVEL: {}", self.settings.log_level);
        info!("TZ: {}", self.settings.tz);
        debug!("Column States: {:?}", self.settings.model_column_states);
        debug!("Sort State: {:?}", self.settings.model_sort_state);
        info!("--------------------------------");
    }

    /// Spawns an asynchronous task to refresh the list of models from the Ollama server.
    fn refresh_model_list(&self) {
        // Keep as &self, state changes happen via messages
        let config = self.get_current_config();
        let sender = self.task_update_sender.clone();
        let rt_handle = self.rt.clone();
        let status_arc = self.status.clone();

        // Use try_lock to avoid blocking UI if lock is held (though unlikely here)
        if let Ok(mut current_status) = status_arc.try_lock() {
            if *current_status == AppStatus::ListingModels {
                warn!("Model list refresh already in progress.");
                return;
            }
            if !matches!(
                *current_status,
                AppStatus::Idle | AppStatus::Success | AppStatus::Error(_)
            ) {
                warn!(
                    "Cannot refresh model list while another operation ({:?}) is in progress.",
                    *current_status
                );
                let _ = sender.send(UpdateMessage::Log(format!(
                    "WARN: Cannot refresh model list during {:?}.",
                    *current_status
                )));
                return;
            }
            // Set status to ListingModels
            *current_status = AppStatus::ListingModels;
        } else {
            warn!("Could not acquire status lock to start refresh.");
            return;
        }
        // Lock is released here

        let _ = sender.send(UpdateMessage::StatusText("Listing models...".to_string()));
        info!("Refreshing model list...");

        rt_handle.spawn(async move {
            match ollama::list_models_async(&config, sender.clone()).await {
                Ok(models) => {
                    info!("Successfully listed {} models.", models.len());
                    let _ = sender.send(UpdateMessage::ModelList(models)); // Send the new list
                    let _ = sender.send(UpdateMessage::StatusText(
                        "Model list updated.".to_string(),
                    ));
                    let _ = sender.send(UpdateMessage::Status(AppStatus::Idle)); // Set status back to Idle
                }
                Err(e) => {
                    error!("Failed to list models: {}", e);
                    let error_message = format!("Error listing models: {}", e);
                    let _ = sender.send(UpdateMessage::StatusText(error_message.clone()));
                    let _ = sender.send(UpdateMessage::Status(AppStatus::Error(e.to_string())));
                }
            }
        });
    }

    /// Spawns an asynchronous task to delete a specified model from the Ollama server.
    fn trigger_delete_model(&self, model_name: &str) {
        // Keep as &self
        let config = self.get_current_config();
        let sender = self.task_update_sender.clone();
        let rt_handle = self.rt.clone();
        let status_arc = self.status.clone();
        let model_name_clone = model_name.to_string();

        // Use try_lock
        if let Ok(mut current_status) = status_arc.try_lock() {
            if !matches!(
                *current_status,
                AppStatus::Idle | AppStatus::Success | AppStatus::Error(_)
            ) {
                warn!(
                    "Cannot delete model while another operation ({:?}) is in progress.",
                    *current_status
                );
                let _ = sender.send(UpdateMessage::Log(format!(
                    "WARN: Cannot delete model during {:?}.",
                    *current_status
                )));
                return;
            }
            *current_status = AppStatus::DeletingModel(model_name_clone.clone());
        } else {
            warn!("Could not acquire status lock to start delete.");
            return;
        }
        // Lock is released

        let _ = sender.send(UpdateMessage::StatusText(format!(
            "Deleting model {}...",
            model_name_clone
        )));
        info!("Attempting to delete model {}...", model_name_clone);

        rt_handle.spawn(async move {
            match ollama::delete_model_async(&model_name_clone, &config, sender.clone()).await {
                Ok(_) => {
                    info!("Successfully deleted model '{}'.", model_name_clone);
                    let _ = sender.send(UpdateMessage::Log(format!(
                        "INFO: Successfully deleted model '{}'.",
                        model_name_clone
                    )));
                    let _ = sender.send(UpdateMessage::StatusText(
                        "Model deleted successfully.".to_string(),
                    ));
                    // Status is set to Success via message, which triggers refresh in update()
                    let _ = sender.send(UpdateMessage::Status(AppStatus::Success));
                }
                Err(e) => {
                    error!("Failed to delete model '{}': {}", model_name_clone, e);
                    let error_message = format!("Error deleting model: {}", e);
                    let _ = sender.send(UpdateMessage::Log(format!(
                        "ERROR: Failed to delete model '{}': {}",
                        model_name_clone, e
                    )));
                    let _ = sender.send(UpdateMessage::StatusText(error_message.clone()));
                    let _ = sender.send(UpdateMessage::Status(AppStatus::Error(e.to_string())));
                }
            }
        });
    }
} // End of impl OllamaPullerApp

// --- eframe::App Implementation ---

impl App for OllamaPullerApp {
    /// Called once before shutdown.
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        info!("Shutting down {}.", APP_NAME);
        // Attempt to save settings on exit (best effort)
        self.save_settings();
    }

    /// Called on each frame to update the UI and handle events.
    fn update(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        let mut trigger_refresh_after_delete = false; // Flag to refresh list after delete
        let mut needs_repaint = false; // Flag to track if repaint is needed this frame

        // Store previous sort/column state *before* any UI interaction or message processing
        let prev_sort_state = self.model_sort_state.clone();
        let prev_column_states = self.model_column_states.clone();
        let previous_view = self.current_view.clone(); // Store previous view

        // --- 1. Process MPSC Messages ---
        let mut messages_to_process = Vec::new();
        while let Ok(msg) = self.update_receiver.try_recv() {
            messages_to_process.push(msg);
        }

        for msg in messages_to_process {
            needs_repaint = true; // Any message likely requires a repaint
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
                    // Check if a delete operation just succeeded
                    if matches!(*current_status_lock, AppStatus::DeletingModel(_))
                        && matches!(new_status, AppStatus::Success)
                    {
                        trigger_refresh_after_delete = true;
                    }
                    *current_status_lock = new_status;
                }
                UpdateMessage::ModelList(models) => {
                    *self.listed_models.lock().unwrap() = models;
                    self.manage_view_cache_dirty = true; // Mark cache dirty when list updates
                }
            }
        }

        // --- 2. Handle Triggered Refresh ---
        if trigger_refresh_after_delete {
            info!("Delete succeeded, triggering model list refresh.");
            self.refresh_model_list();
            *self.status_text.lock().unwrap() = "Model list updated.".to_string();
            needs_repaint = true;
        }

        // --- 3. Handle View Switch ---
        // Check if view switched *before* drawing UI
        if self.current_view != previous_view {
            debug!("View switched from {:?} to {:?}", previous_view, self.current_view);
            needs_repaint = true;
            // Mark cache dirty only if switching *to* ManageModels view
            if self.current_view == AppView::ManageModels {
                debug!("Switched to ManageModels view, marking cache dirty.");
                self.manage_view_cache_dirty = true;
            }
        }

        // --- 4. Rebuild Log Cache ---
        self.rebuild_log_cache(); // Rebuild log cache if necessary

        // --- 5. Handle Other Actions ---
        let current_status = self.status.lock().unwrap().clone();
        let is_busy = !matches!(
            current_status,
            AppStatus::Idle | AppStatus::Success | AppStatus::Error(_)
        );

        if self.copy_logs_requested {
            if !self.logs_string_cache.is_empty() {
                ctx.copy_text(self.logs_string_cache.clone());
                info!("Logs copied to clipboard.");
                let _ = self
                    .task_update_sender
                    .send(UpdateMessage::Log("INFO: Logs copied to clipboard.".to_string()));
            } else {
                warn!("Log buffer is empty, nothing to copy.");
                let _ = self.task_update_sender.send(UpdateMessage::Log(
                    "WARN: Log buffer is empty, nothing to copy.".to_string(),
                ));
            }
            self.copy_logs_requested = false;
            needs_repaint = true;
        }

        // --- 6. Draw UI Elements (Modals, Panels) ---
        // The order matters here. Draw panels first, then modals.

        // Draw Top Panel (Menu, View Selection)
        TopBottomPanel::top("top_panel").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Settings").clicked() {
                        self.show_settings_window = true;
                        needs_repaint = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                });
                ui.menu_button("Help", |ui| {
                    if ui.button("Copy Logs").clicked() {
                        self.copy_logs_requested = true;
                        needs_repaint = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("About").clicked() {
                        self.show_about_window = true;
                        needs_repaint = true;
                        info!("About button clicked - {} v{}", APP_NAME, SCRIPT_VERSION);
                        ui.close_menu();
                    }
                });
            });
            ui.add_space(4.0);
            // View Selection - Allow interaction, state change handled in step 3
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_view, AppView::Download, "Download Models");
                ui.selectable_value(&mut self.current_view, AppView::ManageModels, "Manage Models");
            });
            ui.add_space(4.0);
            ui.add(Separator::default().spacing(0.0));
        });

        // Draw Bottom Panel (Logs)
        TopBottomPanel::bottom("log_panel")
            .resizable(true)
            .show_separator_line(true)
            .show(ctx, |ui| {
                let header_response = CollapsingHeader::new("Logs")
                    .default_open(!self.logs_collapsed)
                    .show(ui, |ui| {
                        widgets::draw_log_view_content(self, ui);
                    });
                // Update collapsed state based on interaction
                if header_response.header_response.clicked() {
                    self.logs_collapsed = header_response.body_returned.is_none();
                    needs_repaint = true;
                }
                header_response
                    .header_response
                    .on_hover_text("Click to expand/collapse logs");
            });

        // Draw Central Panel (Main View Content)
        // This is where manage_models_view might update sort state or column widths
        CentralPanel::default().show(ctx, |ui| {
            match self.current_view {
                AppView::Download => {
                    views::download_view::draw_download_view(self, ui, &current_status);
                }
                AppView::ManageModels => {
                    // Cache rebuild happens inside draw_manage_models_view if needed
                    views::manage_models_view::draw_manage_models_view(self, ui, &current_status);
                }
            }
        });

        // Draw Modals / Separate Windows *after* main panels
        // This allows them to potentially react to state changes from the main UI in the same frame
        let mut select_columns_closed_with_changes = false;
        if self.show_settings_window {
            if self.pending_settings.is_none() {
                info!("Settings window opened, cloning current settings to pending state.");
                self.pending_settings = Some(self.settings.clone());
            }
            windows::settings_window::draw_settings_window(self, ctx);
            if !self.show_settings_window { needs_repaint = true; }
        }
        if self.show_about_window {
            windows::about_window::draw_about_window(self, ctx);
             if !self.show_about_window { needs_repaint = true; }
        }
        if self.show_select_columns_window {
            if self.pending_column_states.is_none() {
                 info!("Select Columns window opened, cloning current column states to pending state.");
                 self.pending_column_states = Some(self.model_column_states.clone());
            }
            windows::select_columns_window::draw_select_columns_window(self, ctx);
            if !self.show_select_columns_window {
                needs_repaint = true;
                // Check if the state *actually* changed when the window closed
                if self.model_column_states != prev_column_states {
                    select_columns_closed_with_changes = true;
                }
            }
        }
        // Handle Delete Confirmation Modal last
        let delete_confirmation_result =
            windows::delete_confirmation_window::draw_delete_confirmation_window(self, ctx);
        if delete_confirmation_result.is_some() { needs_repaint = true; }
        match delete_confirmation_result {
            Some(true) => {
                if let Some(model_to_delete_name) = self.model_to_delete.take() {
                    self.trigger_delete_model(&model_to_delete_name);
                    needs_repaint = true;
                }
            }
            Some(false) => {
                if self.model_to_delete.is_some() {
                    info!("Model deletion cancelled by user.");
                    self.model_to_delete = None;
                    needs_repaint = true;
                }
            }
            None => {}
        }


        // --- 7. Check for State Changes AFTER Drawing ALL UI ---
        // Compare current state with the state stored at the beginning of the frame
        if self.model_sort_state != prev_sort_state {
            debug!("Sort state changed detected after drawing UI.");
            self.save_settings(); // Save state changes immediately
            self.manage_view_cache_dirty = true; // Mark cache dirty
            needs_repaint = true; // Ensure repaint happens
        }

        if self.model_column_states != prev_column_states {
             // Check if it was *only* width that changed, or if visibility/order also changed
             let visibility_changed = self.model_column_states.iter().map(|cs| (&cs.column, cs.visible)).collect::<Vec<_>>() !=
                                      prev_column_states.iter().map(|cs| (&cs.column, cs.visible)).collect::<Vec<_>>();

             if visibility_changed || select_columns_closed_with_changes {
                 // Visibility/order changed (or Select Columns window applied changes)
                 debug!("Column visibility/order changed detected after drawing UI.");
                 self.manage_view_cache_dirty = true; // Mark cache dirty only if visibility/order changed
             } else {
                 // Only width must have changed
                 debug!("Column width changed detected after drawing UI.");
             }
             self.save_settings(); // Save any column state change
             needs_repaint = true; // Ensure repaint happens
        }

        // --- 8. Final Repaint Request ---
        if needs_repaint || is_busy {
             ctx.request_repaint();
        }

    } // End of update function
} // End of impl App

