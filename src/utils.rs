//! Shared utility functions for the spuff CLI.
//!
//! This module contains common formatting and helper functions used across
//! multiple commands to avoid code duplication.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

/// Formats a duration in seconds into a human-readable string.
///
/// # Examples
///
/// ```
/// use spuff::utils::format_duration;
///
/// assert_eq!(format_duration(3661), "1h 1m 1s");
/// assert_eq!(format_duration(61), "1m 1s");
/// assert_eq!(format_duration(30), "30s");
/// ```
pub fn format_duration(seconds: i64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    match (hours, minutes) {
        (0, 0) => format!("{}s", secs),
        (0, _) => format!("{}m {}s", minutes, secs),
        _ => format!("{}h {}m {}s", hours, minutes, secs),
    }
}

/// Formats the elapsed time since a given timestamp into a human-readable string.
///
/// # Examples
///
/// ```ignore
/// use chrono::Utc;
/// use spuff::utils::format_elapsed;
///
/// let start = Utc::now() - chrono::Duration::hours(2) - chrono::Duration::minutes(30);
/// assert_eq!(format_elapsed(start), "2h 30m");
/// ```
pub fn format_elapsed(since: chrono::DateTime<chrono::Utc>) -> String {
    let duration = chrono::Utc::now() - since;
    let hours = duration.num_hours();
    let minutes = duration.num_minutes() % 60;

    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else {
        format!("{}m", minutes)
    }
}

/// Formats a byte count into a human-readable string with appropriate units.
///
/// Uses binary units (KiB, MiB, GiB) with one decimal place.
///
/// # Examples
///
/// ```
/// use spuff::utils::format_bytes;
///
/// assert_eq!(format_bytes(500), "500 B");
/// assert_eq!(format_bytes(1536), "1.5 KB");
/// assert_eq!(format_bytes(1073741824), "1.0 GB");
/// ```
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Truncates a string to a maximum length, appending "..." if truncated.
///
/// Returns a `Cow<str>` to avoid allocation when no truncation is needed.
///
/// # Examples
///
/// ```
/// use spuff::utils::truncate;
///
/// assert_eq!(truncate("hello", 10), "hello");
/// assert_eq!(truncate("hello world", 8), "hello...");
/// ```
pub fn truncate(s: &str, max_len: usize) -> Cow<'_, str> {
    if s.len() <= max_len {
        Cow::Borrowed(s)
    } else if max_len <= 3 {
        Cow::Borrowed(&s[..max_len])
    } else {
        Cow::Owned(format!("{}...", &s[..max_len - 3]))
    }
}

/// Validates and canonicalizes a path, ensuring it stays within a base directory.
///
/// This function is critical for preventing path traversal attacks. It:
/// 1. Canonicalizes the input path to resolve all symlinks and `..` components
/// 2. Verifies the resulting path is within the allowed base directory
///
/// # Arguments
///
/// * `path` - The path to validate
/// * `allowed_base` - The base directory that the path must be within
///
/// # Returns
///
/// * `Ok(PathBuf)` - The canonicalized path if valid
/// * `Err(PathValidationError)` - If the path is invalid or escapes the base
///
/// # Examples
///
/// ```
/// use spuff::utils::validate_path_within;
///
/// // Valid path
/// let result = validate_path_within("/var/log/syslog", "/var/log");
/// assert!(result.is_ok());
///
/// // Path traversal attempt
/// let result = validate_path_within("/var/log/../etc/passwd", "/var/log");
/// assert!(result.is_err());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum PathValidationError {
    /// The path does not exist or cannot be canonicalized
    InvalidPath(String),
    /// The path escapes the allowed base directory
    PathTraversal { path: PathBuf, base: PathBuf },
}

impl std::fmt::Display for PathValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPath(msg) => write!(f, "Invalid path: {}", msg),
            Self::PathTraversal { path, base } => {
                write!(
                    f,
                    "Path traversal detected: '{}' escapes base '{}'",
                    path.display(),
                    base.display()
                )
            }
        }
    }
}

impl std::error::Error for PathValidationError {}

pub fn validate_path_within(
    path: impl AsRef<Path>,
    allowed_base: impl AsRef<Path>,
) -> Result<PathBuf, PathValidationError> {
    let path = path.as_ref();
    let allowed_base = allowed_base.as_ref();

    // Canonicalize the base (it must exist)
    let canonical_base = allowed_base
        .canonicalize()
        .map_err(|e| PathValidationError::InvalidPath(format!("Base path error: {}", e)))?;

    // Canonicalize the input path (it must exist)
    let canonical_path = path
        .canonicalize()
        .map_err(|e| PathValidationError::InvalidPath(format!("{}: {}", path.display(), e)))?;

    // Check that the canonical path starts with the canonical base
    if canonical_path.starts_with(&canonical_base) {
        Ok(canonical_path)
    } else {
        Err(PathValidationError::PathTraversal {
            path: canonical_path,
            base: canonical_base,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_seconds_only() {
        assert_eq!(format_duration(0), "0s");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_minutes_and_seconds() {
        assert_eq!(format_duration(60), "1m 0s");
        assert_eq!(format_duration(61), "1m 1s");
        assert_eq!(format_duration(3599), "59m 59s");
    }

    #[test]
    fn test_format_duration_with_hours() {
        assert_eq!(format_duration(3600), "1h 0m 0s");
        assert_eq!(format_duration(3661), "1h 1m 1s");
        assert_eq!(format_duration(86400), "24h 0m 0s");
    }

    #[test]
    fn test_format_bytes_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1023), "1023 B");
    }

    #[test]
    fn test_format_bytes_kilobytes() {
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1536), "1.5 KB");
    }

    #[test]
    fn test_format_bytes_megabytes() {
        assert_eq!(format_bytes(1048576), "1.0 MB");
        assert_eq!(format_bytes(1572864), "1.5 MB");
    }

    #[test]
    fn test_format_bytes_gigabytes() {
        assert_eq!(format_bytes(1073741824), "1.0 GB");
    }

    #[test]
    fn test_truncate_no_truncation_needed() {
        let result = truncate("hello", 10);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_exact_length() {
        let result = truncate("hello", 5);
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_with_ellipsis() {
        let result = truncate("hello world", 8);
        assert!(matches!(result, Cow::Owned(_)));
        assert_eq!(result, "hello...");
    }

    #[test]
    fn test_truncate_very_short_max() {
        assert_eq!(truncate("hello", 3), "hel");
        assert_eq!(truncate("hello", 2), "he");
    }

    #[test]
    fn test_validate_path_within_valid() {
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        std::fs::write(&file_path, "test").unwrap();

        let result = validate_path_within(&file_path, temp_dir.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_within_traversal() {
        let temp_dir = tempfile::tempdir().unwrap();
        let base = temp_dir.path().join("base");
        std::fs::create_dir(&base).unwrap();

        let outside_file = temp_dir.path().join("outside.txt");
        std::fs::write(&outside_file, "test").unwrap();

        // Try to access file outside base using path traversal
        let traversal_path = base.join("..").join("outside.txt");
        let result = validate_path_within(&traversal_path, &base);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PathValidationError::PathTraversal { .. }
        ));
    }

    #[test]
    fn test_validate_path_within_nonexistent() {
        let result = validate_path_within("/nonexistent/path", "/var/log");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            PathValidationError::InvalidPath(_)
        ));
    }
}
