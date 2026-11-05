use std::collections::HashSet;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;

use crate::git::{Commit, RefKind, Worktree};
use crate::graph::{Graph, Row};

pub enum Action {
    None,
    Quit,
    /// Run `git fetch` in the background and reload when it is done.
    Fetch,
    /// Show the diff of this hash; `stat` asks for the file summary only.
    ShowDiff { hash: String, stat: bool },
    /// Show what is staged, unstaged and untracked, in one pager.
    ShowWorktree,
    /// Put this hash on the clipboard.
    CopyHash(String),
    /// `git checkout` this branch or commit, then reload.
    Checkout(String),
    /// `git branch -d` this local branch, then reload.
    DeleteBranch(String),
}

pub struct App {
    pub commits: Vec<Commit>,
    /// One graph row per commit, same indexing as `commits`.
    pub rows: Vec<Row>,
    pub graph: Graph,
    /// Index of the selected commit.
    pub cursor: usize,
    /// Index of the first visible row.
    pub offset: usize,
    /// Rows available for the list; updated by the UI on every draw.
    pub height: usize,
    pub status: String,
    /// Widest relative date and widest timestamp loaded so far.
    pub date_width: usize,
    pub timestamp_width: usize,
    /// Show the absolute timestamp instead of the age.
    pub show_timestamp: bool,
    /// Show git's own date string (`--date=` given) instead of compact ages.
    pub git_dates: bool,
    /// Terminal background is dark; picks the selection colour.
    pub dark: bool,
    /// Sticky error from the git log stream, shown until quit.
    pub error: Option<String>,
    /// True while git log is still producing commits.
    pub loading: bool,
    /// True while a background `git fetch` runs.
    pub fetching: bool,
    /// After a reload, put the cursor back on this commit when it arrives.
    pending_cursor: Option<String>,
    /// Branch names that exist on some remote; others are drawn framed.
    pub remote_branches: HashSet<String>,
    /// A branch delete waiting for `y`.
    pub confirm_delete: Option<String>,
    /// How graph lines are drawn.
    pub style: LineStyle,
    /// Uncommitted changes, drawn as a row above HEAD; `None` when clean.
    pub worktree: Option<Worktree>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStyle {
    /// Heavy box-drawing lines with square corners.
    Heavy,
    /// Thin box-drawing lines with rounded corners.
    Thin,
    /// `* | / \` for terminals without box-drawing glyphs.
    Ascii,
}

impl App {
    pub fn new(commits: Vec<Commit>) -> Self {
        let mut app = Self {
            commits: Vec::with_capacity(commits.len()),
            rows: Vec::with_capacity(commits.len()),
            graph: Graph::new(),
            cursor: 0,
            offset: 0,
            height: 1,
            status: String::new(),
            date_width: 0,
            timestamp_width: 0,
            show_timestamp: false,
            git_dates: false,
            dark: true,
            error: None,
            loading: false,
            fetching: false,
            pending_cursor: None,
            remote_branches: HashSet::new(),
            confirm_delete: None,
            style: LineStyle::Heavy,
            worktree: None,
        };
        for c in commits {
            app.push(c);
        }
        app
    }

    /// Append one commit (they must arrive in git log order).
    pub fn push(&mut self, commit: Commit) {
        if self.pending_cursor.as_deref() == Some(commit.hash.as_str()) {
            self.pending_cursor = None;
            self.cursor = self.commits.len();
            self.scroll_to_cursor();
        }
        self.date_width = self.date_width.max(UnicodeWidthStr::width(commit.date.as_str()));
        self.timestamp_width =
            self.timestamp_width.max(UnicodeWidthStr::width(commit.timestamp.as_str()));
        self.rows.push(self.graph.push(&commit));
        self.commits.push(commit);
    }

