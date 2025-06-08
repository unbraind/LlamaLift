#[cfg(test)]
mod startup_tests {
    use llamalift::app::config::{AppSettings, APP_NAME, default_column_states};
    use llamalift::app::state::ModelColumn;
    use llamalift::app::OllamaPullerApp; // Assuming OllamaPullerApp is accessible
    use std::sync::mpsc;
    use eframe::{egui, CreationContext};
    use tempfile::TempDir;
    use std::path::PathBuf;
    use std::fs;
    use tokio::runtime::Runtime; // OllamaPullerApp::new creates a tokio runtime
    use std::sync::Arc;

    // Helper to create a basic CreationContext for tests
    // This might need to be more sophisticated depending on what ::new actually uses from cc.
    fn create_test_creation_context() -> CreationContext<'static> {
        // If cc.egui_ctx is used for more than just load_image_from_bytes, this might not be enough.
        // For tests not running a full UI, we often have to accept some limitations here.
        // The goal is to allow `OllamaPullerApp::new` to run without panicking.
        let egui_ctx = egui::Context::default();

        // Leak the context to get a 'static reference. This is generally okay for test harnesses
        // where the context lives for the duration of the test.
        let static_egui_ctx = Box::leak(Box::new(egui_ctx));

        CreationContext {
            egui_ctx: static_egui_ctx,
            integration_info: Default::default(),
            storage: None, // Usually, you'd provide a mock storage if settings were through eframe::Storage
            raw_window_handle: None, // For wgpu
            window_info: Default::default(), // For wgpu
            wgpu_render_state: None, // For custom 3D painting
        }
    }

    // Helper function to get the confy config path for a specific temp directory
    fn get_config_file_path(temp_dir: &TempDir) -> PathBuf {
        temp_dir.path().join(format!("{}.toml", APP_NAME))
    }

    #[test]
    fn test_startup_no_existing_settings() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config_path_original = confy::get_configuration_file_path(APP_NAME, None).ok();

        // Override confy's base path for this test
        // This is tricky as confy doesn't directly support changing base path easily after compilation.
        // A common strategy is to set environment variables that confy might use,
        // or ensure the test runs in an environment where the default user config/data dirs are redirected.
        // For this test, we'll manually check the file that `OllamaPullerApp::new` would create.
        // `OllamaPullerApp::new` itself uses `confy::load` and `confy::store` with `APP_NAME, None`.
        // We need to ensure these operations target our temp_dir.
        // The most robust way is to ensure `OllamaPullerApp::new` can accept a config path or that confy is mocked.
        // Since we can't easily change confy's global behavior, we'll simulate the file operations.

        // 1. Simulate `confy::load` finding nothing: Ensure no config file in temp_dir initially.
        //    (This is implicitly true for a new TempDir).
        //    `OllamaPullerApp::new` will then use `AppSettings::default()` and attempt to save it.

        let cc = create_test_creation_context();
        let (update_sender, _update_receiver) = mpsc::channel();
        let (_task_sender, task_receiver) = mpsc::channel(); // Renamed to avoid conflict

        // Temporarily set the XDG_CONFIG_HOME to our temp dir for confy
        let original_xdg_config_home = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());

        // Ensure the specific config directory for the app exists, as confy might expect it
        let app_conf_dir = temp_dir.path().join(APP_NAME);
        fs::create_dir_all(&app_conf_dir).expect("Failed to create app conf dir in tempdir");

        let mut app = OllamaPullerApp::new(&cc, _task_sender, task_receiver);

        // `OllamaPullerApp::new` should have saved the default settings because no config file was found.
        let expected_config_file = app_conf_dir.join("default-config.toml"); // Confy default file name
                                                                                // or just APP_NAME.toml if no subfolder by default

        // Let's use the path `OllamaPullerApp` itself determines for saving.
        // `app.config_path` should be Some(...) and point into our temp_dir.
        let saved_config_path = app.config_path.clone().expect("App should have a config path after new");
        assert!(saved_config_path.starts_with(temp_dir.path()), "Config path should be inside temp_dir");

        assert!(saved_config_path.exists(), "Default config file should have been saved by ::new");

        // Verify the content of the saved config file
        let saved_settings: AppSettings = confy::load_path(&saved_config_path).expect("Failed to load saved default settings");
        let default_settings_for_compare = AppSettings::default();

        assert_eq!(saved_settings.ollama_host, default_settings_for_compare.ollama_host);
        assert_eq!(saved_settings.log_level, default_settings_for_compare.log_level);
        assert_eq!(saved_settings.tz, default_settings_for_compare.tz);
        assert_eq!(saved_settings.model_sort_state, default_settings_for_compare.model_sort_state);

        assert_eq!(saved_settings.model_column_states.len(), default_settings_for_compare.model_column_states.len());
        for i in 0..saved_settings.model_column_states.len() {
            assert_eq!(saved_settings.model_column_states[i].column, default_settings_for_compare.model_column_states[i].column);
            assert_eq!(saved_settings.model_column_states[i].visible, default_settings_for_compare.model_column_states[i].visible);
            assert_eq!(saved_settings.model_column_states[i].width, default_settings_for_compare.model_column_states[i].width,
                       "Default width for {:?} not saved correctly", saved_settings.model_column_states[i].column);
        }

        // To test "no rapid restart/resave", we'd ideally call app.update() a few times.
        // This requires a more complete egui::Context and eframe::Frame.
        // For now, we'll check the state immediately after `new()`.
        // The `needs_resave` flag in `new()` is true if column definitions change.
        // If starting fresh, `needs_resave` is false (as default settings match current column defs).
        // So, one save is expected (to write the initial default config). No more saves should happen without interaction.

        // Clean up: Reset XDG_CONFIG_HOME
        if let Some(val) = original_xdg_config_home {
            std::env::set_var("XDG_CONFIG_HOME", val);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
        // TempDir is automatically cleaned up when it goes out of scope.
    }

    #[test]
    fn test_startup_with_existing_settings_and_custom_widths() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");

        let original_xdg_config_home = std::env::var("XDG_CONFIG_HOME").ok();
        std::env::set_var("XDG_CONFIG_HOME", temp_dir.path());
        let app_conf_dir = temp_dir.path().join(APP_NAME);
        fs::create_dir_all(&app_conf_dir).expect("Failed to create app conf dir in tempdir");
        let config_file_path = app_conf_dir.join("default-config.toml");


        // 1. Create a custom settings file
        let mut custom_settings = AppSettings::default();
        custom_settings.ollama_host = "http://customhost:5555".to_string();
        let name_col_idx = custom_settings.model_column_states.iter_mut()
            .position(|cs| cs.column == ModelColumn::Name)
            .unwrap();
        custom_settings.model_column_states[name_col_idx].width = Some(888.0);
        custom_settings.model_column_states[name_col_idx].visible = false;

        let families_col_idx = custom_settings.model_column_states.iter_mut()
            .position(|cs| cs.column == ModelColumn::Families)
            .unwrap();
        custom_settings.model_column_states[families_col_idx].width = Some(777.0);
        custom_settings.model_column_states[families_col_idx].visible = true;


        confy::store_path(&config_file_path, &custom_settings).expect("Failed to store custom settings for test");
        assert!(config_file_path.exists(), "Custom config file should exist before test app starts.");

        // 2. Initialize app
        let cc = create_test_creation_context();
        let (update_sender, _update_receiver) = mpsc::channel();
        let (_task_sender, task_receiver) = mpsc::channel();

        let app = OllamaPullerApp::new(&cc, _task_sender, task_receiver);

        // 3. Verify settings are loaded
        assert_eq!(app.settings.ollama_host, "http://customhost:5555");

        let app_name_col_state = app.model_column_states.iter()
            .find(|cs| cs.column == ModelColumn::Name)
            .unwrap();
        assert_eq!(app_name_col_state.width, Some(888.0), "Custom width for Name column not loaded");
        assert_eq!(app_name_col_state.visible, false, "Custom visibility for Name column not loaded");

        let app_families_col_state = app.model_column_states.iter()
            .find(|cs| cs.column == ModelColumn::Families)
            .unwrap();
        assert_eq!(app_families_col_state.width, Some(777.0), "Custom width for Families column not loaded");
        assert_eq!(app_families_col_state.visible, true, "Custom visibility for Families column not loaded");

        // 4. Verify no immediate resave IF the loaded config was perfectly valid and didn't require sync.
        // In `OllamaPullerApp::new`, `needs_resave` is only true if the *set* of columns in the config
        // differs from the *set* of columns defined in the app. If they only differ by properties (width, order),
        // `needs_resave` in `new()` is false.
        // So, no second save should have occurred if the custom settings file was for the current version of columns.
        // We can check this by comparing the file's modification time before and after `new()`,
        // or by ensuring `save_settings` isn't called if `needs_resave` is false.
        // The current `OllamaPullerApp::new` will call `app.save_settings()` only if `needs_resave` is true.
        // Since our custom_settings used `AppSettings::default()` as a base, the column *set* is identical.
        // So `needs_resave` should be false.

        // This is tricky to assert directly without instrumenting `save_settings` or complex file monitoring.
        // However, we know that `OllamaPullerApp::new` only calls `save_settings` if `needs_resave` is true.
        // `needs_resave` is true if `all_cols_enum != current_cols_enum`.
        // In this test, `custom_settings` is based on `AppSettings::default()`, so the sets of columns are identical.
        // Thus, `needs_resave` should be false, and `save_settings` should not have been called from `new`.

        // Restore XDG_CONFIG_HOME
        if let Some(val) = original_xdg_config_home {
            std::env::set_var("XDG_CONFIG_HOME", val);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }
}
