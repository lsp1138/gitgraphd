mod app;
mod git;
mod graph;
mod pager;
mod ui;

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

    let commits = git::log(args.all, args.max_count)?;
    let mut app = app::App::new(commits);
    app.ascii = args.ascii;

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut app::App) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        if !event::poll(Duration::from_millis(250))? {
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
