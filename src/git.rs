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
    /// Committer date as git formatted it (`--date=...`, relative by default).
    pub date: String,
    /// Committer time as `YYYY-MM-DD HH:MM` in the committer's own timezone.
    pub timestamp: String,
    /// Committer time as seconds since the epoch, for compact ages.
    pub time: i64,
    /// Raw `%D` decoration string, e.g. `HEAD -> main, origin/main, tag: v1`.
    pub refs: String,
    pub subject: String,
}

impl Commit {
    /// The row for uncommitted changes: no hash, sits on top of HEAD.
    pub fn worktree(w: &Worktree, now: i64) -> Self {
        Self {
            hash: String::new(),
            parents: vec![w.head.clone()],
            author: String::new(),
            date: "now".into(),
            timestamp: "now".into(),
            time: now,
            refs: String::new(),
            subject: format!("Uncommitted changes  {}", w.summary()),
        }
    }

    /// True for the synthetic uncommitted-changes row.
    pub fn is_worktree(&self) -> bool {
        self.hash.is_empty()
    }

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

// Committer dates, because --date-order sorts by them; author dates of
// rebased or cherry-picked commits would look out of sequence.
const FORMAT: &str = "--format=%H%x00%P%x00%an%x00%cd%x00%cI%x00%ct%x00%D%x00%s";

#[derive(Debug, Clone)]
pub struct LogOpts {
    pub all: bool,
    pub max_count: Option<usize>,
    /// A git date format to show instead of the compact age.
    pub date: Option<String>,
    pub order: String,
}

/// Start `git log` and stream its commits through a channel. If git fails,
/// the last message is the error; the channel closes once git is done.
/// Dropping the receiver kills git.
pub fn stream(opts: &LogOpts) -> Result<Receiver<Result<Commit>>> {
    let mut cmd = Command::new("git");
    cmd.args(["log", "--decorate=full", FORMAT]);
    cmd.arg(format!("--{}-order", opts.order));
    cmd.arg(format!("--date={}", opts.date.as_deref().unwrap_or("relative")));
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

/// Names of branches that exist on any remote (`origin/dev` -> `dev`),
/// so a local branch can be told apart from one that was never pushed.
pub fn remote_branches() -> std::collections::HashSet<String> {
    let Ok(out) = Command::new("git")
        .args(["for-each-ref", "--format=%(refname:short)", "refs/remotes/"])
        .output()
    else {
        return Default::default();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split_once('/').map(|(_, branch)| branch.to_string()))
        .filter(|b| b != "HEAD")
        .collect()
}

/// True when git has a pager of its own configured (config or environment).
pub fn has_pager_config() -> bool {
    if ["GIT_PAGER", "PAGER"].iter().any(|v| std::env::var_os(v).is_some_and(|p| !p.is_empty())) {
        return true;
    }
    Command::new("git")
        .args(["config", "--get", "core.pager"])
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
}

/// Is `delta` somewhere on the PATH?
pub fn delta_on_path() -> bool {
    let Some(path) = std::env::var_os("PATH") else { return false };
    std::env::split_paths(&path).any(|dir| dir.join("delta").is_file())
}

/// `git checkout <target>`; returns git's own one-line report.
pub fn checkout(target: &str) -> Result<String> {
    let out = Command::new("git")
        .args(["checkout", target])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .context("could not run git checkout")?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    let report = stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string();
    if !out.status.success() {
        return Err(anyhow!("{}", if report.is_empty() { "git checkout failed" } else { &report }));
    }
    Ok(if report.is_empty() { format!("checked out {target}") } else { report })
}

/// `git branch -d <name>`: the safe delete, which refuses unmerged work.
pub fn delete_branch(name: &str) -> Result<String> {
    let out = Command::new("git")
        .args(["branch", "-d", name])
        .stdin(Stdio::null())
        .output()
        .context("could not run git branch")?;
    let text = String::from_utf8_lossy(if out.status.success() { &out.stdout } else { &out.stderr });
    let line = text
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
        .trim_start_matches("error: ")
        .to_string();
    if !out.status.success() {
        return Err(anyhow!("{}", if line.is_empty() { "git branch -d failed".into() } else { line }));
    }
    Ok(if line.is_empty() { format!("deleted branch {name}") } else { line })
}

/// `git fetch --all --prune` without any prompting. Returns a one-line summary.
pub fn fetch() -> Result<String> {
    let out = Command::new("git")
        .args(["fetch", "--all", "--prune"])
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
    let mut f = line.splitn(8, '\0');
    let hash = f.next()?.to_string();
    let parents = f
        .next()?
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let author = f.next()?.to_string();
    let date = f.next()?.to_string();
    // 2026-10-22T15:14:03+01:00 -> 2026-10-22 15:14
    let iso = f.next()?;
    let timestamp = match iso.get(..16) {
        Some(t) => t.replacen('T', " ", 1),
        None => iso.to_string(),
    };
    let time = f.next()?.trim().parse().unwrap_or(0);
    let refs = f.next()?.to_string();
    let subject = f.next().unwrap_or("").to_string();
    Some(Commit {
        hash,
        parents,
        author,
        date,
        timestamp,
        time,
        refs,
        subject,
    })
}

/// What is in the working tree beyond HEAD: counts for the graph row and
/// the untracked paths for the diff. `None` when the tree is clean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// The commit the changes sit on top of.
    pub head: String,
    pub staged: usize,
    /// Tracked files changed but not staged (including conflicts).
    pub modified: usize,
    pub untracked: Vec<String>,
}

impl Worktree {
    /// Text for the graph row: `2 staged · 3 modified · 1 untracked`.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.staged > 0 {
            parts.push(format!("{} staged", self.staged));
        }
        if self.modified > 0 {
            parts.push(format!("{} modified", self.modified));
        }
        if !self.untracked.is_empty() {
            parts.push(format!("{} untracked", self.untracked.len()));
        }
        parts.join(" · ")
    }
}

