// src/app/ollama.rs
// Handles interactions with the Ollama API: defines request/response structs and async functions for API calls (pull, list, delete).

use crate::app::config::Config;
use crate::app::state::UpdateMessage;
use crate::app::utils::format_size; 
use chrono::{DateTime, FixedOffset}; // Used for parsing dates, Added FixedOffset
use chrono_tz::Tz;
use futures_util::StreamExt;
use log::{debug, error, trace, warn};
use reqwest;
use serde::{Deserialize, Serialize};
use std::sync::mpsc::Sender;

// --- Ollama API Structures ---

/// Represents the status messages received during a model pull operation (streamed).
#[derive(Deserialize, Debug, Clone)]
pub struct OllamaPullStatus {
    pub status: String,
    pub digest: Option<String>,
    pub total: Option<u64>,
    pub completed: Option<u64>,
    pub error: Option<String>,
}

/// Represents the details nested within a model response.
#[derive(Deserialize, Debug, Clone, Default)]
pub struct OllamaModelDetails {
    pub format: Option<String>,
    pub family: Option<String>,
    pub families: Option<Vec<String>>, // Handle null array from API
    pub parameter_size: Option<String>,
    pub quantization_level: Option<String>,
}

/// Represents a single model returned by the `/api/tags` endpoint.
#[derive(Deserialize, Debug, Clone)]
pub struct OllamaModel {
    pub name: String,
    pub modified_at: String, // Original timestamp string from Ollama
    pub size: u64,           // Size in bytes
    pub digest: String,      // Added digest field

    // Add the details field, defaulting if missing in JSON
    #[serde(default)] // Use serde default for the whole struct
    pub details: OllamaModelDetails,

    // Fields added/populated locally after fetching
    #[serde(skip)]
    pub modified_local: Option<String>,
    #[serde(skip)]
    pub size_human: String, // Human-readable size
    // Add parsed DateTime for sorting
    #[serde(skip)] // Don't expect this from JSON
    pub modified_dt: Option<DateTime<FixedOffset>>, // Store with original offset
}

/// Represents the overall response structure from the `/api/tags` endpoint.
#[derive(Deserialize, Debug, Clone)]
pub struct OllamaTagsResponse {
    pub models: Vec<OllamaModel>,
}

/// Represents the request body for the `/api/delete` endpoint.
#[derive(Serialize, Debug, Clone)]
pub struct OllamaDeleteRequest {
    pub name: String,
}

// --- Async Operations ---

