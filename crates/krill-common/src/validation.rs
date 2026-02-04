// Phase 1.4: Shell Command Validator

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ValidationError {
    #[error("Empty command")]
    EmptyCommand,

    #[error("Command contains dangerous pattern: {0}")]
    DangerousPattern(String),

    #[error("Command too long (max 4096 characters)")]
    CommandTooLong,

    #[error("Invalid shell metacharacter usage: {0}")]
    InvalidMetacharacter(String),
}

const MAX_COMMAND_LENGTH: usize = 4096;

// Forbidden shell metacharacters and patterns (per roadmap v2)
const FORBIDDEN_PATTERNS: &[(&str, &str)] = &[
    ("|", "pipes"),
    (";", "semicolon chaining"),
    ("&&", "AND chaining"),
    ("||", "OR chaining"),
    ("$(", "command substitution"),
    ("`", "backtick substitution"),
    (">", "output redirection"),
    ("<", "input redirection"),
    ("&", "background execution"),
];

/// Validate a shell command for safety
pub fn validate_shell_command(command: &str) -> Result<(), ValidationError> {
    // Check if empty
    if command.trim().is_empty() {
        return Err(ValidationError::EmptyCommand);
    }

    // Check length
    if command.len() > MAX_COMMAND_LENGTH {
        return Err(ValidationError::CommandTooLong);
    }

    // Check for forbidden patterns
    for (pattern, desc) in FORBIDDEN_PATTERNS {
        if command.contains(pattern) {
            return Err(ValidationError::DangerousPattern(format!(
                "Contains {} ('{}'). Use a pixi task instead.",
                desc, pattern
            )));
        }
    }

    // Check for unbalanced quotes
    let single_quotes = command.chars().filter(|&c| c == '\'').count();
    let double_quotes = command.chars().filter(|&c| c == '"').count();

    if single_quotes % 2 != 0 {
        return Err(ValidationError::InvalidMetacharacter(
            "unbalanced single quotes".to_string(),
        ));
    }

    if double_quotes % 2 != 0 {
        return Err(ValidationError::InvalidMetacharacter(
            "unbalanced double quotes".to_string(),
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_command() {
        assert!(validate_shell_command("echo hello").is_ok());
        assert!(validate_shell_command("python script.py --arg value").is_ok());
        assert!(validate_shell_command("ls -la /tmp").is_ok());
    }

    #[test]
    fn test_empty_command() {
        assert!(matches!(
            validate_shell_command(""),
            Err(ValidationError::EmptyCommand)
        ));
        assert!(matches!(
            validate_shell_command("   "),
            Err(ValidationError::EmptyCommand)
        ));
    }

    #[test]
    fn test_dangerous_patterns() {
        // Test pipe
        assert!(matches!(
            validate_shell_command("ls | grep foo"),
            Err(ValidationError::DangerousPattern(_))
        ));

        // Test semicolon
        assert!(matches!(
            validate_shell_command("echo hello; echo world"),
            Err(ValidationError::DangerousPattern(_))
        ));

        // Test command substitution
        assert!(matches!(
            validate_shell_command("echo $(date)"),
            Err(ValidationError::DangerousPattern(_))
        ));

        // Test redirection
        assert!(matches!(
            validate_shell_command("echo hello > file.txt"),
            Err(ValidationError::DangerousPattern(_))
        ));

        // Test background execution
        assert!(matches!(
            validate_shell_command("sleep 10 &"),
            Err(ValidationError::DangerousPattern(_))
        ));
    }

    #[test]
    fn test_unbalanced_quotes() {
        assert!(matches!(
            validate_shell_command("echo 'hello"),
            Err(ValidationError::InvalidMetacharacter(_))
        ));
        assert!(matches!(
            validate_shell_command(r#"echo "world"#),
            Err(ValidationError::InvalidMetacharacter(_))
        ));
    }

    #[test]
    fn test_balanced_quotes() {
        assert!(validate_shell_command("echo 'hello world'").is_ok());
        assert!(validate_shell_command(r#"echo "hello world""#).is_ok());
    }

    #[test]
    fn test_command_too_long() {
        let long_cmd = "a".repeat(5000);
        assert!(matches!(
            validate_shell_command(&long_cmd),
            Err(ValidationError::CommandTooLong)
        ));
    }
}
