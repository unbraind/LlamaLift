// src/app/ui/windows/about_window.rs
// Contains the drawing function for the About window.

// --- Necessary imports ---
use crate::app::{
    config::{APP_NAME, SCRIPT_VERSION},
    OllamaPullerApp,
};
use egui::{Align2, Context, Image, Window};

// --- Window Drawing Function ---

// Draws the "About" window.
//
// # Arguments
//
// * app - Mutable reference to the main application state (OllamaPullerApp).
// * ctx - The egui context (&egui::Context).
pub fn draw_about_window(app: &mut OllamaPullerApp, ctx: &Context) {
    let mut about_window_open = app.show_about_window;
    let mut close_button_clicked = false;

    Window::new("About LlamaLift")
        .open(&mut about_window_open)
        .collapsible(false)
        .resizable(false)
        .default_size(egui::vec2(350.0, 380.0))
        .anchor(Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(20.0); // Top padding

                // Display Logo
                if let Some(texture) = &app.logo_texture {
                    // Use the texture handle stored in the app state
                    ui.add(
                        Image::new(texture)
                            .max_size(egui::vec2(128.0, 128.0)) // Limit logo size
                            .maintain_aspect_ratio(true), // Keep aspect ratio
                    );
                } else {
                    // Fallback text if logo loading failed
                    ui.label("[Logo Load Failed]");
                    ui.add_space(128.0);
                }
                ui.add_space(10.0); // Space below logo

                // App Name and Version
                ui.heading(APP_NAME);
                ui.label(format!("Version: {}", SCRIPT_VERSION));
                ui.add_space(15.0); // More space before links

                // Links Section
                ui.horizontal(|ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label("made with <3 by ");
                        ui.hyperlink_to("unbrained", "https://links.unbrained.dev/");
                    });
                });
                ui.horizontal(|ui| {
                    ui.centered_and_justified(|ui| {
                        ui.label("Source code on");
                        ui.hyperlink_to("GitHub", "https://github.com/unbraind/LlamaLift");
                    });
                });

                ui.add_space(30.0);

                // Close Button
                if ui.button("Close").clicked() {
                    close_button_clicked = true;
                }
            });
            ui.add_space(10.0);
        });

    // --- Post-Window Logic ---
    if close_button_clicked {
        about_window_open = false;
    }
    app.show_about_window = about_window_open;
}