/// Asynchronously pulls a model from the Ollama server using the `/api/pull` endpoint.
/// Streams progress updates back to the UI thread via the sender.
pub async fn pull_model_async(
    model_id: &str,
    config: &Config,
    sender: Sender<UpdateMessage>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    // Ensure host URL starts with http:// or https://
    let host = if config.ollama_host.starts_with("http://") || config.ollama_host.starts_with("https://")
    {
        config.ollama_host.clone()
    } else {
        format!("http://{}", config.ollama_host) // Prepend http:// if missing
    };
    let url = format!("{}/api/pull", host);
    let request_body = serde_json::json!({ "name": model_id, "stream": true });

    debug!("Sending pull request to {} for model '{}'", url, model_id);
    // Send DEBUG log via channel as well, as logger might filter it
    let _ = sender.send(UpdateMessage::Log(format!(
        "DEBUG: Sending pull request to {} for model '{}'",
        url, model_id
    )));

    // Send the POST request
    let res = client.post(&url).json(&request_body).send().await.map_err(|e| {
        let err_msg = format!("Network request failed for {}: {}", url, e);
        error!("{}", err_msg); // Log error
        let _ = sender.send(UpdateMessage::Log(format!("ERROR: {}", err_msg))); // Send error to UI
        err_msg // Return error message
    })?;

    let status_code = res.status();
    // Check if the request was successful (e.g., 2xx status code)
    if !status_code.is_success() {
        let error_body = res
            .text()
            .await
            .unwrap_or_else(|_| "Unknown server error".to_string()); // Read error body
        error!(
            "Ollama server at {} returned error status {}: {}",
            host, status_code, error_body
        );
        let log_msg = format!(
            "ERROR: Ollama server returned error status {}: {}",
            status_code, error_body
        );
        let _ = sender.send(UpdateMessage::Log(log_msg.clone())); // Send error to UI
        // Return a formatted error
        return Err(format!("Server error ({}) from {}: {}", status_code, host, error_body).into());
    }

    // Process the response stream
    let mut stream = res.bytes_stream();
    let mut last_digest = String::new(); // Track the current layer digest
    let mut current_total: Option<u64> = None; // Total size of the current layer
    let mut layer_completed: Option<u64> = None; // Completed bytes of the current layer

    // Iterate over chunks in the stream
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| format!("Stream error while pulling {}: {}", model_id, e))?;
        // Ollama streams JSON objects separated by newlines
        let lines = String::from_utf8_lossy(&chunk);

        for line in lines.lines() {
            if line.trim().is_empty() {
                continue;
            } // Skip empty lines
            trace!("Raw line from {}: {}", model_id, line); // Log raw data at TRACE level

            // Attempt to parse each line as an OllamaPullStatus JSON object
            match serde_json::from_str::<OllamaPullStatus>(line) {
                Ok(status) => {
                    trace!("[{}] Parsed: {:?}", model_id, status); // Log parsed status at TRACE
                    let log_msg = format!("[{}] {}", model_id, status.status);
                    debug!("{}", log_msg); // Log status message at DEBUG

                    // Send status text update to UI
                    let _ = sender.send(UpdateMessage::StatusText(status.status.clone()));

                    // Send shorter status messages to UI log panel (INFO level)
                    if status.status.len() < 100 {
                        let _ = sender.send(UpdateMessage::Log(format!("INFO: {}", log_msg)));
                    }

                    // Check for explicit errors in the status message
                    if let Some(err_msg) = status.error {
                        error!("Stream error reported for {}: {}", model_id, err_msg);
                        let _ =
                            sender.send(UpdateMessage::Log(format!("ERROR: Stream error: {}", err_msg)));
                    }

                    // Update progress based on digest changes
                    if let Some(digest) = &status.digest {
                        if *digest != last_digest {
                            // New layer started
                            last_digest = digest.clone();
                            current_total = status.total;
                            layer_completed = status.completed;
                            debug!(
                                "[{}] Starting layer {} (Total: {:?}, Completed: {:?})",
                                model_id, digest, current_total, layer_completed
                            );
                            let _ = sender.send(UpdateMessage::Log(format!(
                                "DEBUG: [{}] Starting layer {}...",
                                model_id, digest
                            )));
                            // Reset progress for the new layer
                            let _ = sender.send(UpdateMessage::Progress(0.0));
                        } else {
                            // Update progress for the current layer
                            layer_completed = status.completed;
                            // Guess sometimes total might arrive later
                            if status.total.is_some() {
                                current_total = status.total;
                            }
                        }
                    } else {
                        // Status message without digest (e.g., "pulling manifest", "verifying sha256", "success")
                        // Reset layer tracking if we were tracking one
                        if !last_digest.is_empty() {
                            last_digest.clear();
                            current_total = None;
                            layer_completed = None;
                        }
                        // Set progress to 1.0 on success, 0.0 otherwise for these general statuses
                        let progress = if status.status.contains("success") {
                            1.0
                        } else {
                            0.0
                        };
                        let _ = sender.send(UpdateMessage::Progress(progress));
                    }

                    // Calculate and send layer progress if possible
                    if let (Some(completed), Some(total)) = (layer_completed, current_total) {
                        if total > 0 {
                            let progress = completed as f32 / total as f32;
                            trace!(
                                "[{}] Layer progress: {} / {} = {}",
                                model_id,
                                completed,
                                total,
                                progress
                            );
                            // Send progress, ensuring it doesn't exceed 1.0
                            let _ = sender.send(UpdateMessage::Progress(progress.min(1.0)));
                        } else {
                            // Handle cases where total is 0 (e.g., layer already exists)
                            let progress = if status.status.contains("pulling")
                                || status.status.contains("downloading")
                            {
                                0.0 // Still in progress technically
                            } else {
                                1.0 // Assume complete if not pulling/downloading and total is 0
                            };
                            let _ = sender.send(UpdateMessage::Progress(progress));
                        }
                    } else if status.status.contains("success") {
                        // If no layer info but status is success, report 100% progress
                        trace!("[{}] Step success, progress 1.0", model_id);
                        let _ = sender.send(UpdateMessage::Progress(1.0));
                    }
                }
                Err(e) => {
                    // Log JSON parsing errors
                    warn!(
                        "JSON parse failed for line from {}: '{}'. Error: {}",
                        model_id, line, e
                    );
                    let _ = sender.send(UpdateMessage::Log(format!(
                        "WARN: Failed to parse line: {}",
                        line
                    )));
                }
            }
        }
    }
    debug!("Stream finished for model '{}'.", model_id);
    let _ = sender.send(UpdateMessage::Log(format!(
        "DEBUG: Stream finished for model '{}'.",
        model_id
    )));
    Ok(()) // Indicate successful completion of the pull stream processing
}

