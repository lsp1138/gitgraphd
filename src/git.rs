use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;

use anyhow::{Context, Result, anyhow};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    /// Detached HEAD, shown on its own.
    Head,
    Branch,
    Remote,
    Tag,
    /// Anything else under refs/, e.g. refs/stash.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    pub name: String,
    pub kind: RefKind,
    /// HEAD points at this branch.
    pub is_head: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub author: String,
    pub date: String,
    /// Raw `%D` decoration string, e.g. `HEAD -> main, origin/main, tag: v1`.
    pub refs: String,
    pub subject: String,
}

impl Commit {
    pub fn short_hash(&self) -> &str {
        &self.hash[..self.hash.len().min(7)]
    }

    /// True for the commit HEAD points at, attached or detached.
    pub fn is_head(&self) -> bool {
        self.refs == "HEAD" || self.refs.starts_with("HEAD -> ") || self.refs.starts_with("HEAD, ")
    }

    /// Typed refs from the `--decorate=full` decoration string.
    pub fn parsed_refs(&self) -> Vec<Ref> {
        self.refs
            .split(", ")
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(|part| {
                if part == "HEAD" {
                    return Ref { name: "HEAD".into(), kind: RefKind::Head, is_head: true };
                }
                let (is_head, name) = match part.strip_prefix("HEAD -> ") {
                    Some(n) => (true, n),
                    None => (false, part),
                };
                let (kind, name) = if let Some(n) = name.strip_prefix("tag: ") {
                    (RefKind::Tag, n.strip_prefix("refs/tags/").unwrap_or(n))
                } else if let Some(n) = name.strip_prefix("refs/heads/") {
                    (RefKind::Branch, n)
                } else if let Some(n) = name.strip_prefix("refs/remotes/") {
                    (RefKind::Remote, n)
                } else {
                    (RefKind::Other, name.strip_prefix("refs/").unwrap_or(name))
                };
                Ref { name: name.into(), kind, is_head }
            })
            .collect()
    }
}

const FORMAT: &str = "--format=%H%x00%P%x00%an%x00%ad%x00%D%x00%s";

#[derive(Debug, Clone)]
pub struct LogOpts {
    pub all: bool,
    pub max_count: Option<usize>,
    pub date: String,
    pub order: String,
}

/// Start `git log` and stream its commits through a channel. If git fails,
/// the last message is the error; the channel closes once git is done.
/// Dropping the receiver kills git.
pub fn stream(opts: &LogOpts) -> Result<Receiver<Result<Commit>>> {
    let mut cmd = Command::new("git");
    cmd.args(["log", "--decorate=full", FORMAT]);
    cmd.arg(format!("--{}-order", opts.order));
    cmd.arg(format!("--date={}", opts.date));
    if opts.all {
        cmd.arg("--all");
    }
    if let Some(n) = opts.max_count {
        cmd.arg(format!("--max-count={n}"));
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not run git")?;
    let stdout = child.stdout.take().expect("stdout is piped");
    let mut stderr = child.stderr.take().expect("stderr is piped");

    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        // Drain stderr separately so a chatty git can never block on it.
        let stderr_reader = thread::spawn(move || {
            let mut s = String::new();
            let _ = stderr.read_to_string(&mut s);
            s
        });
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            let Some(commit) = parse_line(&line) else { continue };
            if tx.send(Ok(commit)).is_err() {
                let _ = child.kill();
                break;
            }
        }
        let status = child.wait();
        let errors = stderr_reader.join().unwrap_or_default();
        let outcome = match status {
            Ok(s) if s.success() => return,
            Ok(_) => anyhow!("{}", errors.trim()),
            Err(e) => anyhow!("git: {e}"),
        };
        let _ = tx.send(Err(outcome));
    });
    Ok(rx)
}

/// `git fetch --all` without any prompting. Returns a one-line summary.
pub fn fetch() -> Result<String> {
    let out = Command::new("git")
        .args(["fetch", "--all"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .context("could not run git fetch")?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() {
        let reason = stderr
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("git fetch failed");
        return Err(anyhow!("{}", reason.trim()));
    }
    // git reports each updated ref on its own line containing "->".
    let updated = stderr.lines().filter(|l| l.contains("->")).count();
    Ok(match updated {
        0 => "fetched: already up to date".to_string(),
        1 => "fetched: 1 ref updated".to_string(),
        n => format!("fetched: {n} refs updated"),
    })
}

fn parse_line(line: &str) -> Option<Commit> {
    let mut f = line.splitn(6, '\0');
    let hash = f.next()?.to_string();
    let parents = f
        .next()?
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let author = f.next()?.to_string();
    let date = f.next()?.to_string();
    let refs = f.next()?.to_string();
    let subject = f.next().unwrap_or("").to_string();
    Some(Commit {
        hash,
        parents,
        author,
        date,
        refs,
        subject,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_merge_with_refs() {
        let line = "abc\0p1 p2\0Lars\02 hours ago\0HEAD -> main, tag: v1\0Merge: a\0b";
        let c = parse_line(line).unwrap();
        assert_eq!(c.hash, "abc");
        assert_eq!(c.parents, vec!["p1", "p2"]);
        assert_eq!(c.author, "Lars");
        assert_eq!(c.date, "2 hours ago");
        assert_eq!(c.refs, "HEAD -> main, tag: v1");
        // the subject keeps any stray NULs; splitn(6) guarantees that
        assert_eq!(c.subject, "Merge: a\0b");
    }

    #[test]
    fn classifies_full_refs() {
        let c = parse_line(
            "h\0\0a\0d\0HEAD -> refs/heads/main, refs/remotes/origin/main, tag: refs/tags/v1, refs/stash\0s",
        )
        .unwrap();
        let refs = c.parsed_refs();
        let view: Vec<(&str, RefKind, bool)> =
            refs.iter().map(|r| (r.name.as_str(), r.kind, r.is_head)).collect();
        assert_eq!(
            view,
            [
                ("main", RefKind::Branch, true),
                ("origin/main", RefKind::Remote, false),
                ("v1", RefKind::Tag, false),
                ("stash", RefKind::Other, false),
            ]
        );
        assert!(c.is_head());
    }

    #[test]
    fn detached_head() {
        let c = parse_line("h\0\0a\0d\0HEAD, tag: refs/tags/v2\0s").unwrap();
        let refs = c.parsed_refs();
        assert_eq!(refs[0], Ref { name: "HEAD".into(), kind: RefKind::Head, is_head: true });
        assert_eq!(refs[1].kind, RefKind::Tag);
        assert!(c.is_head());
    }

    #[test]
    fn parses_root_commit_without_parents_or_refs() {
        let c = parse_line("abc\0\0Lars\0now\0\0init").unwrap();
        assert!(c.parents.is_empty());
        assert_eq!(c.refs, "");
        assert_eq!(c.subject, "init");
    }
}