    /// Drop the history ahead of a reload, remembering where the cursor was.
    pub fn reset(&mut self) {
        self.pending_cursor = self.selected().filter(|c| !c.is_worktree()).map(|c| c.hash.clone());
        let lane = self.head_lane();
        self.graph = Graph::with_memory(Graph::memory(&self.commits, &self.rows));
        self.commits.clear();
        self.rows.clear();
        self.cursor = 0;
        self.offset = 0;
        self.date_width = 0;
        self.timestamp_width = 0;
        self.error = None;
        self.loading = true;
        self.push_worktree_row(lane);
    }

    /// Take a fresh `git status`. The row appearing or going away lays the
    /// graph out again; a change of counts only updates the row's text.
    pub fn set_worktree(&mut self, worktree: Option<Worktree>) {
        if worktree == self.worktree {
            return;
        }
        let same_shape = match (&worktree, &self.worktree) {
            (Some(new), Some(old)) => new.head == old.head,
            _ => false,
        };
        self.worktree = worktree;
        if same_shape {
            if let (Some(w), Some(row)) = (&self.worktree, self.commits.first_mut()) {
                row.subject = format!("Uncommitted changes  {}", w.summary());
            }
            return;
        }
        self.relayout();
    }

    /// Lay the graph out again with the same colours, with or without the
    /// uncommitted-changes row. The row goes in HEAD's lane, found by laying
    /// out without it first, so the rows below keep their lanes and colours
    /// instead of all shifting right by one. Called once more when a load
    /// finishes, since the lane was only a guess while commits streamed in.
    pub fn relayout(&mut self) {
        let selected = self.selected().map(|c| c.hash.clone());
        let memory = Graph::memory(&self.commits, &self.rows);
        let commits: Vec<Commit> = self.commits.drain(..).filter(|c| !c.is_worktree()).collect();
        self.rows.clear();
        self.graph = Graph::with_memory(memory.clone());
        let mut rows: Vec<Row> = commits.iter().map(|c| self.graph.push(c)).collect();
        if let Some(w) = &self.worktree {
            let lane = commits.iter().position(|c| c.hash == w.head).map(|i| rows[i].lane);
            self.graph = Graph::with_memory(memory);
            self.push_worktree_row(lane);
            rows = commits.iter().map(|c| self.graph.push(c)).collect();
        }
        self.rows.extend(rows);
        self.commits.extend(commits);
        // Stay on the same commit; a vanished row leaves the cursor on top.
        self.cursor = selected
            .and_then(|h| self.commits.iter().position(|c| c.hash == h))
            .unwrap_or(0);
        self.scroll_to_cursor();
    }

    /// Lane of the commit the uncommitted changes sit on, in the current layout.
    fn head_lane(&self) -> Option<usize> {
        let head = self.worktree.as_ref()?.head.as_str();
        self.commits.iter().position(|c| c.hash == head).map(|i| self.rows[i].lane)
    }

    /// Push the uncommitted row into `lane`. Without a lane (first load, or
    /// HEAD not among the commits) nothing is pushed: `relayout` adds the
    /// row once the load is done and HEAD's lane is known, rather than lay
    /// the whole graph out around a guess and shift it afterwards.
    fn push_worktree_row(&mut self, lane: Option<usize>) {
        if let (Some(w), Some(lane)) = (&self.worktree, lane) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let c = Commit::worktree(w, now);
            self.rows.push(self.graph.push_at(&c, lane));
            self.commits.push(c);
        }
    }

    pub fn selected(&self) -> Option<&Commit> {
        self.commits.get(self.cursor)
    }

