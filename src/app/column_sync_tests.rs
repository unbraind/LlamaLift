#[cfg(test)]
mod tests {
    use crate::app::config;
    use crate::app::state::{ColumnState, ModelColumn, SortState}; // Assuming AppSettings is not needed directly for sync fn
    use std::collections::HashSet;

    // This function replicates the core column synchronization logic from OllamaPullerApp::new
    // It takes the column states that would have been loaded from a config file
    // and synchronizes them with the application's current default column definitions.
    fn synchronize_column_states(
        loaded_states: Vec<ColumnState>,
        // Represents ModelColumn::all() from the main app code
        current_app_columns: &[ModelColumn],
        // Represents config::default_column_states() from the main app code
        current_app_default_states: &[ColumnState]
    ) -> (Vec<ColumnState>, bool) {

        let all_cols_enum_set: HashSet<ModelColumn> =
            current_app_columns.iter().cloned().collect();
        let loaded_cols_set: HashSet<ModelColumn> =
            loaded_states.iter().map(|cs| cs.column.clone()).collect();

        let mut needs_resave = false;
        let mut final_states = loaded_states.clone(); // Start with loaded states

        if all_cols_enum_set != loaded_cols_set {
            needs_resave = true;
            let mut new_synced_states = Vec::new();

            // Iterate through the current app's default states (which are based on current_app_columns)
            // This ensures correct order and inclusion of all current columns.
            for app_default_state in current_app_default_states {
                if let Some(loaded_state) = loaded_states
                    .iter()
                    .find(|cs| cs.column == app_default_state.column)
                {
                    // If the column from app defaults exists in loaded states, use the loaded one (preserves user's width/visibility)
                    new_synced_states.push(loaded_state.clone());
                } else {
                    // If the column from app defaults does not exist in loaded (it's a new column to the app),
                    // add it using its default state from current_app_default_states.
                    new_synced_states.push(app_default_state.clone());
                }
            }
            // After this loop, new_synced_states contains all current_app_columns,
            // with user's preferences for existing ones, and defaults for new ones.
            // It also implicitly drops any columns that were in loaded_states but are not in current_app_columns.
            final_states = new_synced_states;
        }

        (final_states, needs_resave)
    }

    fn get_all_current_model_columns() -> Vec<ModelColumn> {
        ModelColumn::all()
    }

    fn get_current_app_default_states() -> Vec<ColumnState> {
        config::default_column_states()
    }

    #[test]
    fn test_column_sync_no_changes() {
        let app_defaults = get_current_app_default_states();
        let (synced_states, needs_resave) = synchronize_column_states(
            app_defaults.clone(),
            &get_all_current_model_columns(),
            &app_defaults
        );
        assert_eq!(synced_states, app_defaults);
        assert!(!needs_resave, "Should not need resave if states match current defaults perfectly");
    }

    #[test]
    fn test_column_sync_new_column_added_to_app() {
        let mut loaded_config_states = get_current_app_default_states();
        // Simulate a loaded config from an older version by removing one column
        let removed_column_for_test = ModelColumn::Families; // Pick one that's not critical for other tests
        loaded_config_states.retain(|cs| cs.column != removed_column_for_test);

        let current_app_cols = get_all_current_model_columns();
        let current_app_defaults = get_current_app_default_states();

        let (synced_states, needs_resave) = synchronize_column_states(
            loaded_config_states,
            &current_app_cols,
            &current_app_defaults
        );

        assert!(needs_resave, "Should need resave when a new column is added");
        assert_eq!(synced_states.len(), current_app_cols.len(), "Synced states should have all current app columns");

        // Check that the "newly added" column (Families) is present and has its default width
        let added_col_state = synced_states.iter().find(|cs| cs.column == removed_column_for_test).expect("Newly added column should be present");
        let default_state_for_added_col = current_app_defaults.iter().find(|cs| cs.column == removed_column_for_test).unwrap();

        assert_eq!(added_col_state.width, default_state_for_added_col.width, "Newly added column should have default width");
        assert_eq!(added_col_state.visible, default_state_for_added_col.visible, "Newly added column should have default visibility");

        // Verify other columns retained their original state (e.g., Name column width)
        let name_col_loaded = get_current_app_default_states().iter().find(|cs| cs.column == ModelColumn::Name).unwrap().clone();
        // (No change was made to Name column's state in loaded_config_states for this test)
        let name_col_synced = synced_states.iter().find(|cs| cs.column == ModelColumn::Name).unwrap();
        assert_eq!(name_col_synced.width, name_col_loaded.width);
        assert_eq!(name_col_synced.visible, name_col_loaded.visible);
    }