/// Ask git what is uncommitted on top of HEAD. Errors (no HEAD yet, not a
/// repository) count as clean.
pub fn worktree() -> Option<Worktree> {
    let head = Command::new("git").args(["rev-parse", "HEAD"]).output().ok()?;
    if !head.status.success() {
        return None;
    }
    let head = String::from_utf8_lossy(&head.stdout).trim().to_string();
    let out = Command::new("git")
        .args(["status", "--porcelain=v1", "-z", "--untracked-files=all"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_status(&String::from_utf8_lossy(&out.stdout), head)
}

/// Parse `git status --porcelain=v1 -z` output.
fn parse_status(out: &str, head: String) -> Option<Worktree> {
    let mut w = Worktree { head, staged: 0, modified: 0, untracked: Vec::new() };
    let mut entries = out.split('\0');
    while let Some(entry) = entries.next() {
        if entry.len() < 4 {
            continue;
        }
        let (xy, path) = entry.split_at(3);
        let x = xy.as_bytes()[0];
        let y = xy.as_bytes()[1];
        if xy.starts_with("??") {
            w.untracked.push(path.to_string());
            continue;
        }
        if x == b'R' || x == b'C' {
            // a rename carries the old path in the next entry
            entries.next();
        }
        if x != b' ' {
            w.staged += 1;
        }
        if y != b' ' {
            w.modified += 1;
        }
    }
    (w.staged + w.modified + w.untracked.len() > 0).then_some(w)
}

/// How many untracked files get shown in full.
const MAX_UNTRACKED_DIFFS: usize = 50;

/// The staged diff, the unstaged diff and every untracked file as an
/// addition, in that order, each under a header, coloured for the pager.
pub fn worktree_diff(untracked: &[String]) -> Result<Vec<u8>> {
    fn diff(args: &[&str]) -> Result<Vec<u8>> {
        let out = Command::new("git")
            .args(["-c", "color.ui=always", "diff", "--color=always"])
            .args(args)
            .output()
            .context("could not run git diff")?;
        // --no-index exits 1 whenever the files differ
        Ok(out.stdout)
    }
    fn header(buf: &mut Vec<u8>, title: &str) {
        if !buf.is_empty() {
            buf.push(b'\n');
        }
        buf.extend_from_slice(format!("\x1b[1m────  {title}  ────\x1b[0m\n\n").as_bytes());
    }
    let mut buf = Vec::new();
    let staged = diff(&["--cached"])?;
    if !staged.is_empty() {
        header(&mut buf, "staged");
        buf.extend_from_slice(&staged);
    }
    let unstaged = diff(&[])?;
    if !unstaged.is_empty() {
        header(&mut buf, "not staged");
        buf.extend_from_slice(&unstaged);
    }
    if !untracked.is_empty() {
        header(&mut buf, "untracked");
        // One git per file; a tree full of untracked build output would
        // take ages and nobody reads a thousand added files anyway.
        for path in untracked.iter().take(MAX_UNTRACKED_DIFFS) {
            buf.extend_from_slice(&diff(&["--no-index", "--", "/dev/null", path])?);
        }
        if untracked.len() > MAX_UNTRACKED_DIFFS {
            buf.extend_from_slice(
                format!("\n... and {} more untracked files\n", untracked.len() - MAX_UNTRACKED_DIFFS)
                    .as_bytes(),
            );
        }
    }
    if buf.is_empty() {
        buf.extend_from_slice(b"nothing uncommitted\n");
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_merge_with_refs() {
        let line =
            "abc\0p1 p2\0Lars\02 hours ago\02026-10-22T15:14:03+01:00\01792000000\0HEAD -> main, tag: v1\0Merge: a\0b";
        let c = parse_line(line).unwrap();
        assert_eq!(c.hash, "abc");
        assert_eq!(c.parents, vec!["p1", "p2"]);
        assert_eq!(c.author, "Lars");
        assert_eq!(c.date, "2 hours ago");
        assert_eq!(c.timestamp, "2026-10-22 15:14");
        assert_eq!(c.time, 1_792_000_000);
        assert_eq!(c.refs, "HEAD -> main, tag: v1");
        // the subject keeps any stray NULs; splitn(6) guarantees that
        assert_eq!(c.subject, "Merge: a\0b");
    }

    #[test]
    fn classifies_full_refs() {
        let c = parse_line(
            "h\0\0a\0d\0t\00\0HEAD -> refs/heads/main, refs/remotes/origin/main, tag: refs/tags/v1, refs/stash\0s",
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
        let c = parse_line("h\0\0a\0d\0t\00\0HEAD, tag: refs/tags/v2\0s").unwrap();
        let refs = c.parsed_refs();
        assert_eq!(refs[0], Ref { name: "HEAD".into(), kind: RefKind::Head, is_head: true });
        assert_eq!(refs[1].kind, RefKind::Tag);
        assert!(c.is_head());
    }

    #[test]
    fn parses_root_commit_without_parents_or_refs() {
        let c = parse_line("abc\0\0Lars\0now\0t\00\0\0init").unwrap();
        assert!(c.parents.is_empty());
        assert_eq!(c.refs, "");
        assert_eq!(c.subject, "init");
    }

    #[test]
    fn status_counts_staged_modified_and_untracked() {
        let out = "M  a.rs\0 M b.rs\0MM c.rs\0?? new.txt\0R  new.rs\0old.rs\0UU war.rs\0";
        let w = parse_status(out, "abc".into()).unwrap();
        assert_eq!(w.head, "abc");
        assert_eq!(w.staged, 4); // a, c, rename, conflict
        assert_eq!(w.modified, 3); // b, c, conflict
        assert_eq!(w.untracked, vec!["new.txt"]);
        assert_eq!(w.summary(), "4 staged · 3 modified · 1 untracked");
    }

    #[test]
    fn clean_status_is_none() {
        assert!(parse_status("", "abc".into()).is_none());
    }
}
