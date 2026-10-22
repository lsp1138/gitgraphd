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
| `D` / `Ctrl-d`        | half page down                       |
| `U` / `Ctrl-u`        | half page up                         |
| `g` / `Home`          | first commit                         |
| `G` / `End`           | last commit                          |
| `Space` / `Enter`     | show the diff (`git show` via pager) |
| `q` / `Esc`           | quit                                 |

Inside the diff you are in git's pager, so `q` returns to the list.

Flags: `--all`, `-n N`, `--order=date|author-date|topo` (date is the default,
as in Git Graph; topo keeps merged branches' commits together but opens far
more lanes on busy repositories), `--date=<git date format>`, `--ascii` for
terminals without box-drawing glyphs, and an optional repository path.

`GG_PAGER` overrides the pager for gg only, for example `GG_PAGER=less`.

The selection colour follows GNOME's light/dark setting, the same way the
`delta-auto` wrapper does.
