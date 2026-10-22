use std::process::Command;

use anyhow::{Context, Result, bail};

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
}

const FORMAT: &str = "--format=%H%x00%P%x00%an%x00%ad%x00%D%x00%s";

/// Run `git log` and parse every commit, in topological order.
pub fn log(all: bool, max_count: Option<usize>) -> Result<Vec<Commit>> {
    let mut cmd = Command::new("git");
    cmd.args(["log", "--topo-order", "--date=relative", FORMAT]);
    if all {
        cmd.arg("--all");
    }
    if let Some(n) = max_count {
        cmd.arg(format!("--max-count={n}"));
    }
    let out = cmd.output().context("could not run git")?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Ok(text.lines().filter_map(parse_line).collect())
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
    fn parses_root_commit_without_parents_or_refs() {
        let c = parse_line("abc\0\0Lars\0now\0\0init").unwrap();
        assert!(c.parents.is_empty());
        assert_eq!(c.refs, "");
        assert_eq!(c.subject, "init");
    }
}
