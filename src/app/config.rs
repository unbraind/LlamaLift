// src/app/config.rs
// Defines configuration structures, constants, loading/saving logic, and initial setup for LlamaLift settings, including table state persistence.

// Import necessary types from sibling modules
use crate::app::state::{ColumnState, ModelColumn, SortState};

use chrono_tz::Tz;
use dotenvy::dotenv;
use log::{warn, LevelFilter}; // Use log::warn for consistency
use serde::{Deserialize, Serialize};
use std::{env, str::FromStr};

// --- Global Configuration Block ---
pub const SCRIPT_VERSION: &str = "0.1.1";
pub const APP_NAME: &str = "LlamaLift";
pub const MAX_MODEL_INPUTS: usize = 100;
pub const DEFAULT_TZ: &str = "Europe/Vienna";
pub const DEFAULT_LOG_LEVEL: &str = "INFO";
pub const DEFAULT_OLLAMA_HOST: &str = "127.0.0.1:11434";
pub const MAX_LOG_ENTRIES: usize = 1000; // Max number of log entries to keep

// --- Configuration Structs ---

/// Runtime configuration derived from AppSettings (Made pub)
#[derive(Clone, Debug)]
pub struct Config {
    pub ollama_host: String,
    pub tz: Tz,
}

/// Configuration loaded initially from environment/.env for logger setup and defaults (Made pub)
#[derive(Clone, Debug)]
pub struct InitialConfig {
    pub ollama_host: String,
    pub log_level: LevelFilter,
    pub tz: Tz,
}

/// Persistently stored application settings using confy (Made pub)
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct AppSettings {
    pub ollama_host: String,
    pub log_level: String,
    pub tz: String,
    #[serde(default = "default_column_states")] 
    pub model_column_states: Vec<ColumnState>,
    pub model_sort_state: SortState,
}

// --- Default Implementation for AppSettings ---

/// Provides default column states (visibility and order). Made public.
pub fn default_column_states() -> Vec<ColumnState> {
    ModelColumn::all()
        .into_iter()
        .map(|col| {
            let default_width = match col {
                ModelColumn::Name => Some(250.0),
                ModelColumn::Size => Some(100.0),
                ModelColumn::Modified => Some(180.0),
                ModelColumn::Digest => Some(120.0),
                ModelColumn::Format => Some(100.0),
                ModelColumn::Family => Some(120.0),
                ModelColumn::Families => Some(120.0),
                ModelColumn::ParameterSize => Some(120.0),
                ModelColumn::QuantizationLevel => Some(150.0),
            };
            ColumnState {
                visible: matches!(
                    col,
                    ModelColumn::Name | ModelColumn::Size | ModelColumn::Modified
                ),
                column: col,
                width: default_width,
            }
        })
        .collect()
}

