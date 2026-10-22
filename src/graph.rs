//! Lane layout for the commit graph, the way VS Code's Git Graph does it.
//!
//! Commits arrive in topological order. Each active lane remembers the hash
//! it is waiting for. A commit lands on the first lane waiting for it; every
//! other lane waiting for it curves in and is freed. Its first parent keeps
//! the lane; extra parents either join a lane already waiting for them or
//! open a new one in the lowest free slot. Lanes never shift sideways, so
//! every line is a straight vertical with corners only at commits.
//!
//! Each row is a list of cells, two per lane: a glyph and a spacer.

use crate::git::Commit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    /// Lane index, used to pick the colour.
    pub lane: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Lane the commit node sits on.
    pub lane: usize,
    pub cells: Vec<Cell>,
}

impl Row {
    #[cfg(test)]
    pub fn text(&self) -> String {
        self.cells.iter().map(|c| c.ch).collect()
    }
}

#[derive(Default)]
pub struct Graph {
    /// Active lanes, each holding the hash it waits for. `None` is a free slot.
    lanes: Vec<Option<String>>,
    /// Widest row produced so far, in cells.
    pub width: usize,
}

impl Graph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, commit: &Commit) -> Row {
        let up: Vec<bool> = self.lanes.iter().map(Option::is_some).collect();
        let waiting: Vec<usize> = self
            .lanes
            .iter()
            .enumerate()
            .filter(|(_, l)| l.as_deref() == Some(commit.hash.as_str()))
            .map(|(i, _)| i)
            .collect();

        let node = match waiting.first() {
            Some(&i) => i,
            None => self.alloc(),
        };

        // Lanes merging into this node end here.
        let mut connected: Vec<usize> = waiting.into_iter().filter(|&j| j != node).collect();
        for &j in &connected {
            self.lanes[j] = None;
        }

        // First parent inherits the lane; extra parents branch out.
        let mut parents = commit.parents.iter();
        let first = parents.next().cloned();
        self.lanes[node] = first.clone();
        let mut seen: Vec<&String> = Vec::new();
        for p in parents {
            if first.as_ref() == Some(p) || seen.contains(&p) {
                continue;
            }
            seen.push(p);
            let j = match self.lanes.iter().position(|l| l.as_ref() == Some(p)) {
                Some(j) => j,
                None => {
                    let j = self.alloc();
                    self.lanes[j] = Some(p.clone());
                    j
                }
            };
            connected.push(j);
        }

        let down: Vec<bool> = self.lanes.iter().map(Option::is_some).collect();
        let ncols = self.lanes.len();
        let up_at = |j: usize| up.get(j).copied().unwrap_or(false);
        let mut cells = vec![Cell { ch: ' ', lane: 0 }; ncols * 2];

        // Horizontal runs, farthest first so the nearer connection wins.
        connected.sort_by_key(|&j| std::cmp::Reverse(j.abs_diff(node)));
        for &j in &connected {
            let (a, b) = if j > node {
                (2 * node + 1, 2 * j - 1)
            } else {
                (2 * j + 1, 2 * node - 1)
            };
            for cell in &mut cells[a..=b] {
                *cell = Cell { ch: '─', lane: j };
            }
        }

        // Lanes passing straight through, crossing any horizontal run.
        for j in 0..ncols {
            if j == node || connected.contains(&j) || !(up_at(j) && down[j]) {
                continue;
            }
            let ch = if cells[2 * j].ch == '─' { '┼' } else { '│' };
            cells[2 * j] = Cell { ch, lane: j };
        }

        // Corners where connected lanes start, end, or are joined.
        for &j in &connected {
            let right = j > node;
            let farther = connected
                .iter()
                .any(|&k| if right { k > j } else { k < j });
            let ch = match (up_at(j), down[j]) {
                (true, false) => match (right, farther) {
                    (true, false) => '╯',
                    (false, false) => '╰',
                    (_, true) => '┴',
                },
                (false, true) => match (right, farther) {
                    (true, false) => '╮',
                    (false, false) => '╭',
                    (_, true) => '┬',
                },
                (true, true) => match (right, farther) {
                    (true, false) => '┤',
                    (false, false) => '├',
                    (_, true) => '┼',
                },
                (false, false) => '─',
            };
            cells[2 * j] = Cell { ch, lane: j };
        }

        let node_ch = if commit.is_head() {
            '◉'
        } else if commit.parents.len() > 1 {
            '○'
        } else {
            '●'
        };
        cells[2 * node] = Cell { ch: node_ch, lane: node };

        while cells.last().is_some_and(|c| c.ch == ' ') {
            cells.pop();
        }
        while self.lanes.last().is_some_and(Option::is_none) {
            self.lanes.pop();
        }
        self.width = self.width.max(cells.len());
        Row { lane: node, cells }
    }

    /// Lowest free lane, growing the list if every lane is busy.
    fn alloc(&mut self) -> usize {
        match self.lanes.iter().position(Option::is_none) {
            Some(j) => j,
            None => {
                self.lanes.push(None);
                self.lanes.len() - 1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(hash: &str, parents: &[&str]) -> Commit {
        Commit {
            hash: hash.into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            author: String::new(),
            date: String::new(),
            refs: String::new(),
            subject: String::new(),
        }
    }

    fn layout(commits: &[Commit]) -> Vec<String> {
        let mut g = Graph::new();
        commits.iter().map(|c| g.push(c).text()).collect()
    }

    #[test]
    fn linear() {
        let rows = layout(&[c("c", &["b"]), c("b", &["a"]), c("a", &[])]);
        assert_eq!(rows, ["●", "●", "●"]);
    }

    #[test]
    fn branch_and_merge() {
        // m merges f into a; both descend from b
        let rows = layout(&[
            c("m", &["a", "f"]),
            c("f", &["b"]),
            c("a", &["b"]),
            c("b", &[]),
        ]);
        assert_eq!(rows, ["○─╮", "│ ●", "● │", "●─╯"]);
    }

    #[test]
    fn merge_joins_existing_lane() {
        // two tips (--all): d→c→x and m merges e with x, c already waits for x
        let rows = layout(&[
            c("d", &["c"]),
            c("c", &["x"]),
            c("m", &["e", "x"]),
            c("e", &[]),
            c("x", &[]),
        ]);
        assert_eq!(rows, ["●", "●", "├─○", "│ ●", "●"]);
    }

    #[test]
    fn octopus_and_crossing() {
        let rows = layout(&[
            c("o", &["a", "b", "c"]),
            c("a", &["x"]),
            c("c", &["x"]),
            c("b", &["y"]),
            c("x", &["y"]),
            c("y", &[]),
        ]);
        assert_eq!(
            rows,
            ["○─┬─╮", "● │ │", "│ │ ●", "│ ● │", "●─┼─╯", "●─╯"]
        );
    }

    #[test]
    fn freed_lane_is_reused_by_next_branch() {
        // a merge whose branch ends right away; the next branch takes lane 1
        let rows = layout(&[
            c("m", &["a", "f"]),
            c("f", &["a"]),
            c("a", &["b", "g"]),
            c("g", &["b"]),
            c("b", &[]),
        ]);
        assert_eq!(rows, ["○─╮", "│ ●", "○─┤", "│ ●", "●─╯"]);
    }

    #[test]
    fn width_tracks_widest_row() {
        let mut g = Graph::new();
        for commit in [c("o", &["a", "b", "c"]), c("a", &[]), c("b", &[]), c("c", &[])] {
            g.push(&commit);
        }
        assert_eq!(g.width, 5);
    }
}
