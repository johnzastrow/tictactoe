//! pt terminal client (placeholder until the TUI milestone).

use std::path::PathBuf;

pub struct Options {
    pub server: String,
    pub user: Option<String>,
    pub ca_cert: Option<PathBuf>,
    pub theme: Option<String>,
}

pub async fn run(_opts: Options) -> anyhow::Result<()> {
    anyhow::bail!("the TUI is not built yet")
}

pub fn forget_session(_server: &str) -> anyhow::Result<()> {
    Ok(())
}
