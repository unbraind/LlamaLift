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
        .map(|col| ColumnState {
            visible: matches!(
                col,
                ModelColumn::Name | ModelColumn::Size | ModelColumn::Modified
            ),
            column: col,
            width: None,
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