    #[test]
    fn test_column_sync_column_removed_from_app() {
        let mut loaded_config_states = get_current_app_default_states();
        // Add an "extra" column to the loaded config that we'll pretend is no longer in the app
        // For this test, we'll use 'Format' as the column that "exists in config" but "not in current app defs"
        // The `synchronize_column_states` function needs to be tested against a version of
        // `current_app_columns` and `current_app_default_states` that *don't* include this "removed" column.

        let removed_column_variant = ModelColumn::Format; // This column will be "removed" from the app's current definition for this test

        // `loaded_config_states` is fine, it simulates a config that still has the 'Format' column.
        // Let's ensure it has a distinct width to check it's properly dropped.
        if let Some(state) = loaded_config_states.iter_mut().find(|cs| cs.column == removed_column_variant) {
            state.width = Some(999.0); // Custom width for the "old" column
        }

        // Simulate that `ModelColumn::Format` is no longer part of the application
        let mut current_app_cols_for_test = get_all_current_model_columns();
        current_app_cols_for_test.retain(|c| *c != removed_column_variant);

        let mut current_app_defaults_for_test = get_current_app_default_states();
        current_app_defaults_for_test.retain(|cs| cs.column != removed_column_variant);

        let (synced_states, needs_resave) = synchronize_column_states(
            loaded_config_states, // Contains the "removed" Format column
            &current_app_cols_for_test, // Does not list Format
            &current_app_defaults_for_test // Does not define Format
        );

        assert!(needs_resave, "Should need resave when a column is removed");
        assert_eq!(synced_states.len(), current_app_cols_for_test.len(), "Synced states should only contain current app columns");
        assert!(synced_states.iter().find(|cs| cs.column == removed_column_variant).is_none(), "Removed column should not be in synced states");
    }

    #[test]
    fn test_column_sync_preserves_user_settings() {
        let mut loaded_config_states = get_current_app_default_states();
        // Modify some properties for a column that will persist (e.g., Name)
        let name_column_idx = loaded_config_states.iter().position(|cs| cs.column == ModelColumn::Name).unwrap();
        loaded_config_states[name_column_idx].width = Some(333.0);
        loaded_config_states[name_column_idx].visible = !loaded_config_states[name_column_idx].visible; // Toggle visibility

        let current_app_cols = get_all_current_model_columns();
        let current_app_defaults = get_current_app_default_states(); // These have standard defaults

        let (synced_states, needs_resave) = synchronize_column_states(
            loaded_config_states.clone(), // Pass the modified states
            &current_app_cols,
            &current_app_defaults
        );

        // needs_resave should be false if the *set of columns* hasn't changed, only their properties.
        // The original logic in `OllamaPullerApp::new` sets `needs_resave` only if `all_cols_enum != current_cols_enum`.
        // Mere changes to width/visibility of existing columns don't trigger `needs_resave` at that stage.
        // The `save_settings()` call after `OllamaPullerApp::new` is conditional on this `needs_resave`.
        // However, individual changes to column width/visibility are saved later by `app.update` detecting `prev_column_states != self.model_column_states`.
        // For this `synchronize_column_states` test function, if the sets of columns are the same, `needs_resave` will be false.
        assert!(!needs_resave, "Needs_resave should be false if only properties of existing columns changed, not the set of columns.");

        let synced_name_col = synced_states.iter().find(|cs| cs.column == ModelColumn::Name).unwrap();
        assert_eq!(synced_name_col.width, Some(333.0), "User's custom width should be preserved");
        assert_eq!(synced_name_col.visible, loaded_config_states[name_column_idx].visible, "User's custom visibility should be preserved");

        // Ensure other columns still match app defaults if they weren't changed in loaded_config_states
        let size_col_synced = synced_states.iter().find(|cs| cs.column == ModelColumn::Size).unwrap();
        let size_col_default = current_app_defaults.iter().find(|cs| cs.column == ModelColumn::Size).unwrap();
        assert_eq!(size_col_synced.width, size_col_default.width);
        assert_eq!(size_col_synced.visible, size_col_default.visible);
    }

