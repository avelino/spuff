use console::style;
use dialoguer::{Input, Select};

use crate::config::AppConfig;
use crate::error::Result;

pub async fn execute() -> Result<()> {
    println!("{}", style("🚀 Welcome to spuff!").bold().cyan());
    println!("Let's configure your ephemeral dev environment.\n");

    let providers = vec!["DigitalOcean", "Hetzner", "AWS"];
    let provider_idx = Select::new()
        .with_prompt("Select your cloud provider")
        .items(&providers)
        .default(0)
        .interact()?;

    let provider = match provider_idx {
        0 => "digitalocean",
        1 => "hetzner",
        2 => "aws",
        _ => "digitalocean",
    };

    let api_token: String = Input::new()
        .with_prompt(format!("Enter your {} API token", providers[provider_idx]))
        .interact_text()?;

    let regions = get_regions_for_provider(provider);
    let region_idx = Select::new()
        .with_prompt("Select default region")
        .items(&regions)
        .default(0)
        .interact()?;

    let sizes = get_sizes_for_provider(provider);
    let size_idx = Select::new()
        .with_prompt("Select default instance size")
        .items(&sizes)
        .default(1)
        .interact()?;

    let idle_timeout: String = Input::new()
        .with_prompt("Auto-destroy after idle (e.g., 2h, 30m)")
        .default("2h".to_string())
        .interact_text()?;

    let environments = vec!["devbox", "nix", "docker"];
    let env_idx = Select::new()
        .with_prompt("Environment type")
        .items(&environments)
        .default(0)
        .interact()?;

    let dotfiles: String = Input::new()
        .with_prompt("Dotfiles repository (optional)")
        .allow_empty(true)
        .interact_text()?;

    let ssh_key_path: String = Input::new()
        .with_prompt("SSH private key path")
        .default("~/.ssh/id_ed25519".to_string())
        .interact_text()?;

    let config = AppConfig {
        provider: provider.to_string(),
        api_token,
        region: regions[region_idx].to_string(),
        size: sizes[size_idx].to_string(),
        idle_timeout,
        environment: environments[env_idx].to_string(),
        dotfiles: if dotfiles.is_empty() {
            None
        } else {
            Some(dotfiles)
        },
        ssh_key_path: shellexpand::tilde(&ssh_key_path).to_string(),
        ssh_user: "dev".to_string(),
        tailscale_enabled: false,
        tailscale_authkey: None,
        agent_token: None, // Can be set via SPUFF_AGENT_TOKEN env var
        ai_tools: None,    // None means use default (all)
        volumes: Vec::new(),
    };

    config.save()?;

    println!("\n{}", style("✓ Configuration saved!").green().bold());
    println!(
        "Config file: {}",
        style(AppConfig::config_path()?.display()).dim()
    );
    println!(
        "\nRun {} to create your first environment.",
        style("spuff up").cyan()
    );

    Ok(())
}

fn get_regions_for_provider(provider: &str) -> Vec<&'static str> {
    match provider {
        "digitalocean" => vec!["nyc1", "nyc3", "sfo3", "ams3", "lon1", "fra1", "sgp1"],
        "hetzner" => vec!["fsn1", "nbg1", "hel1", "ash", "hil"],
        "aws" => vec![
            "us-east-1",
            "us-west-2",
            "eu-west-1",
            "eu-central-1",
            "ap-southeast-1",
        ],
        _ => vec!["default"],
    }
}

fn get_sizes_for_provider(provider: &str) -> Vec<&'static str> {
    match provider {
        "digitalocean" => vec!["s-1vcpu-1gb", "s-2vcpu-4gb", "s-4vcpu-8gb", "s-8vcpu-16gb"],
        "hetzner" => vec!["cx22", "cx32", "cx42", "cx52"],
        "aws" => vec!["t3.small", "t3.medium", "t3.large", "t3.xlarge"],
        _ => vec!["default"],
    }
}
