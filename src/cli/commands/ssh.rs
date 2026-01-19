use console::style;

use crate::config::AppConfig;
use crate::error::{Result, SpuffError};
use crate::state::StateDb;

const BANNER: &str = r#"
╔═══════════════════════════╗
║  s p u f f                ║
║  ephemeral dev env        ║
╚═══════════════════════════╝
"#;

pub fn print_banner() {
    println!("{}", style(BANNER).cyan());
}

pub async fn execute(config: &AppConfig) -> Result<()> {
    let db = StateDb::open()?;

    let instance = db
        .get_active_instance()?
        .ok_or(SpuffError::NoActiveInstance)?;

    print_banner();
    println!(
        "  {} {} {}",
        style("→").bold(),
        style(&instance.name).white().bold(),
        style(format!("({})", &instance.ip)).dim()
    );
    println!();

    crate::connector::ssh::connect(&instance.ip, config).await
}
