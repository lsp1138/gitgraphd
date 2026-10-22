# gitgraphd

`gg` is the smallest useful slice of tig: a commit graph drawn like VS Code's
Git Graph extension, tig-style navigation, and `Space` to open the selected
commit in [delta](https://github.com/dandavison/delta). Nothing else.

```
◉    58a3dcc    3 months ago  Gregory Pako   master   origin/master   made fpp robust
●    c509f37    3 months ago  Gregory Pako  quote #{socket_path} when invoking TMUX
│ ●  190b0e5    6 years ago   Lars           feat99750  work on feat99750
○─╯  34fa663    6 years ago   Lars          merge feat99750
```

## Install

```
cargo install gitgraphd
```

or from a checkout, `cargo install --path .`. Either way the binary is `gg`
in `~/.cargo/bin`. It needs `git` on the PATH and uses whatever `core.pager`
git is configured with, so if you already use delta for `git diff`, `gg`
uses it too. Any pager works; delta is recommended because it looks best:

```
cargo install git-delta
git config --global core.pager delta
```

Without delta, diffs show in git's own colours through less, and gg says so
once in the status line.

## Status

Version 0.1. Built and used daily on Ubuntu 26.04 inside tmux with
Alacritty. It only talks to `git` and the terminal, so it should work on any
Linux or macOS with git installed, but those are untested; reports welcome.
Needs Rust 1.88 or newer to build.

## Why

tig does a lot; Git Graph in VS Code draws the nicest commit tree; delta
renders the best diffs. `gg` is just those two pictures glued together with
tig's keys, in one small binary that shells out to git and never touches
your repository.

## Use

Run `gg` inside a repository. It shows every branch, like Git Graph;
`gg --head` shows only the checked-out history, like tig.

| key                   | action                               |
|-----------------------|--------------------------------------|
| `j` / `↓`             | next commit                          |
| `k` / `↑`             | previous commit                      |
| `d` / `Ctrl-d`        | half page down                       |
| `u` / `Ctrl-u`        | half page up                         |
| `g` / `Home`          | first commit                         |
| `G` / `End`           | last commit                          |
| `Space` / `Enter`     | show the diff (`git show` via pager) |
| `C`                   | check out the branch here, or commit |
| `T`                   | toggle relative date / timestamp     |
| `H`                   | copy the full hash to the clipboard  |
| `F`                   | `git fetch --all --prune`, reload    |
| `q` / `Esc`           | quit                                 |

`C` checks out the local branch on the selected commit; with only a remote
branch there it creates the tracking branch the way `git checkout name`
does; with no branch at all it checks out the commit detached. The graph
reloads so HEAD moves. git's own message, or error, shows in the status line.

`H` uses wl-copy, xclip, xsel or pbcopy when one is installed, and otherwise
the OSC 52 escape, which most terminals and tmux pass on to the clipboard.

The mouse wheel or trackpad scrolls the list, and a click selects a row. In
the diff, less gets `--mouse` so it scrolls there too. Because the terminal
is in mouse mode, hold Shift to select text while gg is running.

Inside the diff you are in git's pager, so `q` returns to the list.

Dates are committer dates, which is what the default date order sorts by.
The line above the key hints shows the position and the selected commit's
full subject, in case the row cut it off. The full message is part of what
`Space` shows. On the root commit, `Space` shows the message and the list of
files it added instead of the full diff, which would be the entire import.

Flags: `--head`, `-n N`, `--order=date|author-date|topo` (date is the default,
as in Git Graph; topo keeps merged branches' commits together but opens far
more lanes on busy repositories), `--date=<git date format>`, `--ascii` for
terminals without box-drawing glyphs, `--thin` for thin rounded lines instead
of the heavy square ones, and an optional repository path.

`GG_PAGER` overrides the pager for gg only, for example `GG_PAGER=less`.

For the pager it launches, gg sets `LESS` to `-R --mouse` (on top of your own
value) and passes `delta.paging=always`, so a diff that fits on one screen
still waits for `q` instead of flashing past.

## Colours

Graph colours follow lines, as in Git Graph: a line is a commit and its
first-parent chain until it ends by merging into another line. A feature
branch sitting directly on top of dev is the same line as dev and shares its
colour; one that diverged gets its own. A new line takes the lowest colour
no live line is using, from Git Graph's default twelve colours, and a reload
after `F` keeps the colours lines had before, so main and dev do not swap.

Ref pills take the colour of the line their commit is on, as in Git Graph,
and follow its label rules: a local branch and its remote on the same commit
are one pill (`dev │ origin`); a local branch whose remote is on another
commit is a plain filled pill; a remote with no local counterpart is a filled
`origin/feature` pill; a branch that exists on no remote at all, and the
stash, are drawn as a frame, `[ wip ]`, so purely local things stand out.
The checked-out branch comes first, bold and underlined. Tags are prefixed
with `tag:`. The selection
colour follows the desktop light/dark setting on GNOME and macOS; set
`GG_THEME=light` or `GG_THEME=dark` to override, or anywhere else.
