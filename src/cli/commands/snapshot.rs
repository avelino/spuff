use console::style;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::provider::create_provider;
use crate::state::StateDb;

pub async fn create(config: &AppConfig, name: Option<String>) -> Result<()> {
    let db = StateDb::open()?;
    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    let snapshot_name = name.unwrap_or_else(|| {
        format!(
            "{}-{}",
            instance.name,
            chrono::Utc::now().format("%Y%m%d-%H%M")
        )
    });

    println!(
        "{} Creating snapshot of {}...",
        style("→").cyan().bold(),
        style(&instance.name).cyan()
    );

    let provider = create_provider(config)?;
    let snapshot = provider
        .create_snapshot(&instance.id, &snapshot_name)
        .await?;

    println!(
        "\n{} Snapshot created: {}",
        style("✓").green().bold(),
        style(&snapshot.id).cyan()
    );
    println!("  Name: {}", style(&snapshot.name).dim());

    Ok(())
}

pub async fn list(config: &AppConfig) -> Result<()> {
    let provider = create_provider(config)?;
    let snapshots = provider.list_snapshots().await?;

    if snapshots.is_empty() {
        println!("{}", style("No snapshots found.").dim());
        return Ok(());
    }

    println!("{}", style("Snapshots").bold().cyan());
    println!();

    for snapshot in snapshots {
        println!(
            "  {} {}",
            style(&snapshot.id).cyan(),
            style(&snapshot.name).white()
        );
        if let Some(created) = snapshot.created_at {
            println!(
                "    Created: {}",
                style(created.format("%Y-%m-%d %H:%M")).dim()
            );
        }
    }

    Ok(())
}

pub async fn delete(config: &AppConfig, id: String) -> Result<()> {
    let provider = create_provider(config)?;

    println!(
        "{} Deleting snapshot {}...",
        style("→").yellow().bold(),
        style(&id).cyan()
    );

    provider.delete_snapshot(&id).await?;

    println!("{} Snapshot deleted.", style("✓").green().bold());

    Ok(())
}
