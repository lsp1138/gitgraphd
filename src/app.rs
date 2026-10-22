use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use unicode_width::UnicodeWidthStr;

use crate::git::Commit;
use crate::graph::{Graph, Row};

pub enum Action {
    None,
    Quit,
    ShowDiff(String),
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
    /// Widest date string loaded so far; the date column is this wide.
    pub date_width: usize,
    /// Terminal background is dark; picks the selection colour.
    pub dark: bool,
    /// Sticky error from the git log stream, shown until quit.
    pub error: Option<String>,
    /// True while git log is still producing commits.
    pub loading: bool,
    /// Draw the graph with `* | / \` instead of box-drawing glyphs.
    pub ascii: bool,
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
            dark: true,
            error: None,
            loading: false,
            ascii: false,
        };
        for c in commits {
            app.push(c);
        }
        app
    }

    /// Append one commit (they must arrive in topological order).
    pub fn push(&mut self, commit: Commit) {
        self.date_width = self.date_width.max(UnicodeWidthStr::width(commit.date.as_str()));
        self.rows.push(self.graph.push(&commit));
        self.commits.push(commit);
    }

    pub fn selected(&self) -> Option<&Commit> {
        self.commits.get(self.cursor)
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        self.status.clear();
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
            (KeyCode::Char(' '), _) | (KeyCode::Enter, _) => {
                if let Some(c) = self.selected() {
                    if c.parents.is_empty() {
                        // The root commit's diff is the whole initial import;
                        // delta rendering all of it is slow and not useful.
                        self.status = "initial commit: no diff to show".into();
                        return Action::None;
                    }
                    return Action::ShowDiff(c.hash.clone());
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
