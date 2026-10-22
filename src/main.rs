mod app;
mod git;
mod graph;
mod pager;
mod ui;

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use ratatui::crossterm::event::{self, Event, KeyEventKind};

/// Commit graph + delta. Nothing else.
#[derive(Parser, Debug)]
#[command(name = "gg", version, about)]
struct Args {
    /// Show every branch, not only HEAD history
    #[arg(long)]
    all: bool,

    /// Draw the graph with plain ASCII instead of box-drawing characters
    #[arg(long)]
    ascii: bool,

    /// Stop after this many commits
    #[arg(short = 'n', long = "max-count", value_name = "N")]
    max_count: Option<usize>,

    /// Repository to open (default: current directory)
    path: Option<std::path::PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if let Some(p) = &args.path {
        std::env::set_current_dir(p)?;
    }

    let rx = git::stream(args.all, args.max_count)?;
    let mut app = app::App::new(Vec::new());
    app.ascii = args.ascii;

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

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app, &rx);
    ratatui::restore();
    result
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
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut app::App,
    rx: &Receiver<Result<git::Commit>>,
) -> Result<()> {
    loop {
        if app.loading {
            drain(app, rx);
        }
        terminal.draw(|frame| ui::draw(frame, app))?;

        let tick = Duration::from_millis(if app.loading { 30 } else { 250 });
        if !event::poll(tick)? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        match app.handle_key(key) {
            app::Action::None => {}
            app::Action::Quit => return Ok(()),
            app::Action::ShowDiff(hash) => {
                // Hand the real terminal to git/delta/less, then take it back.
                ratatui::restore();
                let outcome = pager::show(&hash);
                *terminal = ratatui::init();
                terminal.clear()?;
                if let Err(e) = outcome {
                    app.status = format!("diff failed: {e:#}");
                }
            }
        }
    }
}
