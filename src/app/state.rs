// src/app/state.rs
// Defines state-related enums and structs for LlamaLift: application status, views, inter-thread messages, and table column/sort/width state.

// Import necessary types from other modules within the app
use crate::app::ollama::OllamaModel;
use serde::{Deserialize, Serialize};

// --- Application State Enums ---

/// Represents the possible operational statuses of the application.
#[derive(Clone, Debug, PartialEq)]
pub enum AppStatus {
    Idle,
    /// Contains (current_model_index, total_models_in_batch).
    Pulling(usize, usize),
    /// The application is fetching the list of models from the Ollama server.
    ListingModels,
    /// Contains the model_name being deleted.
    DeletingModel(String),
    /// The last operation completed successfully.
    Success,
    /// Contains the error_message.
    Error(String),
}

/// Represents the main views available in the application UI.
#[derive(Clone, Debug, PartialEq)]
pub enum AppView {
    /// The view for downloading new models.
    Download,
    /// The view for managing existing downloaded models.
    ManageModels,
}

/// Defines messages passed from background tasks (like Ollama interactions)
/// or the logger to the main UI thread via an MPSC channel to trigger updates.
#[derive(Debug)]
pub enum UpdateMessage {
    /// A log message (typically INFO level or lower) to be displayed in the UI.
    Log(String),
    /// An update to the progress bar value (typically 0.0 to 1.0).
    Progress(f32),
    /// An update to the human-readable status text displayed to the user.
    StatusText(String),
    /// A change in the overall application status.
    Status(AppStatus),
    /// A new list of models received from the Ollama server.
    ModelList(Vec<OllamaModel>),
}

// --- Manage Models Table State ---

/// Represents the columns available in the Manage Models table.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ModelColumn {
    Name,
    Size,
    Modified,
    Digest,
    Format,
    Family,
    Families,
    ParameterSize,
    QuantizationLevel,
    // Note: Actions (Delete, Copy, Edit buttons) column is handled separately in the table layout
}

impl ModelColumn {
    /// Returns the display name for the column header.
    pub fn display_name(&self) -> &'static str {
        match self {
            ModelColumn::Name => "Name",
            ModelColumn::Size => "Size",
            ModelColumn::Modified => "Modified (Local TZ)",
            ModelColumn::Digest => "Digest",
            ModelColumn::Format => "Format",
            ModelColumn::Family => "Family",
            ModelColumn::Families => "Families",
            ModelColumn::ParameterSize => "Parameter Size",
            ModelColumn::QuantizationLevel => "Quantization Level",
        }
    }

    /// Returns a vector of all possible columns.
    pub fn all() -> Vec<Self> {
        vec![
            Self::Name,
            Self::Size,
            Self::Modified,
            Self::Digest,
            Self::Format,
            Self::Family,
            Self::Families,
            Self::ParameterSize,
            Self::QuantizationLevel,
        ]
    }
}

/// Holds the state (e.g., visibility, width) for a specific column.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ColumnState {
    pub column: ModelColumn,
    pub visible: bool,
    pub width: Option<f32>,
}

/// Represents the sort direction for a table column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// Holds the current sorting state for the Manage Models table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortState {
    pub column: ModelColumn,
    pub direction: SortDirection,
}

impl Default for SortState {
    fn default() -> Self {
        // Default sort by Modified date, Descending (newest first)
        Self {
            column: ModelColumn::Modified,
            direction: SortDirection::Descending,
        }
    }
}
