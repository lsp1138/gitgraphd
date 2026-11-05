use std::io::Write;
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
    // Give less a LESS of our own, with mouse/trackpad scrolling where the
    // installed less supports it, and tell delta to always page (a no-op
    // for any other pager).
    let less = std::env::var("LESS").unwrap_or_default();
    let mouse = if less_supports_mouse() { " --mouse" } else { "" };
    cmd.env("LESS", format!("{less} -R{mouse}").trim().to_string());
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

/// Page already-coloured text through the same pager git would use (the
/// `GG_PAGER` override, else git's own choice), so a diff gg assembled
/// itself looks the same as one `git show` produced. Returns when the
/// pager exits.
pub fn page(text: &[u8]) -> Result<()> {
    let pager = match std::env::var("GG_PAGER") {
        Ok(p) if !p.is_empty() => p,
        _ => {
            let out = Command::new("git").args(["var", "GIT_PAGER"]).output();
            match out {
                Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
                _ => "less".to_string(),
            }
        }
    };
    if pager == "cat" {
        std::io::stdout().write_all(text)?;
        return Ok(());
    }
    let less = std::env::var("LESS").unwrap_or_default();
    let mouse = if less_supports_mouse() { " --mouse" } else { "" };
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&pager)
        .env("LESS", format!("{less} -R{mouse}").trim().to_string())
        // the same `-c delta.paging=always` git would export to delta
        .env("GIT_CONFIG_PARAMETERS", "'delta.paging=always'")
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("could not run pager '{pager}'"))?;
    if let Some(mut stdin) = child.stdin.take() {
        // quitting the pager early closes the pipe; that is not an error
        let _ = stdin.write_all(text);
    }
    let status = child.wait().context("pager")?;
    if !status.success() {
        bail!("pager exited with {status}");
    }
    Ok(())
}

/// `--mouse` arrived in less 530; the less that ships with macOS is older
/// and would refuse to start with it. Checked once per process.
fn less_supports_mouse() -> bool {
    use std::sync::OnceLock;
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        let Ok(out) = Command::new("less").arg("--version").output() else {
            return false;
        };
        // first line: "less 668 (GNU regular expressions)"
        String::from_utf8_lossy(&out.stdout)
            .split_whitespace()
            .nth(1)
            .and_then(|v| v.parse::<u32>().ok())
            .is_some_and(|v| v >= 530)
    })
}
