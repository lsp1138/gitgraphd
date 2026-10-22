use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Show one commit's diff with the terminal attached, so git runs its own
/// `core.pager` (delta-auto on this machine). `GG_PAGER` overrides the pager
/// for gg only. With `stat`, only the message and file summary are shown.
/// Returns when the pager exits.
pub fn show(hash: &str, stat: bool) -> Result<()> {
    let mut cmd = Command::new("git");
    if let Ok(pager) = std::env::var("GG_PAGER")
        && !pager.is_empty()
    {
        cmd.arg("-c").arg(format!("core.pager={pager}"));
    }
    // Two things make a short diff vanish before we get the screen back:
    // git sets LESS=FRX when LESS is unset (F = quit if one screen), and
    // delta's default paging mode adds --quit-if-one-screen on its own.
    // Give less a LESS of our own, with mouse/trackpad scrolling, and tell
    // delta to always page (a no-op for any other pager).
    let less = std::env::var("LESS").unwrap_or_default();
    cmd.env("LESS", format!("{less} -R --mouse").trim().to_string());
    cmd.args(["-c", "delta.paging=always", "show"]);
    if stat {
        cmd.arg("--stat");
    }
    let status = cmd
        .arg(hash)
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
