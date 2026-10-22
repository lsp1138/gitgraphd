use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::graph::Row;

const AUTHOR_WIDTH: usize = 12;
const DATE_WIDTH: usize = 14;
/// Widest the graph column may get before rows are cut off on the right.
const MAX_GRAPH_WIDTH: usize = 48;

/// Git Graph's default lane colours, cycled by lane index.
const PALETTE: [Color; 8] = [
    Color::Rgb(0x00, 0x85, 0xd9),
    Color::Rgb(0xd9, 0x00, 0x8f),
    Color::Rgb(0x00, 0xd9, 0x0a),
    Color::Rgb(0xd9, 0x85, 0x00),
    Color::Rgb(0xa3, 0x00, 0xd9),
    Color::Rgb(0xff, 0x00, 0x00),
    Color::Rgb(0x00, 0xd9, 0xd9),
    Color::Rgb(0xd9, 0xd9, 0x00),
];

fn lane_color(lane: usize) -> Color {
    PALETTE[lane % PALETTE.len()]
}

/// Spans for one graph row, padded to `width` cells.
fn graph_spans(row: &Row, width: usize) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(row.cells.len() + 1);
    let mut run = String::new();
    let mut run_lane = usize::MAX;
    for cell in row.cells.iter().take(width) {
        if cell.lane != run_lane && !run.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut run), Style::new().fg(lane_color(run_lane))));
        }
        run_lane = cell.lane;
        run.push(cell.ch);
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, Style::new().fg(lane_color(run_lane))));
    }
    let used = row.cells.len().min(width);
    if used < width {
        spans.push(Span::raw(" ".repeat(width - used)));
    }
    spans
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let [list_area, status_area] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());

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
            frame
                .buffer_mut()
                .set_style(area, Style::new().bg(Color::Indexed(237)));
        }

        let mut spans = graph_spans(&app.rows[index], graph_width);
        spans.push(Span::raw("  "));
        spans.extend([
            Span::styled(commit.short_hash().to_string(), Style::new().fg(Color::Yellow)),
            Span::raw("  "),
            Span::styled(pad_left(&commit.date, DATE_WIDTH), Style::new().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled(pad_right(&commit.author, AUTHOR_WIDTH), Style::new().fg(Color::Cyan)),
            Span::raw("  "),
            Span::styled(
                commit.subject.clone(),
                if selected {
                    Style::new().add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                },
            ),
        ]);
        frame.render_widget(Line::from(spans), area);
    }

    let status = if app.status.is_empty() {
        format!(
            "[{}/{}]  j/k move  ^d/^u page  g/G ends  D/Enter diff  q quit",
            (app.cursor + 1).min(app.commits.len()),
            app.commits.len()
        )
    } else {
        app.status.clone()
    };
    frame.render_widget(
        Line::styled(status, Style::new().fg(Color::Black).bg(Color::Indexed(250))),
        status_area,
    );
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
