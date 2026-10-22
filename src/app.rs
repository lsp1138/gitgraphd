use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::git::Commit;

pub enum Action {
    None,
    Quit,
    ShowDiff(String),
}

pub struct App {
    pub commits: Vec<Commit>,
    /// Index of the selected commit.
    pub cursor: usize,
    /// Index of the first visible row.
    pub offset: usize,
    /// Rows available for the list; updated by the UI on every draw.
    pub height: usize,
    pub status: String,
}

impl App {
    pub fn new(commits: Vec<Commit>) -> Self {
        Self {
            commits,
            cursor: 0,
            offset: 0,
            height: 1,
            status: String::new(),
        }
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
            (KeyCode::Char('d'), true) | (KeyCode::PageDown, _) => {
                self.move_by(self.half_page())
            }
            (KeyCode::Char('u'), true) | (KeyCode::PageUp, _) => {
                self.move_by(-self.half_page())
            }
            (KeyCode::Char('g'), false) | (KeyCode::Home, _) => self.move_to(0),
            (KeyCode::Char('G'), _) | (KeyCode::End, _) => self.move_to(usize::MAX),
            (KeyCode::Char('D'), _) | (KeyCode::Enter, _) => {
                if let Some(c) = self.selected() {
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
        if self.cursor < self.offset {
            self.offset = self.cursor;
        } else if self.cursor >= self.offset + h {
            self.offset = self.cursor + 1 - h;
        }
    }
}
