//! HTTP helpers for agent communication
//!
//! Functions for making HTTP requests to the agent via SSH tunnel.

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};

/// Find the start of a JSON array, skipping ANSI escape sequences.
fn find_json_array_start(output: &str) -> Option<usize> {
    let bytes = output.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'[' {
            // Skip if this is an ANSI escape sequence (preceded by ESC 0x1b)
            if i > 0 && bytes[i - 1] == 0x1b {
                continue;
            }
            // Check if it looks like a JSON array: '[{', '["', '[n' (number), or '[]'
            if i + 1 < bytes.len() {
                let next = bytes[i + 1];
                if next == b'{' || next == b'"' || next == b']' || next.is_ascii_digit() {
                    return Some(i);
                }
            }
        }
    }
    None
}

/// Extract JSON from output that may contain banner text before/after.
///
/// Priority: whichever appears first in the output (array or object).
fn extract_json(output: &str) -> &str {
    let brace_pos = output.find('{');
    let bracket_pos = find_json_array_start(output);

    // Determine which comes first: array or object
    let (start_pos, is_array) = match (bracket_pos, brace_pos) {
        (Some(b), Some(o)) => {
            if b < o {
                (b, true)
            } else {
                (o, false)
            }
        }
        (Some(b), None) => (b, true),
        (None, Some(o)) => (o, false),
        (None, None) => return output.trim(),
    };

    if is_array {
        // Extract JSON array
        let mut depth = 0;
        let mut end_pos = None;
        for (i, c) in output[start_pos..].char_indices() {
            match c {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = Some(start_pos + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(end) = end_pos {
            return &output[start_pos..=end];
        }
    } else {
        // Extract JSON object
        let mut depth = 0;
        let mut end_pos = None;
        for (i, c) in output[start_pos..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end_pos = Some(start_pos + i);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(end) = end_pos {
            return &output[start_pos..=end];
        }
    }

    output.trim()
}

pub async fn agent_request<T: serde::de::DeserializeOwned>(
    ip: &str,
    config: &AppConfig,
    endpoint: &str,
) -> Result<T> {
    // Use SSH to tunnel to the agent
    let output = crate::connector::ssh::run_command(
        ip,
        config,
        &format!("curl -s http://127.0.0.1:7575{}", endpoint),
    )
    .await?;

    // Extract JSON from output (may contain banner text from shell profile)
    let json_str = extract_json(&output);

    serde_json::from_str(json_str).map_err(|e| {
        SpuffError::Provider(format!(
            "Failed to parse agent response: {}. Response: {}",
            e, output
        ))
    })
}

/// Make a POST request to the agent with JSON body.
pub async fn agent_request_post<T: serde::de::DeserializeOwned>(
    ip: &str,
    config: &AppConfig,
    endpoint: &str,
    body: &serde_json::Value,
) -> Result<T> {
    // Escape single quotes in JSON for shell
    let json_body = body.to_string().replace('\'', "'\\''");

    let output = crate::connector::ssh::run_command(
        ip,
        config,
        &format!(
            "curl -s -X POST -H 'Content-Type: application/json' -d '{}' http://127.0.0.1:7575{}",
            json_body, endpoint
        ),
    )
    .await?;

    // Extract JSON from output (may contain banner text from shell profile)
    let json_str = extract_json(&output);

    serde_json::from_str(json_str).map_err(|e| {
        SpuffError::Provider(format!(
            "Failed to parse agent response: {}. Response: {}",
            e, output
        ))
    })
}
