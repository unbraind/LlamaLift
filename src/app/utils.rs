// src/app/utils.rs
// Contains utility functions and constants for LlamaLift, such as size formatting and image loading.

use egui::{ColorImage, Context, ImageData, TextureHandle, TextureOptions};
use image;
use log::error;
use std::sync::Arc;

// --- Constants ---
pub const LOGO_BYTES: &[u8] = include_bytes!("../../assets/LlamaLift.png");

// --- Utility Functions ---
pub fn format_size(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;
    const TIB: u64 = GIB * 1024;

    if bytes >= TIB {
        format!("{:.2} TiB", bytes as f64 / TIB as f64)
    } else if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{} B", bytes) // Base case: Bytes
    }
}

pub fn load_image_from_bytes(
    ctx: &Context,
    name: &str,
    bytes: &'static [u8],
) -> Option<TextureHandle> {
    match image::load_from_memory(bytes) {
        Ok(image) => {
            // Get image dimensions
            let size = [image.width() as _, image.height() as _];
            let image_buffer = image.to_rgba8();
            let pixels_u8 = image_buffer.into_raw();

            let pixels_color32: Vec<egui::Color32> = pixels_u8
                .chunks_exact(4)
                .map(|rgba| egui::Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]))
                .collect();

            // Create an egui ColorImage
            let color_image = ColorImage {
                size,
                pixels: pixels_color32,
            };

            // Wrap the ColorImage in ImageData
            let image_data = ImageData::Color(Arc::new(color_image));
            let texture_options = TextureOptions::LINEAR;

            // Load the image data into the egui context as a texture
            Some(ctx.load_texture(name, image_data, texture_options))
        }
        Err(err) => {
            // Log an error if image loading/decoding fails
            error!(
                "Failed to decode image '{}' from bytes using image crate: {:?}",
                name, err
            );
            None
        }
    }
}
