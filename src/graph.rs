//! Lane layout for the commit graph, the way VS Code's Git Graph does it.
//!
//! Commits arrive in topological (or date) order. Each active lane remembers
//! the hash it is waiting for. A commit lands on the first lane waiting for
//! it; every other lane waiting for it curves in and is freed. Its first
//! parent keeps the lane; extra parents either join a lane already waiting
//! for them or open a new one in the lowest free slot. Lanes never shift
//! sideways, so every line is a straight vertical with corners at commits.
//!
//! Colour belongs to a line, not a column. Git Graph hands a new line the
//! lowest colour not in use, which on a busy repository gives one short
//! branch after another the same colour; instead the palette is cycled, so
//! a colour comes back only after every other one has been used, and never
//! on the row where it was released.
//!
//! Each row is a list of cells, two per lane: a glyph and a spacer.

use crate::git::Commit;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    /// Index into the palette (unbounded; render with modulo).
    pub colour: usize,
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

    /// Colour of the commit node itself, i.e. of the line it belongs to.
    pub fn node_colour(&self) -> usize {
        self.cells[2 * self.lane].colour
    }
}

#[derive(Debug)]
struct Lane {
    target: String,
    colour: usize,
}

/// Size of the palette the UI draws with; colours cycle through it.
pub const PALETTE_SIZE: usize = 12;

#[derive(Default)]
pub struct Graph {
    /// Active lanes, each holding the hash it waits for. `None` is a free slot.
    lanes: Vec<Option<Lane>>,
    /// Which colour indices a live line is using.
    colours_in_use: Vec<bool>,
    /// Where the next colour search starts, so the palette is cycled.
    next_colour: usize,
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
            .filter(|(_, l)| l.as_ref().is_some_and(|l| l.target == commit.hash))
            .map(|(i, _)| i)
            .collect();

        let node = match waiting.first() {
            Some(&i) => i,
            None => self.alloc_lane(),
        };

        // Lanes merging into this node end here. Their colours stay reserved
        // until the row is done, so nothing starting here can take them.
        let mut released: Vec<usize> = Vec::new();
        let mut ending_colour: Vec<(usize, usize)> = Vec::new();
        let mut connected: Vec<usize> = waiting.into_iter().filter(|&j| j != node).collect();
        for &j in &connected {
            let lane = self.lanes[j].take().expect("waiting lane is active");
            ending_colour.push((j, lane.colour));
            released.push(lane.colour);
        }

        // The commit's own line: continue the lane's colour or start a line.
        let node_colour = match &self.lanes[node] {
            Some(l) => l.colour,
            None => self.alloc_colour(),
        };

        // First parent inherits the lane; extra parents branch out.
        let mut parents = commit.parents.iter();
        let first = parents.next().cloned();
        self.lanes[node] = first.clone().map(|target| Lane { target, colour: node_colour });
        if self.lanes[node].is_none() {
            released.push(node_colour);
        }
        let mut seen: Vec<&String> = Vec::new();
        for p in parents {
            if first.as_ref() == Some(p) || seen.contains(&p) {
                continue;
            }
            seen.push(p);
            let existing = self
                .lanes
                .iter()
                .position(|l| l.as_ref().is_some_and(|l| &l.target == p));
            let j = match existing {
                Some(j) => j,
                None => {
                    let j = self.alloc_lane();
                    let colour = self.alloc_colour();
                    self.lanes[j] = Some(Lane { target: p.clone(), colour });
                    j
                }
            };
            connected.push(j);
        }

        let down: Vec<bool> = self.lanes.iter().map(Option::is_some).collect();
        let ncols = self.lanes.len();
        let up_at = |j: usize| up.get(j).copied().unwrap_or(false);
        // Colour of whatever line occupies column j on this row: the live
        // lane if there is one, else the line that ended here.
        let colour_at = |j: usize| -> usize {
            if let Some(l) = &self.lanes[j] {
                return l.colour;
            }
            ending_colour
                .iter()
                .find(|(k, _)| *k == j)
                .map(|(_, c)| *c)
                .unwrap_or(0)
        };
        let mut cells = vec![Cell { ch: ' ', colour: 0 }; ncols * 2];

        // Horizontal runs, farthest first so the nearer connection wins.
        connected.sort_by_key(|&j| std::cmp::Reverse(j.abs_diff(node)));
        for &j in &connected {
            let (a, b) = if j > node {
                (2 * node + 1, 2 * j - 1)
            } else {
                (2 * j + 1, 2 * node - 1)
            };
            let colour = colour_at(j);
            for cell in &mut cells[a..=b] {
                *cell = Cell { ch: '─', colour };
            }
        }

        // Lanes passing straight through, crossing any horizontal run.
        for j in 0..ncols {
            if j == node || connected.contains(&j) || !(up_at(j) && down[j]) {
                continue;
            }
            let ch = if cells[2 * j].ch == '─' { '┼' } else { '│' };
            cells[2 * j] = Cell { ch, colour: colour_at(j) };
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
            cells[2 * j] = Cell { ch, colour: colour_at(j) };
        }

        let node_ch = if commit.is_head() {
            '◉'
        } else if commit.parents.len() > 1 {
            '○'
        } else {
            '●'
        };
        cells[2 * node] = Cell { ch: node_ch, colour: node_colour };

