#[cfg(test)]
mod settings_handling_tests {
    use llamalift::app::OllamaPullerApp;
    use llamalift::app::config::{
        AppSettings, APP_NAME, LogLevel as AppLogLevel, default_ollama_host,
        default_column_states, default_sort_state, MAX_LOG_ENTRIES, // Added MAX_LOG_ENTRIES
    };
    use llamalift::app::state::{UpdateMessage, ModelColumn, SortState, SortDirection};
    use llamalift::app::ollama::{OllamaModel, OllamaModelDetails};
    use eframe::{egui::{self, Context}, CreationContext}; // Added Context
    use std::sync::{mpsc, Arc};
    use tempfile::TempDir;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::env;
    use chrono_tz::Tz;
    use std::str::FromStr;
    use chrono::{Utc, TimeZone}; // Added TimeZone for Utc.timestamp_millis_opt
    use log::Level as LogLevel; // For actual log levels

    fn create_test_creation_context() -> CreationContext<'static> {
        let egui_ctx = egui::Context::default();
        let static_egui_ctx = Box::leak(Box::new(egui_ctx));
        CreationContext {
            egui_ctx: static_egui_ctx, integration_info: Default::default(), storage: None,
            raw_window_handle: None, window_info: Default::default(), wgpu_render_state: None,
        }
    }

    fn setup_app_for_settings_test(
        initial_settings_writer: Option<&dyn Fn(&Path)>
    ) -> (OllamaPullerApp, TempDir, PathBuf) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir for config");
        let temp_dir_path_str = temp_dir.path().to_str().unwrap().to_string();

        let original_xdg_config_home = env::var("XDG_CONFIG_HOME").ok();
        env::set_var("XDG_CONFIG_HOME", &temp_dir_path_str);

        let final_config_file_path = confy::get_configuration_file_path(APP_NAME, None)
            .expect("Failed to get confy config file path");

        if let Some(parent_dir) = final_config_file_path.parent() {
            fs::create_dir_all(parent_dir)
                .expect("Failed to create parent directory for config file");
        }

        if let Some(writer) = initial_settings_writer {
            writer(&final_config_file_path);
        }

        let cc = create_test_creation_context();
        let (task_sender, task_receiver) = mpsc::channel::<UpdateMessage>();

        let app = OllamaPullerApp::new(&cc, task_sender, task_receiver);

        if let Some(val) = original_xdg_config_home {
            env::set_var("XDG_CONFIG_HOME", val);
        } else {
            env::remove_var("XDG_CONFIG_HOME");
        }

        (app, temp_dir, final_config_file_path)
    }

    #[test]
    fn test_settings_save_and_close() {
        let (mut app, _temp_dir, config_file_path) = setup_app_for_settings_test(None);

        app.pending_settings = Some(app.settings.clone());
        app.show_settings_window = true;

        let new_host = "http://newhost-example.com:11444".to_string();
        let new_log_level_app = AppLogLevel::DEBUG; // This is the enum from app::config
        let new_tz_str = "America/New_York";

        if let Some(settings) = app.pending_settings.as_mut() {
            settings.ollama_host = new_host.clone();
            settings.log_level = new_log_level_app.to_string(); // AppSettings stores log_level as String
            settings.tz = Tz::from_str(new_tz_str).unwrap().name().to_string(); // AppSettings stores tz as String
        } else {
            panic!("pending_settings was not initialized");
        }

        app.save_pending_settings_and_apply();

        assert_eq!(app.settings.ollama_host, new_host);
        assert_eq!(app.settings.log_level, new_log_level_app.to_string());
        assert_eq!(app.settings.tz, new_tz_str); // app.settings.tz is String
        assert!(!app.show_settings_window, "Settings panel should be closed");

        let loaded_settings_from_disk: AppSettings = confy::load_path(&config_file_path)
            .expect("Failed to load settings from disk");
        assert_eq!(loaded_settings_from_disk.ollama_host, new_host);
        assert_eq!(loaded_settings_from_disk.log_level, new_log_level_app.to_string());
        assert_eq!(loaded_settings_from_disk.tz, new_tz_str);

        assert_eq!(app.ollama_client.base_url, new_host.trim_end_matches('/'));
    }

    #[test]
    fn test_settings_cancel() {
        let initial_settings_val = AppSettings {
            ollama_host: "http://originalhost.com".to_string(),
            log_level: AppLogLevel::INFO.to_string(), // Stored as string
            tz: Tz::UTC.name().to_string(), // Stored as string
            model_column_states: default_column_states(),
            model_sort_state: default_sort_state(),
            first_launch: false,
        };

        let (mut app, _temp_dir, config_file_path) = setup_app_for_settings_test(Some(&|p| {
            confy::store_path(p, initial_settings_val.clone()).unwrap();
        }));

        let original_app_settings = app.settings.clone();

        app.pending_settings = Some(app.settings.clone());
        app.show_settings_window = true;

        if let Some(settings) = app.pending_settings.as_mut() {
            settings.ollama_host = "http://some-other-host.com:54321".to_string();
            settings.log_level = AppLogLevel::TRACE.to_string();
        } else {
            panic!("pending_settings was not initialized for staging changes");
        }

        app.show_settings_window = false;
        app.pending_settings = None;

        assert_eq!(app.settings.ollama_host, original_app_settings.ollama_host);
        assert_eq!(app.settings.log_level, original_app_settings.log_level);
        assert_eq!(app.settings.tz, original_app_settings.tz);

        let loaded_settings_from_disk: AppSettings = confy::load_path(&config_file_path)
            .expect("Failed to load settings from disk");
        assert_eq!(loaded_settings_from_disk.ollama_host, original_app_settings.ollama_host);
        assert_eq!(app.ollama_client.base_url, original_app_settings.ollama_host.trim_end_matches('/'));
    }

    #[test]
    fn test_settings_load_corrupted_config() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let temp_dir_path_str = temp_dir.path().to_str().unwrap().to_string();

        let original_xdg_config_home = env::var("XDG_CONFIG_HOME").ok();
        env::set_var("XDG_CONFIG_HOME", &temp_dir_path_str);

        let config_file_path = confy::get_configuration_file_path(APP_NAME, None).unwrap();
        if let Some(parent_dir) = config_file_path.parent() {
            fs::create_dir_all(parent_dir).unwrap();
        }
        fs::write(&config_file_path, "this is not valid toml content. { error = true }")
            .expect("Failed to write corrupted config file");

        let cc = create_test_creation_context();
        let (task_sender, task_receiver) = mpsc::channel::<UpdateMessage>();
        let app = OllamaPullerApp::new(&cc, task_sender, task_receiver);

        let default_settings = AppSettings::default();
        assert_eq!(app.settings.ollama_host, default_settings.ollama_host);
        assert_eq!(app.settings.log_level, default_settings.log_level);
        assert_eq!(app.settings.tz, default_settings.tz);

        let settings_on_disk_after: AppSettings = confy::load_path(&config_file_path)
            .expect("Confy should have overwritten the corrupted file with defaults");
        assert_eq!(settings_on_disk_after.ollama_host, default_settings.ollama_host);
        assert_eq!(settings_on_disk_after.log_level, default_settings.log_level);
        assert_eq!(settings_on_disk_after.tz, default_settings.tz);

        if let Some(val) = original_xdg_config_home {
            env::set_var("XDG_CONFIG_HOME", val);
        } else {
            env::remove_var("XDG_CONFIG_HOME");
        }
    }

    fn create_test_model_data(name: &str, size: u64, days_ago: i64, family: &str, app_tz: &Tz) -> OllamaModel {
        let modified_time_utc = Utc::now() - chrono::Duration::days(days_ago);
        let modified_time_apptz = modified_time_utc.with_timezone(app_tz);

        OllamaModel {
            name: name.to_string(),
            modified_at: modified_time_utc.to_rfc3339(),
            size,
            digest: format!("digest_for_{}", name),
            details: OllamaModelDetails {
                format: Some("gguf".to_string()),
                family: Some(family.to_string()),
                families: Some(vec![family.to_string()]),
                parameter_size: Some("7B".to_string()),
                quantization_level: Some("Q4_0".to_string()),
            },
            modified_local: Some(modified_time_apptz.format("%Y-%m-%d %H:%M:%S").to_string()),
            size_human: format!("{}B", size),
            modified_dt: Some(modified_time_utc.into()),
        }
    }

    #[test]
    fn test_app_sort_models_by_name_asc_desc() {
        let (mut app, _temp_dir, _config_file_path) = setup_app_for_settings_test(None);
        let app_tz = Tz::from_str(&app.settings.tz).unwrap_or(Tz::UTC);


        *app.listed_models.lock().unwrap() = vec![
            create_test_model_data("charlie_model", 100, 1, "charlie_fam", &app_tz),
            create_test_model_data("alpha_model", 200, 2, "alpha_fam", &app_tz),
            create_test_model_data("bravo_model", 150, 3, "bravo_fam", &app_tz),
        ];
        app.rebuild_manage_view_cache();

        app.sort_models(ModelColumn::Name);
        assert_eq!(app.settings.model_sort_state.column, ModelColumn::Name, "Sort column should be Name");
        assert_eq!(app.settings.model_sort_state.direction, SortDirection::Ascending, "Sort direction should be Ascending");
        assert_eq!(app.manage_view_cache[0].name, "alpha_model", "First model should be alpha");
        assert_eq!(app.manage_view_cache[1].name, "bravo_model", "Second model should be bravo");
        assert_eq!(app.manage_view_cache[2].name, "charlie_model", "Third model should be charlie");

        app.sort_models(ModelColumn::Name);
        assert_eq!(app.settings.model_sort_state.column, ModelColumn::Name, "Sort column should still be Name");
        assert_eq!(app.settings.model_sort_state.direction, SortDirection::Descending, "Sort direction should toggle to Descending");
        assert_eq!(app.manage_view_cache[0].name, "charlie_model", "First model should be charlie (desc)");
        assert_eq!(app.manage_view_cache[1].name, "bravo_model", "Second model should be bravo (desc)");
        assert_eq!(app.manage_view_cache[2].name, "alpha_model", "Third model should be alpha (desc)");
    }

    #[test]
    fn test_app_sort_models_by_size_then_modified_date() {
        let (mut app, _temp_dir, _config_file_path) = setup_app_for_settings_test(None);
        let app_tz = Tz::from_str(&app.settings.tz).unwrap_or(Tz::UTC);

        let model_small_new = create_test_model_data("model_small_new", 100, 1, "fam_A", &app_tz);
        let model_large_old = create_test_model_data("model_large_old", 300, 10, "fam_C", &app_tz);
        let model_medium_mid = create_test_model_data("model_medium_mid", 200, 5, "fam_B", &app_tz);

        *app.listed_models.lock().unwrap() = vec![model_small_new.clone(), model_large_old.clone(), model_medium_mid.clone()];
        app.rebuild_manage_view_cache();

        app.sort_models(ModelColumn::Size);
        assert_eq!(app.settings.model_sort_state.column, ModelColumn::Size);
        assert_eq!(app.settings.model_sort_state.direction, SortDirection::Ascending);
        assert_eq!(app.manage_view_cache[0].name, "model_small_new", "Smallest model first by size");
        assert_eq!(app.manage_view_cache[1].name, "model_medium_mid", "Medium model second by size");
        assert_eq!(app.manage_view_cache[2].name, "model_large_old", "Largest model last by size");

        app.sort_models(ModelColumn::Modified);
        assert_eq!(app.settings.model_sort_state.column, ModelColumn::Modified);
        assert_eq!(app.settings.model_sort_state.direction, SortDirection::Ascending);
        assert_eq!(app.manage_view_cache[0].name, "model_large_old", "Oldest model first by date");
        assert_eq!(app.manage_view_cache[1].name, "model_medium_mid", "Mid-age model second by date");
        assert_eq!(app.manage_view_cache[2].name, "model_small_new", "Newest model last by date");

        app.sort_models(ModelColumn::Modified);
        assert_eq!(app.settings.model_sort_state.column, ModelColumn::Modified);
        assert_eq!(app.settings.model_sort_state.direction, SortDirection::Descending);
        assert_eq!(app.manage_view_cache[0].name, "model_small_new", "Newest model first by date (desc)");
        assert_eq!(app.manage_view_cache[1].name, "model_medium_mid", "Mid-age model second by date (desc)");
        assert_eq!(app.manage_view_cache[2].name, "model_large_old", "Oldest model last by date (desc)");
    }

    // --- New Log Tests ---
    #[test]
    fn test_app_add_log_message_and_pruning() {
        let (mut app, _temp_dir, _config_file_path) = setup_app_for_settings_test(None);
        // MAX_LOG_ENTRIES is already imported from app::config

        app.logs.lock().unwrap().clear();

        for i in 0..(MAX_LOG_ENTRIES + 5) {
            // Directly use app.add_log_message. The level here is log::Level.
            app.add_log_message(LogLevel::Info, format!("Test log {}", i));
        }

        let logs_guard = app.logs.lock().unwrap();
        assert_eq!(logs_guard.len(), MAX_LOG_ENTRIES, "Log count should be capped at MAX_LOG_ENTRIES");

        // Check the first (oldest after pruning)
        if let Some((level, msg, _ts)) = logs_guard.front() {
            assert_eq!(*level, LogLevel::Info);
            assert_eq!(msg, "Test log 5", "Oldest log message mismatch after pruning");
        } else {
            panic!("Logs deque is empty after adding entries, but should contain {} entries.", MAX_LOG_ENTRIES);
        }
        // Check the last (newest)
        if let Some((level, msg, _ts)) = logs_guard.back() {
            assert_eq!(*level, LogLevel::Info);
            assert_eq!(msg, format!("Test log {}", MAX_LOG_ENTRIES + 4), "Newest log message mismatch");
        } else {
            panic!("Logs deque is empty after adding entries, but should contain {} entries.", MAX_LOG_ENTRIES);
        }
    }

    #[test]
    fn test_app_get_formatted_logs_output() {
        let (mut app, _temp_dir, _config_file_path) = setup_app_for_settings_test(None);
        app.logs.lock().unwrap().clear();

        let time1_ms = (Utc::now() - chrono::Duration::seconds(20)).timestamp_millis();
        let time2_ms = (Utc::now() - chrono::Duration::seconds(10)).timestamp_millis();

        // Directly add structured logs
        app.add_log_message(LogLevel::Warn, "First warning log".to_string());
        // To control timestamp for test, we temporarily modify the last entry if needed after add_log_message
        // or assume add_log_message sets it close enough. For this test, let's manually push for exact timestamps.
        app.logs.lock().unwrap().clear(); // Clear again before manual push
        app.logs.lock().unwrap().push_back((LogLevel::Warn, "First warning log".to_string(), time1_ms));
        app.logs.lock().unwrap().push_back((LogLevel::Debug, "Second debug log".to_string(), time2_ms));


        let formatted_string = app.get_formatted_logs();

        let app_settings_tz_str = app.settings.tz.clone(); // app.settings.tz is String
        let tz = Tz::from_str(&app_settings_tz_str).unwrap_or(Tz::UTC);

        let dt1_str = Utc.timestamp_millis_opt(time1_ms).unwrap().with_timezone(&tz).format("%Y-%m-%d %H:%M:%S");
        let dt2_str = Utc.timestamp_millis_opt(time2_ms).unwrap().with_timezone(&tz).format("%Y-%m-%d %H:%M:%S");

        let mut expected_string = String::new();
        expected_string.push_str(&format!("[{}] (WARN) First warning log", dt1_str));
        expected_string.push('\n');
        expected_string.push_str(&format!("[{}] (DEBUG) Second debug log", dt2_str));
        expected_string.push('\n');

        assert_eq!(formatted_string, expected_string, "Formatted log string does not match expected output.");

        app.logs.lock().unwrap().clear();
        let empty_formatted_string = app.get_formatted_logs();
        assert_eq!(empty_formatted_string, "", "Formatted string for empty logs should be empty.");
    }
}
