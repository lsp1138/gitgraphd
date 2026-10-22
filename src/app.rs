use std::collections::HashSet;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;

use crate::git::{Commit, RefKind};
use crate::graph::{Graph, Row};

pub enum Action {
    None,
    Quit,
    /// Run `git fetch` in the background and reload when it is done.
    Fetch,
    /// Show the diff of this hash; `stat` asks for the file summary only.
    ShowDiff { hash: String, stat: bool },
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
    /// Show the absolute timestamp instead of git's relative date.
    pub show_timestamp: bool,
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
            dark: true,
            error: None,
            loading: false,
            fetching: false,
            pending_cursor: None,
            remote_branches: HashSet::new(),
            confirm_delete: None,
            style: LineStyle::Heavy,
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
        self.pending_cursor = self.selected().map(|c| c.hash.clone());
        self.graph = Graph::with_memory(Graph::memory(&self.commits, &self.rows));
        self.commits.clear();
        self.rows.clear();
        self.cursor = 0;
        self.offset = 0;
        self.date_width = 0;
        self.timestamp_width = 0;
        self.error = None;
        self.loading = true;
    }

    pub fn selected(&self) -> Option<&Commit> {
        self.commits.get(self.cursor)
    }

    /// The date column's text for a commit, and the column's width.
    pub fn date_column<'a>(&'a self, c: &'a Commit) -> (&'a str, usize) {
        if self.show_timestamp {
            (&c.timestamp, self.timestamp_width)
        } else {
            (&c.date, self.date_width)
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
                    return Action::CopyHash(c.hash.clone());
                }
            }
            (KeyCode::Char(' '), _) | (KeyCode::Enter, _) => {
                if let Some(c) = self.selected() {
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
