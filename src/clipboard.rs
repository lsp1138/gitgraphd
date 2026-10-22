//! Put text on the system clipboard: a native tool when one is installed,
//! else the OSC 52 escape, which terminals and tmux forward to the clipboard.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};

const TOOLS: &[(&str, &[&str])] = &[
    ("wl-copy", &[]),
    ("xclip", &["-selection", "clipboard"]),
    ("xsel", &["--clipboard", "--input"]),
    ("pbcopy", &[]),
];

pub fn copy(text: &str) -> Result<()> {
    for (tool, args) in TOOLS {
        let Ok(mut child) = Command::new(tool)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            continue;
        };
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(text.as_bytes()).context("writing to clipboard tool")?;
        }
        if child.wait().is_ok_and(|s| s.success()) {
            return Ok(());
        }
    }
    osc52(text)
}

fn osc52(text: &str) -> Result<()> {
    let mut out = std::io::stdout().lock();
    write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes())).map_err(|e| anyhow!("{e}"))?;
    out.flush().map_err(|e| anyhow!("{e}"))
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |acc, &b| (acc << 8) | b as u32) << (8 * (3 - chunk.len()));
        for i in 0..4 {
            if i <= chunk.len() {
                s.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                s.push('=');
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_known_values() {
        assert_eq!(super::base64(b""), "");
        assert_eq!(super::base64(b"f"), "Zg==");
        assert_eq!(super::base64(b"fo"), "Zm8=");
        assert_eq!(super::base64(b"foo"), "Zm9v");
        assert_eq!(super::base64(b"abc1234"), "YWJjMTIzNA==");
    }
}
