// src/app/ui/views/manage_models_view.rs
// Contains the UI drawing function for the Manage Models view.

use crate::app::{
    state::{AppStatus, ColumnState, ModelColumn, SortDirection}, // Removed UpdateMessage
    OllamaPullerApp,
};
use egui::{
    Button, Layout, RichText, Ui,
};
use egui_extras::{Column, TableBuilder};
use log::{debug, info};

// --- View Drawing Functions ---

// Draws the content for the "Manage Models" view using egui_extras::TableBuilder.
// Includes sorting, column visibility controls, and persistent column widths.
//
// # Arguments
//
// * app - Mutable reference to the main application state (OllamaPullerApp).
// * ui - Mutable reference to the egui UI context for drawing.
// * current_status - The current application status (AppStatus).
pub fn draw_manage_models_view(
    app: &mut OllamaPullerApp,
    ui: &mut Ui,
    current_status: &AppStatus,
) {
    let is_busy_listing = *current_status == AppStatus::ListingModels;
    let is_busy_deleting = matches!(current_status, AppStatus::DeletingModel(_));
    let is_otherwise_busy = matches!(current_status, AppStatus::Pulling(_, _));
    let is_busy = is_busy_listing || is_busy_deleting || is_otherwise_busy;

    ui.heading("Manage Downloaded Models");
    ui.separator();

    // Header row with label and refresh button/status indicators
    ui.horizontal(|ui| {
        ui.label("Models currently available on the server:");
        // Layout elements from right-to-left for the right side of the header
        ui.with_layout(Layout::right_to_left(egui::Align::Center), |ui| {
            // Refresh button
            if ui
                .add_enabled(!is_busy, Button::new("🔄 Refresh List"))
                .clicked()
            {
                app.refresh_model_list(); // Trigger refresh action
            }
            // Display spinners and status text based on current activity
            if is_busy_listing {
                ui.spinner();
                ui.label("Listing...");
            } else if is_busy_deleting {
                ui.spinner();
                if let AppStatus::DeletingModel(name) = current_status {
                    ui.label(format!("Deleting {}...", name));
                }
            } else if is_otherwise_busy {
                ui.spinner();
                ui.label("Busy downloading..."); // Indicate pull is in progress
            } else if let AppStatus::Error(e) = current_status {
                // Show an error indicator if the last operation failed
                ui.colored_label(ui.visuals().error_fg_color, "!")
                    .on_hover_text(format!("Error: {}. Check logs.", e));
            }
        });
    });
    ui.separator();

    // --- Display Empty/Loading State OR Table ---

    // Check the *original* list before caching/sorting for empty/error state
    let models_lock = app.listed_models.lock().unwrap();
    let original_list_is_empty = models_lock.is_empty();
    drop(models_lock); // Release lock immediately

    if original_list_is_empty {
        // Display messages if the list is empty based on the current status
        if matches!(current_status, AppStatus::Error(_)) {
            ui.colored_label(
                ui.visuals().error_fg_color,
                "Error fetching model list. Check logs and settings.",
            );
        } else if is_busy_listing {
            ui.label("Refreshing list...");
        } else {
            ui.label(
                "No models found on the server or list not refreshed yet. Click 'Refresh List'.",
            );
        }
    } else {
        // --- Rebuild Cache if Dirty (Moved Here) ---
        // This needs to happen *before* we borrow app.model_column_states immutably
        if app.manage_view_cache_dirty {
             debug!("Rebuilding manage view cache before drawing table...");
             app.rebuild_manage_view_cache(); // Rebuild cache now
             // Cache clean status is set inside rebuild_manage_view_cache
        }
        // --- End Cache Rebuild ---

        // --- Build the Table ---
        // Now we can safely hopefully borrow immutably
        let visible_columns: Vec<&ColumnState> = app
            .model_column_states
            .iter()
            .filter(|cs| cs.visible)
            .collect();

        debug!( // Log after potential rebuild
            "Drawing manage models view. Cache dirty: {}, Cache len: {}",
            app.manage_view_cache_dirty, // Should be false now if rebuilt
            app.manage_view_cache.len()
        );

        let num_visible_data_columns = visible_columns.len();
        let delete_button_width = 60.0;
        let default_column_width = 120.0;

        // Calculate row height *before* the TableBuilder borrows ui mutably
        let row_height = ui.text_style_height(&egui::TextStyle::Body);

        // Temporary variable to store the column to hide after the table interaction
        let mut column_to_hide: Option<ModelColumn> = None;

        // --- Define Table Columns with Widths ---
        // Give the table a unique ID for egui's state persistence
        let table_id = egui::Id::new("manage_models_table");
        // Use id_salt instead of id_source
        let mut builder = TableBuilder::new(ui).id_salt(table_id);

        for col_state in &visible_columns { // Iterate through visible columns directly
            let initial_width = col_state.width.unwrap_or(default_column_width);
            // Column doesn't take an ID source/salt directly.
            // The TableBuilder uses the table's salt and column index for state.
            builder = builder.column(Column::initial(initial_width).resizable(true));
        }
        builder = builder.column(Column::exact(delete_button_width));

        // --- Build the Table Header and Body ---
        // Capture the response from the table builder
        let _table_response = builder
            .striped(true)
            .resizable(true)
            .header(20.0, |mut header| {
                // Iterate through VISIBLE columns to draw headers
                for col_state in &visible_columns {
                    let column_enum = &col_state.column; // Get the enum variant
                    header.col(|ui| {
                        ui.horizontal_centered(|ui| {
                            let response = ui.add_enabled(!is_busy, Button::new(RichText::new(column_enum.display_name()).strong()))
                                .on_hover_text(format!("Sort by {}", column_enum.display_name()));

                            // Only update state here, rebuild/repaint handled in mod.rs
                            if response.clicked() {
                                info!("Header clicked for column: {:?}", column_enum); // Log click
                                if app.model_sort_state.column == *column_enum {
                                    app.model_sort_state.direction = match app.model_sort_state.direction {
                                        SortDirection::Ascending => SortDirection::Descending,
                                        SortDirection::Descending => SortDirection::Ascending,
                                    };
                                } else {
                                    app.model_sort_state.column = column_enum.clone();
                                    app.model_sort_state.direction = SortDirection::Ascending;
                                }
                                info!("Sort state changed to: {:?}, {:?}", app.model_sort_state.column, app.model_sort_state.direction);
                            }

                            response.context_menu(|ui| {
                                let can_hide = num_visible_data_columns > 1;
                                if ui.add_enabled(can_hide, Button::new("Hide column")).clicked() {
                                    // Store the column to hide, don't modify state here
                                    column_to_hide = Some(column_enum.clone());
                                    ui.close_menu();
                                }
                                if ui.button("Select columns...").clicked() {
                                    app.show_select_columns_window = true;
                                    // Clone current state into pending state if window opens
                                    if app.pending_column_states.is_none() {
                                        app.pending_column_states = Some(app.model_column_states.clone());
                                        info!("'Select columns...' clicked, initializing pending state."); // Debug log
                                    }
                                    ui.close_menu();
                                }
                            });

                            if app.model_sort_state.column == *column_enum {
                                // --- Reverted sort indicators ---
                                ui.label(match app.model_sort_state.direction {
                                    SortDirection::Ascending => " ^",
                                    SortDirection::Descending => " v",
                                });
                            }
                        });
                    });
                }
                header.col(|ui| { ui.label(""); }); // Empty header for delete column
            })
            .body(|body| {
                // Cache should be clean here because we rebuilt it above if it was dirty
                let models_to_display = &app.manage_view_cache; // Borrow the clean cache

                body.rows(row_height, models_to_display.len(), |mut row| {
                    let row_index = row.index();
                    let model = &models_to_display[row_index];

                    // Iterate through VISIBLE column states
                    for col_state in &visible_columns { // Use the immutable borrow here
                        let column_enum = &col_state.column; // Get the enum variant
                        row.col(|ui| {
                            let text = match column_enum {
                                ModelColumn::Name => model.name.clone(),
                                ModelColumn::Size => model.size_human.clone(),
                                ModelColumn::Modified => model.modified_local.clone().unwrap_or_else(|| "N/A".to_string()),
                                ModelColumn::Digest => model.digest.chars().take(12).collect::<String>() + "...",
                                ModelColumn::Format => model.details.format.clone().unwrap_or_else(|| "-".to_string()),
                                ModelColumn::Family => model.details.family.clone().unwrap_or_else(|| "-".to_string()),
                                ModelColumn::Families => model.details.families.as_ref().map(|v| v.join(", ")).unwrap_or_else(|| "-".to_string()),
                                ModelColumn::ParameterSize => model.details.parameter_size.clone().unwrap_or_else(|| "-".to_string()),
                                ModelColumn::QuantizationLevel => model.details.quantization_level.clone().unwrap_or_else(|| "-".to_string()),
                            };

                            if *column_enum == ModelColumn::Digest {
                                ui.label(text).on_hover_text(&model.digest);
                            } else {
                                ui.label(text);
                            }
                        });
                    }
                    // Cell for the delete button
                    row.col(|ui| {
                        if ui.add_enabled(!is_busy, Button::new("🗑").small())
                           .on_hover_text("Delete Model")
                           .clicked()
                        {
                            app.model_to_delete = Some(model.name.clone());
                            info!("User initiated delete for model '{}'. Showing confirmation.", model.name);
                        }
                    });
                });
            }); // End TableBuilder

        // --- Width Capture Logic ---
        for col_state in app.model_column_states.iter_mut() {
             // Generate the ID egui uses for this column's state within the table
             let column_id = table_id.with(&col_state.column);
             debug!("Checking width for column {:?} using ID: {:?}", col_state.column, column_id);
             // Try to get the width stored by egui for this specific column ID
             if let Some(new_width) = ui.memory(|m| m.data.get_temp::<f32>(column_id)) {
                 let should_update = match col_state.width {
                     Some(old_w) => (new_width - old_w).abs() > 0.1,
                     None => true,
                 };
                 if should_update {
                     debug!(
                         "Detected width change for column {:?}: {:?} -> {}",
                         col_state.column, col_state.width, new_width
                     );
                     col_state.width = Some(new_width);
                     // Saving is handled in app/mod.rs::update by comparing state vectors
                 }
             } else {
                 debug!("No width found in memory for column ID: {:?}", column_id);
             }
         }
        // --- End Width Capture ---


        // --- Apply Deferred State Changes (Column Hiding) ---
        // Apply column hiding *after* the table UI is finished drawing and widths captured
        if let Some(col_to_hide) = column_to_hide {
            if let Some(state) = app.model_column_states.iter_mut().find(|cs| cs.column == col_to_hide) {
                state.visible = false;
                info!("Hiding column: {:?}", col_to_hide);
                // Cache dirty marking and saving handled in app/mod.rs
            }
        }
    }
}
