use console::style;
use dialoguer::Confirm;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::provider::create_provider;
use crate::state::StateDb;
use crate::utils::format_elapsed;

pub async fn execute(config: &AppConfig, create_snapshot: bool, force: bool) -> Result<()> {
    let db = StateDb::open()?;

    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    super::ssh::print_banner();

    // Instance info card
    println!(
        "  {} {} {}",
        style("●").green().bold(),
        style(&instance.name).white().bold(),
        style(format!("({})", &instance.ip)).dim()
    );
    println!();
    println!("  {}      {}", style("Provider").dim(), &instance.provider);
    println!("  {}        {}", style("Region").dim(), &instance.region);
    println!("  {}          {}", style("Size").dim(), &instance.size);
    println!(
        "  {}        {}",
        style("Uptime").dim(),
        style(format_elapsed(instance.created_at)).yellow()
    );
    println!();

    if !force {
        let confirmed = Confirm::new()
            .with_prompt(format!(
                "  {} Destroy this instance?",
                style("?").cyan().bold()
            ))
            .default(false)
            .interact()?;

        if !confirmed {
            println!();
            println!("  {}", style("Cancelled.").dim());
            return Ok(());
        }
    }

    let provider = create_provider(config)?;

    if create_snapshot {
        println!();
        println!(
            "  {} {}",
            style("◐").cyan(),
            style("Creating snapshot...").dim()
        );
        let snapshot_name = format!("{}-snapshot", instance.name);
        match provider.create_snapshot(&instance.id, &snapshot_name).await {
            Ok(snapshot) => {
                println!(
                    "  {} Snapshot: {}",
                    style("✓").green().bold(),
                    style(&snapshot.id).cyan()
                );
            }
            Err(e) => {
                println!("  {} Snapshot failed: {}", style("!").yellow().bold(), e);
            }
        }
    }

    println!();
    println!(
        "  {} {}",
        style("◐").red(),
        style("Destroying instance...").dim()
    );
    provider.destroy_instance(&instance.id).await?;

    db.remove_instance(&instance.id)?;

    println!(
        "  {} Instance {} destroyed.",
        style("✓").green().bold(),
        style(&instance.name).cyan()
    );
    println!();

    Ok(())
}
