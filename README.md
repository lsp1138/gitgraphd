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
cargo install --path .
```

That puts `gg` in `~/.cargo/bin`. It needs `git` on the PATH, and uses whatever
`core.pager` git is configured with, so delta is picked up through your normal
git config.

## Use

Run `gg` inside a repository. `gg --all` shows every branch, like Git Graph.

| key                   | action                               |
|-----------------------|--------------------------------------|
| `j` / `↓`             | next commit                          |
| `k` / `↑`             | previous commit                      |
| `d` / `Ctrl-d`        | half page down                       |
| `u` / `Ctrl-u`        | half page up                         |
| `g` / `Home`          | first commit                         |
| `G` / `End`           | last commit                          |
| `Space` / `Enter`     | show the diff (`git show` via pager) |
| `F`                   | `git fetch --all`, then reload       |
| `q` / `Esc`           | quit                                 |

The mouse wheel or trackpad scrolls the list, and a click selects a row. In
the diff, less gets `--mouse` so it scrolls there too. Because the terminal
is in mouse mode, hold Shift to select text while gg is running.

Inside the diff you are in git's pager, so `q` returns to the list.

The line above the key hints shows the position and the selected commit's
full subject, in case the row cut it off. The full message is part of what
`Space` shows. On the root commit, `Space` shows the message and the list of
files it added instead of the full diff, which would be the entire import.

Flags: `--all`, `-n N`, `--order=date|author-date|topo` (date is the default,
as in Git Graph; topo keeps merged branches' commits together but opens far
more lanes on busy repositories), `--date=<git date format>`, `--ascii` for
terminals without box-drawing glyphs, and an optional repository path.

`GG_PAGER` overrides the pager for gg only, for example `GG_PAGER=less`.

For the pager it launches, gg sets `LESS` to `-R --mouse` (on top of your own
value) and passes `delta.paging=always`, so a diff that fits on one screen
still waits for `q` instead of flashing past.

## Colours

Graph colours follow lines, as in Git Graph: a line is a commit and its
first-parent chain until it ends by merging into another line. A new line
takes the lowest colour no live line is using, from Git Graph's default
twelve-colour palette. A feature branch sitting directly on top of dev is the
same line as dev and shares its colour; one that diverged gets its own.

Pills: green local branch, blue remote, yellow tag, white detached HEAD, grey
anything else. The checked-out branch is bold and underlined. The selection
colour follows GNOME's light/dark setting, the same way the `delta-auto`
wrapper does.