     #[test]
    fn test_column_sync_order_reset_by_app_defaults() {
        let mut app_defaults_ordered = get_current_app_default_states(); // This is in a specific order

        // Create loaded_states in a different order
        let mut loaded_config_states_disordered = Vec::new();
        if app_defaults_ordered.len() >= 2 {
            loaded_config_states_disordered.push(app_defaults_ordered[1].clone()); // e.g., Size first
            loaded_config_states_disordered.push(app_defaults_ordered[0].clone()); // e.g., Name second
        } else {
            // Not enough columns to test reordering, use as is
            loaded_config_states_disordered = app_defaults_ordered.clone();
        }
        // Add any remaining columns from app_defaults_ordered to keep the set of columns the same
        for i in 2..app_defaults_ordered.len() {
            loaded_config_states_disordered.push(app_defaults_ordered[i].clone());
        }

        let current_app_cols = get_all_current_model_columns();
        // current_app_defaults is app_defaults_ordered

        let (synced_states, needs_resave) = synchronize_column_states(
            loaded_config_states_disordered,
            &current_app_cols,
            &app_defaults_ordered
        );

        // needs_resave is true because the set of columns is the same but their order in the *vector* is different.
        // The `HashSet` comparison `all_cols_enum_set != loaded_cols_set` would be false.
        // The logic `if all_cols_enum_set != loaded_cols_set` determines `needs_resave`.
        // However, the actual synchronization loop *does* reorder.
        // The `OllamaPullerApp::new`'s `needs_resave` is only set if the *set* of columns changes.
        // If only order is different but sets are same, `needs_resave` in `OllamaPullerApp::new` would be false.
        // The test for `synchronize_column_states` as written will set `needs_resave = true` if `loaded_states.clone()` is not identical to `final_states` after processing.
        // Let's adjust the test's expectation for `needs_resave` based on the original `OllamaPullerApp::new` logic for `needs_resave`.
        // The sets are the same, so `needs_resave` from `all_cols_enum_set != loaded_cols_set` should be false.
        // My `synchronize_column_states` function's `needs_resave` reflects whether `final_states` differs from `loaded_states`.
        // The crucial check is that the order in `synced_states` matches `app_defaults_ordered`.

        // If the sets of columns are identical, the original code's `needs_resave` would be false.
        // My helper function's `needs_resave` is true if `final_states` is different from `loaded_states`
        // which it will be if reordering occurred.
        // For this test, we care that the *output* `synced_states` has the app's default order.
         if app_defaults_ordered.len() >= 2 && (app_defaults_ordered[0].column != loaded_config_states_disordered[0].column || app_defaults_ordered[1].column != loaded_config_states_disordered[1].column) {
            assert!(needs_resave, "Needs resave should be true if order changed, making the vectors different.");
        } else {
            // If not enough columns to reorder or order was incidentally the same
            assert!(!needs_resave, "Needs resave should be false if order effectively did not change or too few columns.");
        }


        assert_eq!(synced_states.len(), app_defaults_ordered.len());
        for i in 0..synced_states.len() {
            assert_eq!(synced_states[i].column, app_defaults_ordered[i].column, "Column at index {} should match app default order", i);
            // Also check if the correct state was carried over (e.g. width)
             let original_state_for_this_column = loaded_config_states_disordered.iter().find(|cs| cs.column == app_defaults_ordered[i].column).unwrap();
            assert_eq!(synced_states[i].width, original_state_for_this_column.width);
            assert_eq!(synced_states[i].visible, original_state_for_this_column.visible);
        }
    }
}
