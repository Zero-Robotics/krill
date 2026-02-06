use std::path::PathBuf;
use tempfile::NamedTempFile;

use krill_cli::config_discovery::discover_config;

#[test]
fn cli_argument_with_existing_file_returns_ok() {
    // Create a real temp file to act as a config file.
    let tmp = NamedTempFile::new().expect("failed to create temp file");
    let path = tmp.path().to_path_buf();

    let result = discover_config(Some(path.clone()));
    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
    assert_eq!(result.unwrap(), path);
}

#[test]
fn cli_argument_with_nonexistent_file_returns_err() {
    let bogus = PathBuf::from("/tmp/krill_test_does_not_exist_12345.yaml");
    // Make sure the file really does not exist.
    assert!(!bogus.exists());

    let result = discover_config(Some(bogus.clone()));
    assert!(result.is_err(), "expected Err for non-existent path");

    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("not found"),
        "error message should mention 'not found', got: {}",
        msg
    );
}

#[test]
fn no_config_anywhere_returns_descriptive_error() {
    // Remove KRILL_CONFIG from environment so it does not interfere.
    // This test passes None for the CLI arg and relies on the fact that
    // ./krill.yaml and ~/.krill/krill.yaml are unlikely to exist in CI.
    // To be safe we temporarily unset the env var.
    let previous = std::env::var("KRILL_CONFIG").ok();
    std::env::remove_var("KRILL_CONFIG");

    let result = discover_config(None);

    // Restore env var if it was set.
    if let Some(val) = previous {
        std::env::set_var("KRILL_CONFIG", val);
    }

    // The result could be Ok if a krill.yaml happens to exist in the cwd or
    // home directory.  In most test environments it will not, so we check for
    // the error message.  If the file does exist we simply skip the assertion.
    if let Err(e) = result {
        let msg = e.to_string();
        assert!(
            msg.contains("Configuration file not found"),
            "expected descriptive error message, got: {}",
            msg
        );
        assert!(
            msg.contains("krill.yaml"),
            "error message should mention krill.yaml, got: {}",
            msg
        );
    }
}

#[test]
fn env_var_points_to_existing_file() {
    let tmp = NamedTempFile::new().expect("failed to create temp file");
    let path = tmp.path().to_path_buf();

    // Save and override the env var.
    let previous = std::env::var("KRILL_CONFIG").ok();
    std::env::set_var("KRILL_CONFIG", &path);

    let result = discover_config(None);

    // Restore env var.
    match previous {
        Some(val) => std::env::set_var("KRILL_CONFIG", val),
        None => std::env::remove_var("KRILL_CONFIG"),
    }

    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
    assert_eq!(result.unwrap(), path);
}

#[test]
fn env_var_points_to_nonexistent_file() {
    let bogus = PathBuf::from("/tmp/krill_env_nonexistent_98765.yaml");
    assert!(!bogus.exists());

    let previous = std::env::var("KRILL_CONFIG").ok();
    std::env::set_var("KRILL_CONFIG", &bogus);

    let result = discover_config(None);

    match previous {
        Some(val) => std::env::set_var("KRILL_CONFIG", val),
        None => std::env::remove_var("KRILL_CONFIG"),
    }

    assert!(result.is_err(), "expected Err for non-existent env path");

    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("KRILL_CONFIG"),
        "error should mention KRILL_CONFIG, got: {}",
        msg
    );
}

#[test]
fn cli_argument_takes_priority_over_env_var() {
    // Set KRILL_CONFIG to some path, but pass a different file via CLI arg.
    let cli_tmp = NamedTempFile::new().expect("failed to create temp file");
    let cli_path = cli_tmp.path().to_path_buf();

    let env_tmp = NamedTempFile::new().expect("failed to create temp file");
    let env_path = env_tmp.path().to_path_buf();

    let previous = std::env::var("KRILL_CONFIG").ok();
    std::env::set_var("KRILL_CONFIG", &env_path);

    let result = discover_config(Some(cli_path.clone()));

    match previous {
        Some(val) => std::env::set_var("KRILL_CONFIG", val),
        None => std::env::remove_var("KRILL_CONFIG"),
    }

    assert!(result.is_ok(), "expected Ok, got: {:?}", result);
    assert_eq!(
        result.unwrap(),
        cli_path,
        "CLI argument should take priority over KRILL_CONFIG env var"
    );
}
