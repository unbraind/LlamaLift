// src/app/ui/views/download_view.rs
// Contains the UI drawing function for the Download Models view.

use crate::app::{
    config::MAX_MODEL_INPUTS,
    state::{AppStatus, UpdateMessage},
    OllamaPullerApp,
};
use egui::{
    Button, ProgressBar, ScrollArea, TextEdit, Ui,
};
use log::{error, info};
use std::time::Duration;

// --- View Drawing Functions ---

// Draws the content for the "Download Models" view.
//
// # Arguments
//
// * app - Mutable reference to the main application state (OllamaPullerApp).
// * ui - Mutable reference to the egui UI context for drawing.
// * current_status - The current application status (AppStatus).
pub fn draw_download_view(app: &mut OllamaPullerApp, ui: &mut Ui, current_status: &AppStatus) {
    // Check if the application is currently in a pulling state
    let is_pulling = matches!(current_status, AppStatus::Pulling(_, _));

    ui.heading("Download Models");
    ui.separator();
    ui.label("Enter model identifiers (e.g., 'llama3:latest', 'mistral'):");

    let mut add_new_input = false;
    let mut remove_index = None;
    let num_inputs = app.model_inputs.len();

    // Scrollable area for model input fields
    ScrollArea::vertical()
        .auto_shrink([false, true]) // Allow vertical growth, shrink horizontally if needed
        .show(ui, |ui| {
            for i in 0..num_inputs {
                ui.horizontal(|ui| {
                    // Text input field for model identifier
                    let text_edit = TextEdit::singleline(&mut app.model_inputs[i])
                        .hint_text("model:tag or model"); // Placeholder text
                    // Disable input field if pulling is in progress
                    ui.add_enabled(!is_pulling, text_edit);

                    // Add remove button (-) if more than one input field exists
                    if num_inputs > 1 {
                        if ui
                            .add_enabled(!is_pulling, Button::new("➖").small())
                            .clicked()
                        {
                            remove_index = Some(i);
                        }
                    }
                    // Add add button (+) to the last input field if limit not reached
                    if i == num_inputs - 1 && num_inputs < MAX_MODEL_INPUTS {
                        if ui
                            .add_enabled(!is_pulling, Button::new("➕").small())
                            .on_hover_text("Add another model input field")
                            .clicked()
                        {
                            add_new_input = true;
                        }
                    }
                });
            }
        });

    // Process remove/add actions after iterating
    if let Some(index) = remove_index {
        app.model_inputs.remove(index);
        // Ensure there's always at least one input field
        if app.model_inputs.is_empty() {
            app.model_inputs.push("".to_string());
        }
    }
    if add_new_input {
        app.model_inputs.push("".to_string());
    }

    ui.add_space(10.0); // Spacing

    // "Download Models" button
    if ui
        .add_enabled(!is_pulling, Button::new("Download Models"))
        .clicked()
    {
        // Collect valid, non-empty model identifiers from input fields
        let models_to_pull: Vec<String> = app
            .model_inputs
            .iter()
            .map(|s| s.trim()) // Trim whitespace
            .filter(|s| !s.is_empty()) // Filter out empty strings
            .map(|s| s.to_string())
            .collect();

        if models_to_pull.is_empty() {
            // Handle case where no valid models were entered
            error!("No valid model identifiers entered.");
            // Use task_update_sender
            let _ = app.task_update_sender.send(UpdateMessage::Log(
                "ERROR: No valid model identifiers entered.".to_string(),
            ));
            *app.status_text.lock().unwrap() = "Error: No models entered.".to_string();
            *app.status.lock().unwrap() = AppStatus::Error("No models entered".to_string());
        } else {
            // Start the batch pull process
            let num_models = models_to_pull.len();
            info!("Starting batch pull for {} models.", num_models);
            // Use task_update_sender
            let _ = app.task_update_sender.send(UpdateMessage::Log(format!(
                "INFO: Starting batch pull for {} models.",
                num_models
            )));

            // Get necessary resources for the async task
            let current_config = app.get_current_config();
            let sender = app.task_update_sender.clone(); // Clone sender for the task
            let rt_handle = app.rt.clone(); // Clone Tokio runtime handle
            let status_arc = app.status.clone(); // Clone Arc for status

            // Set initial status for pulling
            // Use 1-based indexing for UI display (current model number)
            *status_arc.lock().unwrap() = AppStatus::Pulling(1, num_models);
            *app.progress.lock().unwrap() = 0.0; // Reset progress

            // Spawn the asynchronous task to perform the pull
            rt_handle.spawn(async move {
                let mut overall_success = true; // Track if all pulls succeed
                let mut last_error_msg = String::new(); // Store the last error message

                // Iterate through models and pull them sequentially
                for (index, model_id) in models_to_pull.iter().enumerate() {
                    // Use 1-based index for status messages and progress calculation
                    let current_model_num = index + 1;
                    let status_msg = format!(
                        "Pulling model {}/{} ({})",
                        current_model_num, num_models, model_id
                    );
                    info!("{}", status_msg); // Log start of individual pull

                    // Send updates to UI thread
                    let _ = sender.send(UpdateMessage::Status(AppStatus::Pulling(
                        current_model_num, // Update current model index (1-based)
                        num_models,
                    )));
                    // Send the specific model name being pulled as status text
                    let _ = sender.send(UpdateMessage::StatusText(format!(
                        "Pulling: {}",
                        model_id
                    )));
                    let _ = sender.send(UpdateMessage::Progress(0.0)); // Reset progress for this model

                    // Call the async pull function
                    match crate::app::ollama::pull_model_async(
                        model_id,
                        &current_config,
                        sender.clone(),
                    )
                    .await
                    {
                        Ok(_) => {
                            // Handle successful pull
                            info!("Successfully pulled model '{}'.", model_id);
                            let _ = sender.send(UpdateMessage::Log(format!(
                                "INFO: Successfully pulled model '{}'.",
                                model_id
                            )));
                            // Set progress to 1.0 explicitly for the completed model before delay
                            // This ensures the overall progress bar updates correctly
                            if current_model_num < num_models {
                                let _ = sender.send(UpdateMessage::Progress(1.0));
                            }
                            tokio::time::sleep(Duration::from_millis(300)).await;
                        }
                        Err(e) => {
                            // Handle failed pull
                            error!("Failed to pull model '{}': {}", model_id, e);
                            let err_log =
                                format!("ERROR: Failed to pull model '{}': {}", model_id, e);
                            let _ = sender.send(UpdateMessage::Log(err_log));
                            overall_success = false; // Mark batch as failed
                            last_error_msg = e.to_string(); // Store error message
                        }
                    }
                }

                // Update final status after batch completes
                if overall_success {
                    info!("Batch pull completed successfully.");
                    let _ = sender.send(UpdateMessage::StatusText(
                        "Batch pull completed successfully.".to_string(),
                    ));
                    let _ = sender.send(UpdateMessage::Status(AppStatus::Success));
                    let _ = sender.send(UpdateMessage::Progress(1.0)); // Set progress to 100%
                } else {
                    error!("Batch pull finished with errors.");
                    let final_status_text = format!(
                        "Batch pull finished with errors. Last error: {}",
                        last_error_msg
                    );
                    let _ = sender.send(UpdateMessage::StatusText(final_status_text));
                    let _ = sender.send(UpdateMessage::Status(AppStatus::Error(last_error_msg)));
                    let _ = sender.send(UpdateMessage::Progress(0.0)); // Reset progress on error
                }
            });
        }
    }
    ui.separator();

    // Display progress bar or status text based on current status
    match current_status {
        AppStatus::Pulling(current, total) => {
            // current is 1-based index (1 to total)
            // progress_val is the progress of the current model (0.0 to 1.0)
            let progress_val = *app.progress.lock().unwrap();
            // Status text now comes from the stream (e.g., "pulling fs layer...")
            let status_txt = app.status_text.lock().unwrap().clone();

            // --- Overall Progress ---
            // Calculate overall progress: (models_done + current_model_progress) / total_models
            let overall_progress = ((*current - 1) as f32 + progress_val) / (*total as f32);
            let overall_text = format!(
                "Overall Progress (Model {}/{}) - {:.1}%",
                current,
                total,
                overall_progress * 100.0
            );
            let overall_progress_bar = ProgressBar::new(overall_progress.min(1.0)) // Cap at 1.0
                // .show_percentage() // Percentage is in the text now
                .text(overall_text);
            // Add the overall progress bar, constraining its size
            ui.add_sized(
                [ui.available_width(), ui.spacing().interact_size.y],
                overall_progress_bar,
            );

            // --- Current Item Progress (Only if more than one model total) ---
            if *total > 1 {
                ui.add_space(4.0); // Add some space between the bars

                let current_item_text = format!("{} - {:.1}%", status_txt, progress_val * 100.0);
                let current_item_progress_bar = ProgressBar::new(progress_val)
                    // .show_percentage() // Percentage is in the text now
                    .text(current_item_text);
                // Add the current item progress bar, constraining its size
                ui.add_sized(
                    [ui.available_width(), ui.spacing().interact_size.y],
                    current_item_progress_bar,
                );
            }
        }
        AppStatus::Error(e) => {
            // Display error message in red
            ui.colored_label(ui.visuals().error_fg_color, format!("Error: {}", e));
        }
        AppStatus::Success => {
            // Display success message and maybe a full progress bar
            ui.label(app.status_text.lock().unwrap().clone());
            let progress_bar = ProgressBar::new(1.0)
                .show_percentage()
                .text("Completed");
            ui.add_sized(
                [ui.available_width(), ui.spacing().interact_size.y],
                progress_bar,
            );
        }
        _ => {
            ui.label(app.status_text.lock().unwrap().clone());
        }
    }
}