    /// The date column's text for a commit, the column's width, and whether
    /// it is right-aligned (compact ages are).
    pub fn date_column(&self, c: &Commit, now: i64) -> (String, usize, bool) {
        if self.show_timestamp {
            (c.timestamp.clone(), self.timestamp_width, false)
        } else if self.git_dates {
            (c.date.clone(), self.date_width, false)
        } else {
            (compact_age(now - c.time), 4, true)
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        self.status.clear();
        if let Some(branch) = self.confirm_delete.take() {
            // Only y goes ahead; anything else cancels and is otherwise ignored.
            if key.code == KeyCode::Char('y') {
                return Action::DeleteBranch(branch);
            }
            self.status = "delete cancelled".into();
            return Action::None;
        }
        match (key.code, ctrl) {
            (KeyCode::Char('q'), _) | (KeyCode::Esc, _) | (KeyCode::Char('c'), true) => {
                return Action::Quit;
            }
            (KeyCode::Char('j'), false) | (KeyCode::Down, _) => self.move_by(1),
            (KeyCode::Char('k'), false) | (KeyCode::Up, _) => self.move_by(-1),
            (KeyCode::Char('d'), _) | (KeyCode::PageDown, _) => self.move_by(self.half_page()),
            (KeyCode::Char('u'), _) | (KeyCode::PageUp, _) => self.move_by(-self.half_page()),
            (KeyCode::Char('g'), false) | (KeyCode::Home, _) => self.move_to(0),
            (KeyCode::Char('G'), _) | (KeyCode::End, _) => self.move_to(usize::MAX),
            (KeyCode::Char('F'), _) => return Action::Fetch,
            (KeyCode::Char('T'), _) => self.show_timestamp = !self.show_timestamp,
            (KeyCode::Char('C'), _) => {
                if let Some(c) = self.selected() {
                    if c.is_worktree() {
                        self.status = "nothing to check out here".into();
                        return Action::None;
                    }
                    return Action::Checkout(checkout_target(c));
                }
            }
            (KeyCode::Char('X'), _) => {
                if let Some(c) = self.selected() {
                    match c.parsed_refs().into_iter().find(|r| r.kind == RefKind::Branch && !r.is_head) {
                        Some(r) => {
                            self.status = format!("delete local branch '{}'? y/n", r.name);
                            self.confirm_delete = Some(r.name);
                        }
                        None => self.status = "no local branch here to delete".into(),
                    }
                }
            }
            (KeyCode::Char('H'), _) => {
                if let Some(c) = self.selected() {
                    if c.is_worktree() {
                        self.status = "not committed yet, so no hash".into();
                        return Action::None;
                    }
                    return Action::CopyHash(c.hash.clone());
                }
            }
            (KeyCode::Char(' '), _) | (KeyCode::Enter, _) => {
                if let Some(c) = self.selected() {
                    if c.is_worktree() {
                        return Action::ShowWorktree;
                    }
                    // The root commit's diff is the whole initial import, so
                    // show what it contains rather than every file as added.
                    return Action::ShowDiff { hash: c.hash.clone(), stat: c.parents.is_empty() };
                }
            }
            _ => {}
        }
        Action::None
    }

    fn half_page(&self) -> isize {
        (self.height / 2).max(1) as isize
    }

    fn move_by(&mut self, delta: isize) {
        let target = (self.cursor as isize + delta).max(0) as usize;
        self.move_to(target);
    }

    fn move_to(&mut self, index: usize) {
        if self.commits.is_empty() {
            self.cursor = 0;
            self.offset = 0;
            return;
        }
        self.cursor = index.min(self.commits.len() - 1);
        self.scroll_to_cursor();
    }

    /// Scroll the window by `delta` rows (mouse wheel); the cursor stays
    /// where it is unless it would leave the window.
    pub fn scroll_view(&mut self, delta: isize) {
        if self.commits.is_empty() {
            return;
        }
        let h = self.height.max(1);
        let max_offset = self.commits.len().saturating_sub(h) as isize;
        self.offset = (self.offset as isize + delta).clamp(0, max_offset) as usize;
        let last_visible = (self.offset + h - 1).min(self.commits.len() - 1);
        self.cursor = self.cursor.clamp(self.offset, last_visible);
    }

    /// Select the commit drawn on screen row `row` (0 = top), if any.
    pub fn select_row(&mut self, row: usize) {
        if row < self.height && self.offset + row < self.commits.len() {
            self.cursor = self.offset + row;
        }
    }

    /// Keep the cursor inside the visible window.
    pub fn scroll_to_cursor(&mut self) {
        let h = self.height.max(1);
        // Fill the screen after the terminal grew.
        self.offset = self.offset.min(self.commits.len().saturating_sub(h));
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + h {
            self.offset = self.cursor + 1 - h;
        }
    }
}

/// What `C` checks out on a commit: a local branch there, else a remote
/// branch by its short name (git creates the tracking branch), else the
/// commit itself, detached.
fn checkout_target(c: &Commit) -> String {
    let refs = c.parsed_refs();
    if let Some(local) = refs.iter().find(|r| r.kind == RefKind::Branch) {
        return local.name.clone();
    }
    if let Some(remote) = refs.iter().find(|r| r.kind == RefKind::Remote) {
        if let Some((_, branch)) = remote.name.split_once('/')
            && branch != "HEAD"
        {
            return branch.to_string();
        }
    }
    c.hash.clone()
}

/// `34s`, `11m`, `2h`, `9d`, `2w`, `3mo`, `1y`: at most four characters.
pub fn compact_age(secs: i64) -> String {
    let s = secs.max(0);
    let (n, unit) = if s < 60 {
        (s, "s")
    } else if s < 3600 {
        (s / 60, "m")
    } else if s < 86_400 {
        (s / 3600, "h")
    } else if s < 14 * 86_400 {
        (s / 86_400, "d")
    } else if s < 60 * 86_400 {
        (s / (7 * 86_400), "w")
    } else if s < 365 * 86_400 {
        (s / (30 * 86_400), "mo")
    } else {
        (s / (365 * 86_400), "y")
    };
    format!("{n}{unit}")
}

#[cfg(test)]
mod tests {
    use super::{App, compact_age};
    use crate::git::{Commit, Worktree};

