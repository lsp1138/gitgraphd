# Changelog

## v0.2.0

- Uncommitted changes show as a dotted row above HEAD. Space there pages
  one diff of the whole tree: staged, then not staged, then each untracked
  file as an addition. gg polls `git status` every two seconds so the row
  follows work in other windows; `--no-status` turns that off.
- Colours stay put across a fetch: a new branch or a new commit on top of
  a known branch no longer shifts the colours of lines already on screen.
- Backups `git filter-branch` leaves under `refs/original/`, and git notes,
  are no longer drawn, so rewritten history drops out of the graph.
- Prebuilt binaries for Linux (x86_64, aarch64) and macOS (Intel, Apple
  silicon) on every release, with an `install.sh` that picks the right one.

## v0.1.0

First release: the graph, Space for the diff, checkout, branch delete,
fetch, clipboard, compact ages.
