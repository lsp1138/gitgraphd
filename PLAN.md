# gitgraphd — minimal tig: commit graph + delta

## Goal

A tiny terminal tool with exactly two features:

1. A commit graph that looks as close as possible to VS Code's *Git Graph* extension:
   coloured lanes, round corners, one commit per row, with hash / date / author /
   refs / subject columns.
2. tig-style line-by-line navigation over the commits, and `D` on a commit opens its
   diff in **delta** (your existing `delta-auto` wrapper, so light/dark is respected).

Nothing else from tig: no blame, no status, no stage/unstage, no refs view, no
search, no config file in v1.

## What kind of binary

- A single statically-linked-ish Rust binary named **`gg`**, run from inside any repo:
  `gg` (HEAD history) or `gg --all` (every branch, like Git Graph's default).
  The Cargo package is `gitgraphd`; `Cargo.toml` sets `[[bin]] name = "gg"` so the
  installed command is the short one.
- A full-screen TUI on the alternate screen, like tig. No daemon, no server, no
  state on disk.
- It shells out to `git` rather than linking libgit2/gitoxide:
  - history comes from `git log` with a machine-readable format,
  - the diff comes from `git show <hash>` and git itself invokes `core.pager`,
    which is already `delta-auto` in your gitconfig. gg never needs to know how
    delta is configured.
  - No C dependencies, trivial cross-compile, and it always agrees with what git
    says.
- Installed with `cargo install --path .` to `~/.cargo/bin/gg`. The binary is
  not a dotfile; only the source repo lives under `~/dev/own/gitgraphd`.

## Dependencies

| crate      | why                                                        |
|------------|------------------------------------------------------------|
| ratatui    | TUI widgets/layout, the de-facto standard                  |
| crossterm  | terminal backend, raw mode, alternate screen, key events   |
| clap       | `--all`, `-n`, optional path arg                           |
| anyhow     | error plumbing                                             |

No libgit2, no async runtime. One background thread reads `git log` output.

Prerequisite: Rust toolchain is **not installed** on this machine yet:
`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` then restart the shell.

## Architecture

```
src/
  main.rs    clap args, set up terminal, run the event loop
  git.rs     spawn `git log`, parse one commit per line into a Commit struct
  graph.rs   lane layout: turns the parent lists into per-row drawing cells
  ui.rs      ratatui rendering of the graph rows + columns + status line
  app.rs     state: commit list, cursor, scroll offset, key handling
  pager.rs   suspend the TUI, run `git show <hash>`, resume
```

### git.rs: getting history

```
git log --topo-order --date=relative \
        --format='%H%x00%P%x00%an%x00%ad%x00%D%x00%s' [--all] [-n N]
```

- `%x00` (NUL) as a field separator so subjects with any punctuation parse safely.
- `--topo-order` is what makes lane layout stable and is what Git Graph uses.
- `%D` gives decorations (`HEAD -> main, origin/main, tag: v1.2`) for the ref pills.
- Streamed: a thread reads stdout line by line and pushes into a shared `Vec`.
  The UI shows what has arrived so far, so a 100k-commit repo opens instantly.

### graph.rs: lane layout (the Git Graph look)

Same algorithm Git Graph uses, walked top-down over the topo-ordered list:

- Keep an ordered list of **active lanes**; each lane holds the hash it is waiting for.
- For each commit:
  1. Its column is the first lane waiting for it. If none, append a new lane.
  2. Every other lane that was also waiting for it **merges into** this column
     (draw a curve from that lane into the node, then free the lane).
  3. Its first parent takes over this lane. Each extra parent (merge commit) either
     joins an existing lane already waiting for that parent (curve out) or opens a
     new lane to the right.
  4. Lanes with no commit to wait for are removed and everything right of them
     shifts left, so the graph stays compact like Git Graph rather than ever-widening.
- Output per row: node column, a vertical-line bitmap, and a list of
  (from_col, to_col) connectors for the half-row below.
- Lane colour = lane index modulo a palette of 8, matching Git Graph's cycling.

Rendered with box-drawing characters and truecolor (your terminal advertises
`COLORTERM=truecolor` inside tmux):

```
●  a1b2c3d  2 hours ago   Lars   [main] [origin/main]  Fix the thing
│ ╭─●  9f8e7d6  yesterday     Lars   [feature/x]           Add x
│ │ ●  ...
●─╯    ...                                                  Merge feature/x
```

Node: `●` for ordinary commits, `◉` for the HEAD commit, `○` for merges.
Fallback `--ascii` flag draws `*|/\` for terminals/fonts that lack the glyphs.

### ui.rs: columns

Fixed order, widths computed once per frame:

`graph | short hash (7) | relative date (right-aligned, 14) | author (truncated 12) | refs | subject (fills rest)`

Refs are drawn as coloured pills like Git Graph: local branch green, remote blue,
tag yellow, HEAD bold/inverse. Selected row is highlighted with a background
colour across the whole line, tig-style.

### app.rs: keys (tig-compatible subset)

| key                    | action                           |
|------------------------|----------------------------------|
| `j` / `↓`              | next commit                      |
| `k` / `↑`              | previous commit                  |
| `D` / `Ctrl-d`         | half page down                   |
| `U` / `Ctrl-u`         | half page up                     |
| `g` / `Home`           | first commit                     |
| `G` / `End`            | last loaded commit               |
| `Space` / `Enter`      | show diff of selected commit in delta |
| `q` / `Esc`            | quit                             |

That is the whole key map. No `:` command line, no `?` help view in v1.

### pager.rs: showing the diff in delta

On `D`:

1. Leave the alternate screen, disable raw mode, show the cursor.
2. Run `git show <hash>` with stdin/stdout/stderr inherited from the terminal.
   stdout is a tty, so git pipes through `core.pager` = `delta-auto`, which picks
   `--light`/`--dark` from GNOME and runs delta, which pages with `less`.
   `q` in less returns to gg.
   - Force `GIT_PAGER` unset and pass `-c core.pager=...` only if the user has
     explicitly overridden it via `GG_PAGER`; otherwise trust git.
   - Merge commits: `git show` prints the combined diff by default. Add
     `-m --first-parent` later if that proves more useful; not in v1.
3. Re-enable raw mode, re-enter the alternate screen, force a full redraw.

This is exactly how tig runs external commands, so behaviour will feel familiar.

## Milestones

- **M0** install rustup, `cargo init`, ratatui hello-world on the alternate screen,
  `q` quits cleanly and restores the terminal (also on panic, via a panic hook).
- **M1** list commits from `git log` (no graph column), j/k navigation,
  `D` opens delta and returns. Already usable day to day.
- **M2** lane layout + box-drawing graph column with colours. `--all` flag.
- **M3** ref pills, HEAD marker, column widths/truncation, `--ascii` fallback.
- **M4** streaming load in a background thread, so huge repos open instantly;
  `G` keeps working as more commits arrive.
- **M5** polish: resize handling, `GG_PAGER`, `-n` limit, `--date=` passthrough.

M1 is a few hundred lines; M2 is the only genuinely tricky part (~300 lines).

## Decisions taken

- HEAD-only by default, `--all` for every branch.
- Both `D` and `Enter` open the diff.
- Relative dates by default, `--date=<format>` passes any git date format through.
- `--date-order` by default like Git Graph, not `--topo-order`: on a repository
  with many merged branches topo order emits each merged branch long after its
  merge, so lanes stay open for pages and the graph hits the width cap.
  `--order=topo` is still available.
- Keys revised after first use: `D`/`U` page, `Space` opens the diff.
