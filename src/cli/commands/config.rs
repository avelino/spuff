use console::style;

use crate::config::AppConfig;
use crate::error::Result;

pub async fn show() -> Result<()> {
    let config_path = AppConfig::config_path()?;

    if !config_path.exists() {
        println!("{}", style("No configuration found.").dim());
        println!(
            "Run {} to create one.",
            style("spuff init").cyan()
        );
        return Ok(());
    }

    let config = AppConfig::load()?;

    println!("{}", style("Current Configuration").bold().cyan());
    println!();
    println!("  Provider:     {}", style(&config.provider).white());
    println!("  Region:       {}", style(&config.region).white());
    println!("  Size:         {}", style(&config.size).white());
    println!("  Idle timeout: {}", style(&config.idle_timeout).yellow());
    println!("  Environment:  {}", style(&config.environment).white());
    print!("  Dotfiles:     ");
    match &config.dotfiles {
        Some(d) => println!("{}", style(d).white()),
        None => println!("{}", style("(none)").dim()),
    }
    println!("  SSH key:      {}", style(&config.ssh_key_path).dim());
    println!(
        "  Tailscale:    {}",
        if config.tailscale_enabled {
            style("enabled").green()
        } else {
            style("disabled").dim()
        }
    );
    println!();
    println!(
        "Config file: {}",
        style(config_path.display()).dim()
    );

    Ok(())
}

pub async fn set(key: String, value: String) -> Result<()> {
    let mut config = AppConfig::load().unwrap_or_default();

    match key.as_str() {
        "provider" => config.provider = value.clone(),
        "region" => config.region = value.clone(),
        "size" => config.size = value.clone(),
        "idle_timeout" | "idle-timeout" => config.idle_timeout = value.clone(),
        "environment" => config.environment = value.clone(),
        "dotfiles" => config.dotfiles = Some(value.clone()),
        "ssh_key" | "ssh-key" => config.ssh_key_path = value.clone(),
        "ssh_user" | "ssh-user" => config.ssh_user = value.clone(),
        "tailscale" => config.tailscale_enabled = value == "true" || value == "enabled",
        _ => {
            println!(
                "{} Unknown config key: {}",
                style("!").yellow().bold(),
                style(&key).red()
            );
            println!("\nAvailable keys:");
            println!("  provider, region, size, idle_timeout, environment,");
            println!("  dotfiles, ssh_key, ssh_user, tailscale");
            return Ok(());
        }
    }

    config.save()?;

    println!(
        "{} Set {} = {}",
        style("✓").green().bold(),
        style(&key).cyan(),
        style(&value).white()
    );

    Ok(())
}

pub async fn edit() -> Result<()> {
    let config_path = AppConfig::config_path()?;

    if !config_path.exists() {
        println!("{}", style("No configuration found.").dim());
        println!(
            "Run {} to create one.",
            style("spuff init").cyan()
        );
        return Ok(());
    }

    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string());

    std::process::Command::new(&editor)
        .arg(&config_path)
        .status()?;

    Ok(())
}
