use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Show one commit's diff with the terminal attached, so git runs its own
/// `core.pager` (delta-auto on this machine). `GG_PAGER` overrides the pager
/// for gg only. Returns when the pager exits.
pub fn show(hash: &str) -> Result<()> {
    let mut cmd = Command::new("git");
    if let Ok(pager) = std::env::var("GG_PAGER")
        && !pager.is_empty()
    {
        cmd.arg("-c").arg(format!("core.pager={pager}"));
    }
    let status = cmd
        .args(["show", hash])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("could not run git show")?;
    if !status.success() {
        bail!("git show exited with {status}");
    }
    Ok(())
}
