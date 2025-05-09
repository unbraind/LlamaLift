// src/app/ui/windows/delete_confirmation_window.rs
// Contains the drawing function for the model deletion confirmation window.

// --- Necessary imports ---
use crate::app::OllamaPullerApp;
use egui::{Align2, Color32, Context, Layout, RichText, Window};

// --- Window Drawing Function ---

// Draws the modal confirmation dialog for deleting a model.
//
// # Arguments
//
// * app - Mutable reference to the main application state (OllamaPullerApp).
// * ctx - The egui context (`&egui::Context`).
//
// # Returns
//
// * Some(true) if the user confirmed deletion.
// * Some(false) if the user cancelled deletion (or closed the window).
// * None if the window is not currently supposed to be shown.
pub fn draw_delete_confirmation_window(
    app: &mut OllamaPullerApp,
    ctx: &Context,
) -> Option<bool> {
    let mut result: Option<bool> = None;

    // Check if there is a model queued for deletion confirmation
    if let Some(model_name) = &app.model_to_delete {
        let mut open = true; // Controls window visibility, closing sets it to false
        let model_name_display = model_name.clone(); // Clone for display inside closure

        Window::new("Confirm Deletion")
            .collapsible(false)
            .resizable(false)
            .open(&mut open) // Show the window, allow closing via 'X'
            .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO) // Center the window
            .show(ctx, |ui| {
                ui.label(format!(
                    "Are you sure you want to permanently delete the model '{}'?", // Display model name
                    model_name_display
                ));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    // Layout buttons from right to left
                    ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(10.0); // Spacing on the right - Delete button (styled red)
                        if ui
                            .button(RichText::new("Delete").color(Color32::RED))
                            .clicked()
                        {
                            result = Some(true); // Signal confirmation
                        }
                        ui.add_space(10.0); // Spacing between buttons
                        if ui.button("Cancel").clicked() {
                            result = Some(false);
                        }
                    });
                });
            });

        // If the window was closed (either by 'X' or buttons), treat as cancel unless confirmed
        if !open && result.is_none() {
            result = Some(false); 
        }
    }
    result
}