/// Asynchronously fetches the list of installed models from the Ollama server using `/api/tags`.
/// Processes the response to format size and modification time.
pub async fn list_models_async(
    config: &Config,
    sender: Sender<UpdateMessage>,
) -> Result<Vec<OllamaModel>, Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    // Ensure host URL starts with http:// or https://
    let host = if config.ollama_host.starts_with("http://") || config.ollama_host.starts_with("https://")
    {
        config.ollama_host.clone()
    } else {
        format!("http://{}", config.ollama_host) // Prepend http:// if missing
    };
    let url = format!("{}/api/tags", host);
    debug!("Sending list request to {}", url);
    let _ = sender.send(UpdateMessage::Log(format!(
        "DEBUG: Sending list models request to {}",
        url
    )));

    // Send the GET request
    let res = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Network request failed for {}: {}", url, e))?;

    let status_code = res.status();
    // Check for non-success status codes
    if !status_code.is_success() {
        let error_body = res
            .text()
            .await
            .unwrap_or_else(|_| "Unknown server error".to_string());
        error!(
            "Ollama server at {} returned error status {}: {}",
            host, status_code, error_body
        );
        let log_msg = format!(
            "ERROR listing models: Server returned error status {}: {}",
            status_code, error_body
        );
        let _ = sender.send(UpdateMessage::Log(log_msg.clone()));
        return Err(format!("Server error ({}) from {}: {}", status_code, host, error_body).into());
    }

    // Parse the successful JSON response
    let mut response_body: OllamaTagsResponse = res
        .json()
        .await
        .map_err(|e| format!("Failed to parse JSON response from {}: {}", url, e))?;

    // Get the local timezone from the runtime config
    let local_tz: Tz = config.tz; // Use the Tz type directly

    // Post-process the model list: format size and modification time
    for model in response_body.models.iter_mut() {
        // Format size into human-readable string (e.g., GiB, MiB)
        model.size_human = format_size(model.size);

        // --- Parse and Format Time ---
        // 1. Try parsing the timestamp string (RFC3339 format expected)
        match DateTime::parse_from_rfc3339(&model.modified_at) {
            Ok(parsed_dt_with_offset) => {
                // Store the parsed DateTime with its original offset for accurate sorting later
                model.modified_dt = Some(parsed_dt_with_offset);

                // 2. Convert to the user's configured local timezone for display
                let local_dt = parsed_dt_with_offset.with_timezone(&local_tz);

                // 3. Format the local datetime into a user-friendly string
                model.modified_local = Some(local_dt.format("%Y-%m-%d %H:%M:%S").to_string());
            }
            Err(e) => {
                // Log warning if parsing fails, provide fallback text
                warn!(
                    "Failed to parse model modified_at date '{}' for model '{}': {}. Using original.",
                    model.modified_at, model.name, e
                );
                model.modified_local = Some(format!("{} (Parse Failed)", model.modified_at));
                model.modified_dt = None; // Ensure dt field is None on parse failure
            }
        }
        // --- End Time Parsing ---
    }
    Ok(response_body.models) // Return the processed list of models
}

/// Asynchronously deletes a model from the Ollama server using the `/api/delete` endpoint.
pub async fn delete_model_async(
    model_name: &str,
    config: &Config,
    sender: Sender<UpdateMessage>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let client = reqwest::Client::new();
    // Ensure host URL starts with http:// or https://
    let host = if config.ollama_host.starts_with("http://") || config.ollama_host.starts_with("https://")
    {
        config.ollama_host.clone()
    } else {
        format!("http://{}", config.ollama_host) // Prepend http:// if missing
    };
    let url = format!("{}/api/delete", host);
    // Create the request body required by the delete API
    let request_body = OllamaDeleteRequest {
        name: model_name.to_string(),
    };

    debug!(
        "Sending delete request to {} for model '{}'",
        url, model_name
    );
    let _ = sender.send(UpdateMessage::Log(format!(
        "DEBUG: Sending delete request for '{}'",
        model_name
    )));

    // Send the DELETE request with the JSON body
    let res = client
        .delete(&url)
        .json(&request_body)
        .send()
        .await
        .map_err(|e| format!("Network request failed for {}: {}", url, e))?;

    let status_code = res.status();
    // Handle different response statuses
    if status_code.is_success() {
        // Success (e.g., 200 OK)
        debug!(
            "Successfully received response for deleting model '{}'.",
            model_name
        );
        Ok(()) // Indicate success
    } else if status_code == reqwest::StatusCode::NOT_FOUND {
        // Model not found (treat as success for deletion purpose, maybe it was already deleted)
        warn!(
            "Model '{}' not found on server {} during deletion attempt.",
            model_name, host
        );
        let _ = sender.send(UpdateMessage::Log(format!(
            "WARN: Model '{}' not found on server.",
            model_name
        )));
        Ok(()) // Still return Ok, as the desired state (model not present) is achieved
    } else {
        // Other errors
        let error_body = res
            .text()
            .await
            .unwrap_or_else(|_| "Unknown server error".to_string());
        error!(
            "Ollama server at {} returned error status {} deleting model '{}': {}",
            host, status_code, model_name, error_body
        );
        let log_msg = format!(
            "ERROR deleting model '{}': Server returned error status {}: {}",
            model_name, status_code, error_body
        );
        let _ = sender.send(UpdateMessage::Log(log_msg.clone()));
        Err(format!(
            "Server error ({}) deleting {}: {}",
            status_code, model_name, error_body
        )
        .into()) // Return the error
    }
}