impl Default for AppSettings {
    fn default() -> Self {
        let initial_config = load_initial_config();
        AppSettings {
            ollama_host: initial_config.ollama_host,
            log_level: initial_config.log_level.to_string(),
            tz: initial_config.tz.name().to_string(),
            // Use the specific default functions for table state
            model_column_states: default_column_states(),
            model_sort_state: SortState::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::ModelColumn;

    #[test]
    fn test_default_column_states_widths() {
        let states = default_column_states();
        assert_eq!(states.len(), ModelColumn::all().len(), "Should have a state for every ModelColumn variant");

        for state in states {
            let expected_width = match state.column {
                ModelColumn::Name => Some(250.0),
                ModelColumn::Size => Some(100.0),
                ModelColumn::Modified => Some(180.0),
                ModelColumn::Digest => Some(120.0),
                ModelColumn::Format => Some(100.0),
                ModelColumn::Family => Some(120.0),
                ModelColumn::Families => Some(120.0),
                ModelColumn::ParameterSize => Some(120.0),
                ModelColumn::QuantizationLevel => Some(150.0),
            };
            assert_eq!(state.width, expected_width, "Default width for {:?} is incorrect", state.column);

            let expected_visibility = matches!(
                state.column,
                ModelColumn::Name | ModelColumn::Size | ModelColumn::Modified
            );
            assert_eq!(state.visible, expected_visibility, "Default visibility for {:?} is incorrect", state.column);
        }
    }

    #[test]
    fn test_app_settings_default_uses_default_column_states() {
        // To isolate this test, we don't want .env or real env vars to interfere
        // with load_initial_config() which is called by AppSettings::default().
        // We can temporarily clear env vars that load_initial_config() uses.
        let orig_tz = std::env::var("TZ").ok();
        let orig_log_level = std::env::var("LOG_LEVEL").ok();
        let orig_ollama_host = std::env::var("OLLAMA_HOST").ok();

        std::env::remove_var("TZ");
        std::env::remove_var("LOG_LEVEL");
        std::env::remove_var("OLLAMA_HOST");

        let default_settings = AppSettings::default();
        let expected_column_states = default_column_states();

        assert_eq!(default_settings.model_column_states.len(), expected_column_states.len());
        for i in 0..default_settings.model_column_states.len() {
            assert_eq!(default_settings.model_column_states[i].column, expected_column_states[i].column);
            assert_eq!(default_settings.model_column_states[i].visible, expected_column_states[i].visible);
            assert_eq!(default_settings.model_column_states[i].width, expected_column_states[i].width);
        }

        // Check a few other default fields to ensure load_initial_config() ran as expected (with defaults)
        assert_eq!(default_settings.tz, DEFAULT_TZ); // Assuming load_initial_config falls back to DEFAULT_TZ
        assert_eq!(default_settings.log_level, DEFAULT_LOG_LEVEL);
        assert_eq!(default_settings.ollama_host, DEFAULT_OLLAMA_HOST);


        // Restore original env vars
        if let Some(val) = orig_tz { std::env::set_var("TZ", val); }
        if let Some(val) = orig_log_level { std::env::set_var("LOG_LEVEL", val); }
        if let Some(val) = orig_ollama_host { std::env::set_var("OLLAMA_HOST", val); }
    }

    #[test]
    fn test_app_settings_serialization_deserialization() {
        let mut settings = AppSettings::default();
        // Modify some settings to ensure they are serialized and deserialized correctly
        settings.ollama_host = "http://testhost:1234".to_string();
        settings.log_level = "DEBUG".to_string();
        settings.tz = "America/New_York".to_string();
        if let Some(col_state) = settings.model_column_states.iter_mut().find(|cs| cs.column == ModelColumn::Name) {
            col_state.width = Some(300.0);
            col_state.visible = true;
        }
        if let Some(col_state) = settings.model_column_states.iter_mut().find(|cs| cs.column == ModelColumn::Digest) {
            col_state.visible = true; // Default is false
        }
        settings.model_sort_state = SortState {
            column: ModelColumn::Size,
            direction: crate::app::state::SortDirection::Descending,
        };

        let serialized = serde_json::to_string(&settings).expect("Failed to serialize AppSettings");
        let deserialized: AppSettings = serde_json::from_str(&serialized).expect("Failed to deserialize AppSettings");

        assert_eq!(deserialized.ollama_host, settings.ollama_host);
        assert_eq!(deserialized.log_level, settings.log_level);
        assert_eq!(deserialized.tz, settings.tz);
        assert_eq!(deserialized.model_sort_state, settings.model_sort_state);

        assert_eq!(deserialized.model_column_states.len(), settings.model_column_states.len());
        for i in 0..deserialized.model_column_states.len() {
            assert_eq!(deserialized.model_column_states[i].column, settings.model_column_states[i].column);
            assert_eq!(deserialized.model_column_states[i].visible, settings.model_column_states[i].visible);
            assert_eq!(deserialized.model_column_states[i].width, settings.model_column_states[i].width);
        }

        // Test deserialization of potentially missing fields using #[serde(default)]
        let partial_json = r#"
        {
            "ollama_host": "http://partialhost:5678",
            "log_level": "TRACE",
            "tz": "Asia/Tokyo"
        }
        "#;
        // model_column_states and model_sort_state are missing, so they should use their defaults
        let deserialized_partial: AppSettings = serde_json::from_str(partial_json).expect("Failed to deserialize partial AppSettings");

        assert_eq!(deserialized_partial.ollama_host, "http://partialhost:5678");
        assert_eq!(deserialized_partial.log_level, "TRACE");
        assert_eq!(deserialized_partial.tz, "Asia/Tokyo");

        let expected_default_cols = default_column_states();
        assert_eq!(deserialized_partial.model_column_states.len(), expected_default_cols.len());
        for i in 0..deserialized_partial.model_column_states.len() {
            assert_eq!(deserialized_partial.model_column_states[i].column, expected_default_cols[i].column);
            assert_eq!(deserialized_partial.model_column_states[i].width, expected_default_cols[i].width);
        }
        assert_eq!(deserialized_partial.model_sort_state, SortState::default());
    }
}

// --- Configuration Loading Functions ---

/// Loads the *initial* configuration settings. (Made pub)
/// Priority: Environment Variables > .env file > Hardcoded Defaults.
/// This is primarily used for setting up the logger and providing defaults
/// before the main persistent settings (`AppSettings`) are loaded by `confy`.
pub fn load_initial_config() -> InitialConfig {
    dotenv().ok();

    // Load Timezone (TZ)
    let tz_str = env::var("TZ").unwrap_or_else(|_| {
        warn!(
            "TZ environment variable not set, using default: {}",
            DEFAULT_TZ
        );
        DEFAULT_TZ.to_string()
    });
    let tz = Tz::from_str(&tz_str).unwrap_or_else(|err| {
        // Use eprintln for early logging before logger might be fully initialized
        eprintln!(
            "WARN: Invalid TZ '{}' from env/default. Falling back to UTC. Error: {}",
            tz_str, err
        );
        Tz::UTC // Fallback to UTC on error
    });

    // Load Log Level (LOG_LEVEL)
    let log_level_str = env::var("LOG_LEVEL").unwrap_or_else(|_| {
        warn!(
            "LOG_LEVEL environment variable not set, using default: {}",
            DEFAULT_LOG_LEVEL
        );
        DEFAULT_LOG_LEVEL.to_string()
    });
    let log_level = LevelFilter::from_str(&log_level_str).unwrap_or_else(|err| {
        eprintln!(
            "WARN: Invalid LOG_LEVEL '{}' from env/default. Falling back to {}. Error: {}",
            log_level_str, DEFAULT_LOG_LEVEL, err
        );
        LevelFilter::from_str(DEFAULT_LOG_LEVEL).expect("Default log level is invalid")
    });

    // Load Ollama Host (OLLAMA_HOST)
    let ollama_host = env::var("OLLAMA_HOST").unwrap_or_else(|_| {
        warn!(
            "OLLAMA_HOST environment variable not set, using default: {}",
            DEFAULT_OLLAMA_HOST
        );
        DEFAULT_OLLAMA_HOST.to_string()
    });

    InitialConfig {
        ollama_host,
        log_level,
        tz,
    }
}
