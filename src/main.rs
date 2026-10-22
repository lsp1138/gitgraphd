mod app;
mod clipboard;
mod git;
mod graph;
mod pager;
mod ui;

use std::io::{Stdout, stdout};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::cursor::{Hide, MoveTo, Show};
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, MouseButton,
    MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode,
};

use crate::git::LogOpts;

/// Commit graph + delta. Nothing else.
#[derive(Parser, Debug)]
#[command(name = "gg", version, about)]
struct Args {
    /// Show every branch (the default; kept for habit)
    #[arg(long, conflicts_with = "head")]
    all: bool,

    /// Show only the history of HEAD, like tig
    #[arg(long)]
    head: bool,

    /// Draw the graph with plain ASCII instead of box-drawing characters
    #[arg(long, conflicts_with = "thin")]
    ascii: bool,

    /// Thin graph lines with rounded corners instead of heavy ones
    #[arg(long)]
    thin: bool,

    /// Stop after this many commits
    #[arg(short = 'n', long = "max-count", value_name = "N")]
    max_count: Option<usize>,

    /// Commit order: date (Git Graph's default), author-date, or topo
    #[arg(long, default_value = "date", value_name = "ORDER")]
    order: String,

    /// Date format for git log: relative, short, iso, human, ...
    #[arg(long, default_value = "relative", value_name = "FORMAT")]
    date: String,

    /// Repository to open (default: current directory)
    path: Option<std::path::PathBuf>,
}

type Term = Terminal<CrosstermBackend<Stdout>>;

fn main() {
    if let Err(e) = gg() {
        eprintln!("gg: {e:#}");
        std::process::exit(1);
    }
}

fn gg() -> Result<()> {
    let args = Args::parse();
    if let Some(p) = &args.path {
        std::env::set_current_dir(p)?;
    }
    let opts = LogOpts {
        all: !args.head,
        max_count: args.max_count,
        date: args.date,
        order: args.order,
    };

    let rx = git::stream(&opts)?;
    let mut app = app::App::new(Vec::new());
    app.remote_branches = git::remote_branches();
    app.style = if args.ascii {
        app::LineStyle::Ascii
    } else if args.thin {
        app::LineStyle::Thin
    } else {
        app::LineStyle::Heavy
    };
    app.dark = prefers_dark();

    // Wait for the first commit before taking over the screen, so a bad
    // repository fails on stderr like any other git command would.
    match rx.recv() {
        Ok(Ok(commit)) => {
            app.push(commit);
            app.loading = true;
        }
        Ok(Err(e)) => return Err(e),
        Err(_) => {}
    }

    // Put the terminal back before the default panic message prints.
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        leave();
        hook(info);
    }));

    let mut terminal = enter()?;
    let result = run(&mut terminal, &mut app, &opts, rx);
    leave();
    result
}

/// Raw mode, alternate screen, mouse reporting, hidden cursor.
fn enter() -> Result<Term> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen, EnableMouseCapture, Hide)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout()))?)
}

fn leave() {
    let _ = execute!(stdout(), DisableMouseCapture, Show, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}

/// Is the terminal background dark? The terminal itself cannot be asked
/// from inside tmux, so follow the desktop: `GG_THEME=light|dark` wins,
/// then GNOME's colour scheme, then macOS's appearance. Dark if unknown.
fn prefers_dark() -> bool {
    match std::env::var("GG_THEME").as_deref() {
        Ok("light") => return false,
        Ok("dark") => return true,
        _ => {}
    }
    let probe = |cmd: &str, args: &[&str]| -> Option<String> {
        Command::new(cmd)
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).to_lowercase())
    };
    if let Some(scheme) = probe("gsettings", &["get", "org.gnome.desktop.interface", "color-scheme"]) {
        return scheme.contains("dark");
    }
    if cfg!(target_os = "macos") {
        // Prints "Dark" in dark mode and fails in light mode.
        return probe("defaults", &["read", "-g", "AppleInterfaceStyle"])
            .is_some_and(|s| s.contains("dark"));
    }
    true
}

/// Pull whatever git has produced so far into the app, without blocking.
fn drain(app: &mut app::App, rx: &Receiver<Result<git::Commit>>) {
    for _ in 0..10_000 {
        match rx.try_recv() {
            Ok(Ok(commit)) => app.push(commit),
            Ok(Err(e)) => app.error = Some(format!("{e:#}")),
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                app.loading = false;
                return;
            }
        }
    }
}

fn run(
    terminal: &mut Term,
    app: &mut app::App,
    opts: &LogOpts,
    mut rx: Receiver<Result<git::Commit>>,
) -> Result<()> {
    let (fetch_tx, fetch_rx) = mpsc::channel::<Result<String>>();
    loop {
        if app.loading {
            drain(app, &rx);
        }
        if let Ok(outcome) = fetch_rx.try_recv() {
            app.fetching = false;
            match outcome {
                Ok(summary) => {
                    app.status = summary;
                    app.remote_branches = git::remote_branches();
                    rx = git::stream(opts)?;
                    app.reset();
                }
                Err(e) => app.status = format!("fetch failed: {e:#}"),
            }
        }
        terminal.draw(|frame| ui::draw(frame, app))?;

        let busy = app.loading || app.fetching;
        let tick = Duration::from_millis(if busy { 30 } else { 250 });
        if !event::poll(tick)? {
            continue;
        }
        let action = match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => app.handle_key(key),
            Event::Mouse(m) => {
                match m.kind {
                    MouseEventKind::ScrollDown => app.scroll_view(3),
                    MouseEventKind::ScrollUp => app.scroll_view(-3),
                    MouseEventKind::Down(MouseButton::Left) => app.select_row(m.row as usize),
                    _ => {}
                }
                app::Action::None
            }
            _ => app::Action::None,
        };

        match action {
            app::Action::None => {}
            app::Action::Quit => return Ok(()),
            app::Action::CopyHash(hash) => {
                app.status = match clipboard::copy(&hash) {
                    Ok(()) => format!("copied {hash}"),
                    Err(e) => format!("copy failed: {e:#}"),
                };
            }
            app::Action::Fetch => {
                if !app.fetching {
                    app.fetching = true;
                    let tx = fetch_tx.clone();
                    thread::spawn(move || {
                        let _ = tx.send(git::fetch());
                    });
                }
            }
            app::Action::ShowDiff { hash, stat } => {
                // Hand the terminal to git's pager without leaving the
                // alternate screen, so the shell never shows through; clear
                // it first so nothing of the list shows under a short diff.
                // less drops back to the main screen on exit; re-enter at once.
                let _ = execute!(
                    stdout(),
                    DisableMouseCapture,
                    Clear(ClearType::All),
                    MoveTo(0, 0),
                    Show
                );
                let _ = disable_raw_mode();
                let outcome = pager::show(&hash, stat);
                let _ = enable_raw_mode();
                let _ = execute!(stdout(), EnterAlternateScreen, EnableMouseCapture, Hide);
                terminal.clear()?;
                if let Err(e) = outcome {
                    app.status = format!("diff failed: {e:#}");
                }
            }
        }
    }
}
