use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, LineStyle};
use crate::git::{Commit, RefKind};
use crate::graph::Row;

const AUTHOR_WIDTH: usize = 12;
/// Widest the graph column may get before rows are cut off on the right.
const MAX_GRAPH_WIDTH: usize = 48;

/// Git Graph's default graph colours, taken by line in the order lines
/// are created; wraps around after the twelfth.
const PALETTE: [Color; 12] = [
    Color::Rgb(0x00, 0x85, 0xd9),
    Color::Rgb(0xd9, 0x00, 0x8f),
    Color::Rgb(0x00, 0xd9, 0x0a),
    Color::Rgb(0xd9, 0x85, 0x00),
    Color::Rgb(0xa3, 0x00, 0xd9),
    Color::Rgb(0xff, 0x00, 0x00),
    Color::Rgb(0x00, 0xd9, 0xcc),
    Color::Rgb(0xe1, 0x38, 0xe8),
    Color::Rgb(0x85, 0xd9, 0x00),
    Color::Rgb(0xdc, 0x5b, 0x23),
    Color::Rgb(0x6f, 0x24, 0xd6),
    Color::Rgb(0xff, 0xcc, 0x00),
];

fn line_color(colour: usize) -> Color {
    PALETTE[colour % PALETTE.len()]
}

/// The layout produces thin rounded glyphs; translate for the other styles.
fn glyph(ch: char, style: LineStyle) -> char {
    match style {
        LineStyle::Thin => ch,
        LineStyle::Heavy => match ch {
            '│' => '┃',
            '─' => '━',
            '╭' => '┏',
            '╮' => '┓',
            '╰' => '┗',
            '╯' => '┛',
            '┼' => '╋',
            '┤' => '┫',
            '├' => '┣',
            '┬' => '┳',
            '┴' => '┻',
            other => other,
        },
        LineStyle::Ascii => match ch {
            '●' => '*',
            '◉' => '@',
            '○' => 'o',
            '│' => '|',
            '─' => '-',
            '╮' | '╰' => '\\',
            '╭' | '╯' => '/',
            '┼' | '┤' | '├' | '┬' | '┴' => '+',
            other => other,
        },
    }
}

/// Spans for one graph row, padded to `width` cells.
fn graph_spans(row: &Row, width: usize, style: LineStyle) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(row.cells.len() + 1);
    let mut run = String::new();
    let mut run_colour = usize::MAX;
    for cell in row.cells.iter().take(width) {
        if cell.colour != run_colour && !run.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut run), Style::new().fg(line_color(run_colour))));
        }
        run_colour = cell.colour;
        run.push(glyph(cell.ch, style));
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, Style::new().fg(line_color(run_colour))));
    }
    let used = row.cells.len().min(width);
    if used < width {
        spans.push(Span::raw(" ".repeat(width - used)));
    }
    spans
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [list_area, status_area, hints_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1), Constraint::Length(1)])
            .areas(frame.area());

    app.height = list_area.height as usize;
    app.scroll_to_cursor();
    let graph_width = app.graph.width.clamp(1, MAX_GRAPH_WIDTH);

    for (i, row) in (list_area.y..list_area.bottom()).enumerate() {
        let index = app.offset + i;
        let Some(commit) = app.commits.get(index) else {
            break;
        };
        let area = Rect::new(list_area.x, row, list_area.width, 1);
        let selected = index == app.cursor;
        if selected {
            let bg = if app.dark { Color::Indexed(237) } else { Color::Indexed(254) };
            frame.buffer_mut().set_style(area, Style::new().bg(bg));
        }

        let mut spans = graph_spans(&app.rows[index], graph_width, app.style);
        spans.push(Span::raw("  "));
        spans.extend([
            Span::styled(commit.short_hash().to_string(), Style::new().fg(Color::Yellow)),
            Span::raw("  "),
            Span::styled(pad_left(&commit.date, app.date_width), Style::new().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(pad_right(&commit.author, AUTHOR_WIDTH), Style::new().fg(Color::Cyan)),
            Span::raw("  "),
        ]);
        spans.extend(ref_pills(commit));
        spans.push(Span::styled(
                commit.subject.clone(),
                if selected {
                    Style::new().add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                },
            ));
        frame.render_widget(Line::from(spans), area);
    }

    let (status, style) = if let Some(e) = &app.error {
        (format!("git: {e}"), Style::new().fg(Color::White).bg(Color::Red))
    } else if !app.status.is_empty() {
        (app.status.clone(), Style::new().add_modifier(Modifier::REVERSED))
    } else {
        // Position plus the selected commit's full subject, since the row
        // itself may have cut it off.
        let subject = app.selected().map(|c| c.subject.as_str()).unwrap_or("");
        (
            format!(
                "[{}/{}{}]  {}{subject}",
                (app.cursor + 1).min(app.commits.len()),
                app.commits.len(),
                if app.loading { "+" } else { "" },
                if app.fetching { "fetching...  " } else { "" },
            ),
            Style::new().add_modifier(Modifier::REVERSED),
        )
    };
    frame.render_widget(Line::styled(status, style), status_area);
    frame.render_widget(
        Line::styled(
            " j/k move  d/u page  g/G ends  Space diff  F fetch  q quit",
            Style::new().fg(Color::DarkGray),
        ),
        hints_area,
    );
}

/// Coloured pills for the refs on a commit, Git Graph style, each followed
/// by a space. Empty when the commit has no refs.
fn ref_pills(commit: &Commit) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for r in commit.parsed_refs() {
        let (fg, bg) = match r.kind {
            RefKind::Head => (Color::Black, Color::White),
            RefKind::Branch => (Color::Black, Color::Green),
            RefKind::Remote => (Color::White, Color::Blue),
            RefKind::Tag => (Color::Black, Color::Yellow),
            RefKind::Other => (Color::Black, Color::Gray),
        };
        let mut style = Style::new().fg(fg).bg(bg);
        if r.is_head {
            style = style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
        }
        spans.push(Span::styled(format!(" {} ", r.name), style));
        spans.push(Span::raw(" "));
    }
    spans
}

/// Truncate to `width` columns and pad on the right.
fn pad_right(s: &str, width: usize) -> String {
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = UnicodeWidthStr::width(ch.encode_utf8(&mut [0; 4]) as &str);
        if w + cw > width {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.extend(std::iter::repeat_n(' ', width - w));
    out
}

/// Pad on the left; truncating from the left if too long.
fn pad_left(s: &str, width: usize) -> String {
    let w = UnicodeWidthStr::width(s);
    if w >= width {
        return s.to_string();
    }
    let mut out: String = std::iter::repeat_n(' ', width - w).collect();
    out.push_str(s);
    out
}