        while cells.last().is_some_and(|c| c.ch == ' ') {
            cells.pop();
        }
        while self.lanes.last().is_some_and(Option::is_none) {
            self.lanes.pop();
        }
        for c in released {
            self.colours_in_use[c] = false;
        }
        self.width = self.width.max(cells.len());
        Row { lane: node, cells }
    }

    /// Lowest free lane, growing the list if every lane is busy.
    fn alloc_lane(&mut self) -> usize {
        match self.lanes.iter().position(Option::is_none) {
            Some(j) => j,
            None => {
                self.lanes.push(None);
                self.lanes.len() - 1
            }
        }
    }

    /// Next colour in the cycle that no live line is using; a new index
    /// beyond the palette only when every colour is in use at once.
    fn alloc_colour(&mut self) -> usize {
        let n = self.colours_in_use.len().max(PALETTE_SIZE);
        self.colours_in_use.resize(n, false);
        for k in 0..n {
            let c = (self.next_colour + k) % n;
            if !self.colours_in_use[c] {
                self.colours_in_use[c] = true;
                self.next_colour = (c + 1) % n;
                return c;
            }
        }
        self.colours_in_use.push(true);
        self.colours_in_use.len() - 1
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
            timestamp: String::new(),
            refs: String::new(),
            subject: String::new(),
        }
    }

    fn rows(commits: &[Commit]) -> Vec<Row> {
        let mut g = Graph::new();
        commits.iter().map(|c| g.push(c)).collect()
    }

    fn layout(commits: &[Commit]) -> Vec<String> {
        rows(commits).iter().map(Row::text).collect()
    }

    fn node_colours(commits: &[Commit]) -> Vec<usize> {
        rows(commits).iter().map(Row::node_colour).collect()
    }

    #[test]
    fn linear() {
        let h = [c("c", &["b"]), c("b", &["a"]), c("a", &[])];
        assert_eq!(layout(&h), ["●", "●", "●"]);
        assert_eq!(node_colours(&h), [0, 0, 0]);
    }

    #[test]
    fn branch_and_merge() {
        // m merges f into a; both descend from b
        let h = [
            c("m", &["a", "f"]),
            c("f", &["b"]),
            c("a", &["b"]),
            c("b", &[]),
        ];
        assert_eq!(layout(&h), ["○─╮", "│ ●", "● │", "●─╯"]);
        assert_eq!(node_colours(&h), [0, 1, 0, 0]);
        // the curve into b carries the branch's colour
        let last = &rows(&h)[3];
        assert_eq!(last.cells[1].colour, 1);
        assert_eq!(last.cells[2].colour, 1);
    }

    #[test]
    fn merge_joins_existing_lane() {
        // two tips (--all): d→c→x and m merges e with x, c already waits for x
        let h = [
            c("d", &["c"]),
            c("c", &["x"]),
            c("m", &["e", "x"]),
            c("e", &[]),
            c("x", &[]),
        ];
        assert_eq!(layout(&h), ["●", "●", "├─○", "│ ●", "●"]);
        assert_eq!(node_colours(&h), [0, 0, 1, 1, 0]);
    }

    #[test]
    fn octopus_and_crossing() {
        let h = [
            c("o", &["a", "b", "c"]),
            c("a", &["x"]),
            c("c", &["x"]),
            c("b", &["y"]),
            c("x", &["y"]),
            c("y", &[]),
        ];
        assert_eq!(
            layout(&h),
            ["○─┬─╮", "● │ │", "│ │ ●", "│ ● │", "●─┼─╯", "●─╯"]
        );
        assert_eq!(node_colours(&h), [0, 0, 2, 1, 0, 0]);
    }

    #[test]
    fn colour_released_on_a_row_is_not_reused_on_that_row() {
        // f's line ends at a, where a new line to g starts in the same slot:
        // same lane, but it must get a fresh colour.
        let h = [
            c("m", &["a", "f"]),
            c("f", &["a"]),
            c("a", &["b", "g"]),
            c("g", &["b"]),
            c("b", &[]),
        ];
        assert_eq!(layout(&h), ["○─╮", "│ ●", "○─┤", "│ ●", "●─╯"]);
        assert_eq!(node_colours(&h), [0, 1, 0, 2, 0]);
    }

    #[test]
    fn later_line_takes_the_next_colour_not_the_freed_one() {
        // one branch ends at b, a fresh one starts a row later: the palette
        // cycles on, so it gets colour 2 even though 1 is free again
        let h = [
            c("m", &["b", "f"]),
            c("f", &["b"]),
            c("b", &["n"]),
            c("n", &["a", "g"]),
            c("g", &["a"]),
            c("a", &[]),
        ];
        assert_eq!(layout(&h), ["○─╮", "│ ●", "●─╯", "○─╮", "│ ●", "●─╯"]);
        assert_eq!(node_colours(&h), [0, 1, 0, 0, 2, 0]);
    }

    #[test]
    fn colours_wrap_after_the_whole_palette() {
        // thirteen short branches in a row, each merged by the next merge
        // commit: colours 1..11 are handed out, then the cycle wraps to 1
        let mut h = Vec::new();
        for i in 1..=13 {
            let next = if i == 13 { "root".to_string() } else { format!("m{}", i + 1) };
            let (m, f) = (format!("m{i}"), format!("f{i}"));
            h.push(c(&m, &[&next, &f]));
            h.push(c(&f, &[&next]));
        }
        h.push(c("root", &[]));
        let cols = node_colours(&h);
        let branch_cols: Vec<usize> = cols.iter().skip(1).step_by(2).copied().collect();
        assert_eq!(branch_cols, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 1, 2]);
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