    fn c(hash: &str, parents: &[&str], refs: &str) -> Commit {
        Commit {
            hash: hash.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: String::new(),
            date: String::new(),
            timestamp: String::new(),
            time: 0,
            refs: refs.into(),
            subject: String::new(),
        }
    }

    fn dirty(head: &str, staged: usize) -> Option<Worktree> {
        Some(Worktree { head: head.into(), staged, modified: 1, untracked: vec![] })
    }

    /// main (HEAD) and a feature branch, feature's tip first as in date order.
    fn two_branches() -> App {
        App::new(vec![
            c("f2", &["f1"], "refs/heads/feature"),
            c("m2", &["m1"], "HEAD -> refs/heads/main"),
            c("f1", &["m1"], ""),
            c("m1", &[], ""),
        ])
    }

    fn hashes(app: &App) -> Vec<&str> {
        app.commits.iter().map(|c| c.hash.as_str()).collect()
    }

    fn colours(app: &App) -> Vec<usize> {
        app.rows.iter().map(|r| r.node_colour()).collect()
    }

    #[test]
    fn worktree_row_sits_on_top_and_joins_the_head_line() {
        let mut app = two_branches();
        let before = colours(&app);
        app.cursor = 1; // on m2
        app.set_worktree(dirty("m2", 2));
        assert_eq!(hashes(&app), ["", "f2", "m2", "f1", "m1"]);
        assert_eq!(app.cursor, 2, "cursor follows the commit it was on");
        assert_eq!(app.rows[0].lane, app.rows[2].lane, "sits in HEAD's lane");
        assert_eq!(app.rows[0].text(), "  ◌");
        assert_eq!(app.rows[0].node_colour(), app.rows[2].node_colour(), "same line as HEAD");
        assert_eq!(colours(&app)[1..], before, "nobody else changes colour");
        let lanes: Vec<usize> = app.rows[1..].iter().map(|r| r.lane).collect();
        assert_eq!(lanes, [0, 1, 0, 0], "nobody else changes lane");
        assert!(app.commits[0].subject.ends_with("2 staged · 1 modified"));
    }

