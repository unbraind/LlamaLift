// src/app/ui/windows/select_columns_window.rs
// Contains the drawing function for the Select Columns window used in the Manage Models view.

// --- Necessary imports ---
use crate::app::{
    state::UpdateMessage,
    OllamaPullerApp,
};
use egui::{Align, Align2, Context, Layout, ScrollArea, Window};
use log::{error, info};

// --- Window Drawing Function ---

// Draws the "Select Columns" window for the Manage Models table.
// Allows users to toggle the visibility of columns.
// Uses the app.pending_column_states field to manage temporary state.
//
// # Arguments
//
// * app - Mutable reference to the main application state (OllamaPullerApp).
// * ctx - The egui context (&egui::Context).
pub fn draw_select_columns_window(app: &mut OllamaPullerApp, ctx: &Context) {
    let mut window_open = app.show_select_columns_window; // Control window visibility
    let mut ok_clicked = false;
    let mut cancel_clicked = false;

    Window::new("Select Columns")
        .open(&mut window_open)
        .resizable(false)
        .collapsible(false)
        .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            if app.pending_column_states.is_none() {
                error!("Select Columns window drawn without pending state initialized!");
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    "Internal error: State not initialized.",
                );
                return;
            }

            ui.label("Select columns to display:");
            ui.separator();

            // Scrollable area for the checkboxes
            ScrollArea::vertical()
                .max_height(300.0) // Limit height
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if let Some(pending_states) = app.pending_column_states.as_mut() {
                        for col_state in pending_states.iter_mut() {
                            ui.checkbox(&mut col_state.visible, col_state.column.display_name());
                        }
                    }
                });

            ui.separator();
            // Action buttons (OK, Cancel) at the bottom
            ui.horizontal(|ui| {
                // Layout buttons from right to left
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        cancel_clicked = true;
                    }
                    if ui.button("OK").clicked() {
                        ok_clicked = true;
                    }
                });
            });
        });

    // --- Post-Window Logic ---
    if ok_clicked {
        // Apply the changes: move state from pending to actual, clear pending
        if let Some(pending_states) = app.pending_column_states.take() {
            // Use take()
            app.model_column_states = pending_states; // Apply the changes
            info!("Column selection updated.");
            let _ = app
                .task_update_sender
                .send(UpdateMessage::Log("INFO: Column selection updated.".to_string()));
        } else {
            error!("OK clicked but pending_column_states was None!");
        }
        window_open = false; // Close window
        app.pending_column_states = None; // Ensure cleared
    } else if cancel_clicked {
        // Discard changes: clear pending state
        info!("Column selection cancelled.");
        let _ = app
            .task_update_sender
            .send(UpdateMessage::Log("INFO: Column selection cancelled.".to_string()));
        window_open = false; // Close window
        app.pending_column_states = None; // Clear pending state
    }

    // Handle closing via 'X'
    if !window_open && app.show_select_columns_window && !ok_clicked && !cancel_clicked {
        info!("Select Columns window closed via 'X'. Changes discarded.");
        let _ = app.task_update_sender.send(UpdateMessage::Log(
            "INFO: Select Columns window closed via 'X'. Changes discarded.".to_string(),
        ));
        app.pending_column_states = None; // Clear pending state on 'X' close too
    }

    // Update the application state variable controlling window visibility
    app.show_select_columns_window = window_open;

    // Final safety check: if the window is supposed to be closed, ensure pending state is None
    if !app.show_select_columns_window {
        app.pending_column_states = None;
    }
}
