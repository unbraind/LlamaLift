// src/app/ui/widgets.rs
// Contains drawing functions for reusable UI widgets, such as the log view content area.

use crate::app::OllamaPullerApp; // Import main application state struct
use egui::{Align, Layout, RichText, ScrollArea, TextWrapMode, Ui}; // egui components

// --- Widget Drawing Functions ---

// Draws the content area for the collapsible log view.
// This function is typically called within a CollapsingHeader.
//
// # Arguments
//
// * app - Mutable reference to the main application state (still OllamaPullerApp - I should change that at some point). TODO STEVE
// * ui - Mutable reference to the egui UI context for drawing.
pub fn draw_log_view_content(app: &mut OllamaPullerApp, ui: &mut Ui) {
    // Use a vertical ScrollArea to contain the logs
    ScrollArea::vertical()
        .stick_to_bottom(true)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Ensure the label uses the full available width and doesn't center text
            ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
                // Add the log content as a single Label using RichText for monospace styling
                ui.add(
                    egui::Label::new(RichText::new(&app.logs_string_cache).monospace())
                        .wrap_mode(TextWrapMode::Extend),
                );
            });
        });
}
