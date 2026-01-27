//! Agent cross-compilation for Linux
//!
//! Functions for building the spuff-agent binary for Linux x86_64
//! using cargo-zigbuild, cross, or native cargo.

use std::process::Stdio;

use crate::error::Result;

/// Linux x86_64 target for cross-compilation (musl for static linking)
pub const LINUX_TARGET: &str = "x86_64-unknown-linux-musl";

pub fn get_linux_agent_path() -> String {
    format!("target/{}/release/spuff-agent", LINUX_TARGET)
}

/// Get the cargo binary path, checking common locations
fn get_cargo_path() -> String {
    // Check if cargo is in PATH
    if let Ok(path) = which::which("cargo") {
        return path.to_string_lossy().to_string();
    }

    // Check common locations
    if let Ok(home) = std::env::var("HOME") {
        let cargo_home = format!("{}/.cargo/bin/cargo", home);
        if std::path::Path::new(&cargo_home).exists() {
            return cargo_home;
        }
    }

    // Fallback to just "cargo" and let it fail if not found
    "cargo".to_string()
}

/// Check if cargo-zigbuild is installed
async fn has_cargo_zigbuild() -> bool {
    let cargo = get_cargo_path();
    // cargo zigbuild doesn't have --version, use --help instead
    tokio::process::Command::new(&cargo)
        .args(["zigbuild", "--help"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if cross is installed
async fn has_cross() -> bool {
    // Check common location
    if let Ok(home) = std::env::var("HOME") {
        let cross_path = format!("{}/.cargo/bin/cross", home);
        if std::path::Path::new(&cross_path).exists() {
            return tokio::process::Command::new(&cross_path)
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await
                .map(|s| s.success())
                .unwrap_or(false);
        }
    }

    // Try PATH
    tokio::process::Command::new("cross")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Build the agent for Linux x86_64 using cross-compilation
pub async fn build_linux_agent() -> Result<String> {
    let agent_path = get_linux_agent_path();
    let cargo = get_cargo_path();

    // Determine which build tool to use (in order of preference)
    let (cmd, args): (String, Vec<&str>) = if has_cargo_zigbuild().await {
        // cargo-zigbuild: best option for macOS, no Docker needed
        (
            cargo.clone(),
            vec![
                "zigbuild",
                "--release",
                "--bin",
                "spuff-agent",
                "--target",
                LINUX_TARGET,
            ],
        )
    } else if has_cross().await {
        // cross: requires Docker but works well
        let cross_path = if let Ok(home) = std::env::var("HOME") {
            let path = format!("{}/.cargo/bin/cross", home);
            if std::path::Path::new(&path).exists() {
                path
            } else {
                "cross".to_string()
            }
        } else {
            "cross".to_string()
        };
        (
            cross_path,
            vec![
                "build",
                "--release",
                "--bin",
                "spuff-agent",
                "--target",
                LINUX_TARGET,
            ],
        )
    } else {
        // Fallback to cargo (requires linker setup)
        (
            cargo.clone(),
            vec![
                "build",
                "--release",
                "--bin",
                "spuff-agent",
                "--target",
                LINUX_TARGET,
            ],
        )
    };

    println!(
        "  {} {} {}",
        console::style("→").cyan(),
        console::style(&cmd).white(),
        console::style(args.join(" ")).dim()
    );

    let status = tokio::process::Command::new(&cmd)
        .args(&args)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await?;

    if !status.success() {
        return Err(crate::error::SpuffError::Build(format!(
            "Failed to build spuff-agent for {}.\n\
             Install cross-compilation support:\n  \
             cargo install cargo-zigbuild && brew install zig\n  \
             Or: cargo install cross (requires Docker)",
            LINUX_TARGET
        )));
    }

    if !std::path::Path::new(&agent_path).exists() {
        return Err(crate::error::SpuffError::Build(format!(
            "Build succeeded but binary not found at {}",
            agent_path
        )));
    }

    Ok(agent_path)
}
