// Configuration file discovery

use anyhow::{anyhow, Result};
use std::path::PathBuf;
use tracing::debug;

/// Discover configuration file using priority order:
/// 1. --config CLI argument (if provided)
/// 2. KRILL_CONFIG environment variable
/// 3. ./krill.yaml (current directory)
/// 4. ~/.krill/krill.yaml (user home)
pub fn discover_config(cli_config: Option<PathBuf>) -> Result<PathBuf> {
    // 1. CLI argument takes highest priority
    if let Some(path) = cli_config {
        if path.exists() {
            debug!("Using config from CLI argument: {:?}", path);
            return Ok(path);
        } else {
            return Err(anyhow!("Config file not found: {:?}", path));
        }
    }

    // 2. Environment variable
    if let Ok(env_path) = std::env::var("KRILL_CONFIG") {
        let path = PathBuf::from(env_path);
        if path.exists() {
            debug!("Using config from KRILL_CONFIG env var: {:?}", path);
            return Ok(path);
        } else {
            return Err(anyhow!(
                "Config file from KRILL_CONFIG not found: {:?}",
                path
            ));
        }
    }

    // 3. Current directory
    let current_dir_config = PathBuf::from("./krill.yaml");
    if current_dir_config.exists() {
        debug!(
            "Using config from current directory: {:?}",
            current_dir_config
        );
        return Ok(current_dir_config);
    }

    // 4. User home directory
    if let Some(home_dir) = dirs::home_dir() {
        let home_config = home_dir.join(".krill").join("krill.yaml");
        if home_config.exists() {
            debug!("Using config from home directory: {:?}", home_config);
            return Ok(home_config);
        }
    }

    // Not found anywhere
    Err(anyhow!(
        "Configuration file not found. Tried:\n\
         1. --config argument\n\
         2. KRILL_CONFIG environment variable\n\
         3. ./krill.yaml\n\
         4. ~/.krill/krill.yaml\n\n\
         Please create a krill.yaml configuration file or specify --config <path>"
    ))
}