    #[test]
    fn worktree_row_text_updates_without_relayout() {
        let mut app = two_branches();
        app.set_worktree(dirty("m2", 2));
        app.set_worktree(dirty("m2", 5));
        assert_eq!(app.commits.len(), 5);
        assert!(app.commits[0].subject.ends_with("5 staged · 1 modified"));
    }

    #[test]
    fn worktree_row_goes_away_when_clean() {
        let mut app = two_branches();
        let before = colours(&app);
        app.set_worktree(dirty("m2", 1));
        app.cursor = 0; // on the row itself
        app.set_worktree(None);
        assert_eq!(hashes(&app), ["f2", "m2", "f1", "m1"]);
        assert_eq!(app.cursor, 0);
        assert_eq!(colours(&app), before);
        assert!(app.worktree.is_none());
    }

    #[test]
    fn worktree_row_survives_a_reload() {
        let mut app = two_branches();
        app.set_worktree(dirty("m2", 1));
        let before = colours(&app);
        app.cursor = 2;
        app.reset();
        assert_eq!(hashes(&app), [""], "the row is back before any commit");
        for commit in two_branches().commits {
            app.push(commit);
        }
        assert_eq!(hashes(&app), ["", "f2", "m2", "f1", "m1"]);
        assert_eq!(app.cursor, 2);
        assert_eq!(app.rows[0].lane, 1, "remembered HEAD's lane across the reload");
        assert_eq!(colours(&app), before);
    }

    #[test]
    fn dirty_start_adds_the_row_once_loading_is_done() {
        // Fresh start with a dirty tree: HEAD's lane is unknown until the
        // commits are in, so the row waits rather than push every lane right.
        let mut app = App::new(Vec::new());
        app.set_worktree(dirty("m2", 1));
        for commit in two_branches().commits {
            app.push(commit);
        }
        assert_eq!(hashes(&app), ["f2", "m2", "f1", "m1"]);
        let clean = two_branches();
        assert_eq!(colours(&app), colours(&clean));
        // What main does when the stream ends.
        app.relayout();
        assert_eq!(hashes(&app), ["", "f2", "m2", "f1", "m1"]);
        assert_eq!(app.rows[0].lane, 1, "over HEAD's line");
        let lanes: Vec<usize> = app.rows[1..].iter().map(|r| r.lane).collect();
        let clean_lanes: Vec<usize> = clean.rows.iter().map(|r| r.lane).collect();
        assert_eq!(lanes, clean_lanes);
        assert_eq!(colours(&app)[1..], colours(&clean), "nothing changed colour");
    }

    #[test]
    fn worktree_row_keys_do_nothing_harmful() {
        use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        let mut app = two_branches();
        app.set_worktree(dirty("m2", 1));
        app.cursor = 0;
        for ch in ['C', 'H', 'X'] {
            let a = app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
            assert!(matches!(a, super::Action::None), "{ch}");
            assert!(!app.status.is_empty(), "{ch} explains itself");
        }
        let a = app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(matches!(a, super::Action::ShowWorktree));
    }

    #[test]
    fn compact_ages() {
        assert_eq!(compact_age(0), "0s");
        assert_eq!(compact_age(34), "34s");
        assert_eq!(compact_age(11 * 60), "11m");
        assert_eq!(compact_age(2 * 3600 + 59), "2h");
        assert_eq!(compact_age(9 * 86_400), "9d");
        assert_eq!(compact_age(13 * 86_400), "13d");
        assert_eq!(compact_age(20 * 86_400), "2w");
        assert_eq!(compact_age(59 * 86_400), "8w");
        assert_eq!(compact_age(100 * 86_400), "3mo");
        assert_eq!(compact_age(364 * 86_400), "12mo");
        assert_eq!(compact_age(800 * 86_400), "2y");
        assert_eq!(compact_age(-5), "0s");
    }
}
