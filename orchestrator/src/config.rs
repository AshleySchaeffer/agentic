use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Runtime configuration for the orchestrator.
/// Resolved from `.claude/orchestrator/config.json` if present, otherwise defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// Path to the Unix domain socket the daemon listens on.
    pub socket_path: PathBuf,
    /// Path to the SQLite database file.
    pub db_path: PathBuf,
    /// Directory where large message payloads are offloaded.
    pub docs_dir: PathBuf,
    /// Message size threshold in bytes; payloads exceeding this are offloaded to a file.
    pub message_size_threshold: usize,
    /// Seconds before a delivered-but-unanswered message is flagged as an alert.
    pub unanswered_timeout_secs: u64,
    /// Seconds before an in-progress task with no activity is flagged as stalled.
    pub stall_timeout_secs: u64,
    /// Seconds between automation engine ticks.
    pub automation_interval_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            socket_path: PathBuf::from(".claude/orchestrator/daemon.sock"),
            db_path: PathBuf::from(".claude/orchestrator/orchestrator.db"),
            docs_dir: PathBuf::from(".claude/orchestrator/docs"),
            message_size_threshold: 2048,
            unanswered_timeout_secs: 300,
            stall_timeout_secs: 600,
            automation_interval_secs: 30,
        }
    }
}

impl Config {
    /// Load config from `.claude/orchestrator/config.json`, falling back to defaults.
    pub fn load() -> Self {
        let config_path = PathBuf::from(".claude/orchestrator/config.json");
        if config_path.exists() {
            match std::fs::read_to_string(&config_path) {
                Ok(contents) => match serde_json::from_str(&contents) {
                    Ok(cfg) => return cfg,
                    Err(e) => {
                        eprintln!("orchestrator: failed to parse config.json: {e}; using defaults");
                    }
                },
                Err(e) => {
                    eprintln!("orchestrator: failed to read config.json: {e}; using defaults");
                }
            }
        }
        Config::default()
    }
}
