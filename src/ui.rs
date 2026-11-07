//! Rendering: header, three Miller columns, status line and popups.

use crate::app::{App, InputKind, Mode, HELP};
use crate::fs::{self, Entry};
use crate::icons;
use crate::preview::Preview;
use crate::theme::Theme;
use ratatui::prelude::*;
use ratatui::widgets::*;
use unicode_width::UnicodeWidthStr;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let theme = app.theme.clone();
    f.render_widget(Block::new().style(Style::default().bg(theme.bg).fg(theme.fg)), area);

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Min(1), Constraint::Length(1)]).split(area);
    draw_header(f, app, &theme, rows[0]);
    draw_columns(f, app, &theme, rows[1]);
    draw_status(f, app, &theme, rows[2]);

    match &app.mode {
        Mode::Confirm { prompt, .. } => draw_confirm(f, &theme, area, prompt),
        Mode::Help => draw_help(f, &theme, area),
        _ => {}
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    if max == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if w + cw > max - 1 {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}

fn draw_header(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let bar = Style::default().bg(theme.blue).fg(theme.bg);
    let bold = bar.add_modifier(Modifier::BOLD);
    let open = if app.icons { icons::DIR_OPEN } else { icons::ASCII_DIR_OPEN };
    let mut right = Vec::new();
    if !app.filter.is_empty() {
        right.push(format!("/{}", app.filter));
    }
    if app.show_hidden {
        right.push("hidden".into());
    }
    right.push(format!("sort:{}", app.sort.label()));
    right.push(if app.entries.is_empty() {
        "0 items".into()
    } else {
        format!("{}/{}", app.cursor + 1, app.entries.len())
    });
    right.push(app.theme.name.clone());
    let right = format!(" {} ", right.join("  "));
    let right_w = right.width() as u16;
    let cols = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_w.min(area.width))]).split(area);

    let path = fs::display_path(&app.cwd);
    let path = truncate(&path, cols[0].width.saturating_sub(4) as usize);
    let left = Line::from(vec![Span::styled(format!(" {open} "), bar), Span::styled(path, bold)]);
    f.render_widget(Paragraph::new(left).style(bar), cols[0]);
    f.render_widget(Paragraph::new(right).style(bar).alignment(Alignment::Right), cols[1]);
}

fn entry_style(e: &Entry, theme: &Theme, marked: bool) -> Style {
    let mut style = Style::default().fg(theme.fg);
    if marked {
        return style.fg(theme.yellow).add_modifier(Modifier::BOLD);
    }
    if e.is_dir {
        style = style.fg(theme.blue).add_modifier(Modifier::BOLD);
    } else if e.is_symlink {
        style = style.fg(theme.cyan);
    } else if e.is_exec {
        style = style.fg(theme.green);
    } else {
        style = match e.ext().as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "mp4" | "mkv" | "webm" | "mp3" | "flac" | "wav" => {
                style.fg(theme.magenta)
            }
            "zip" | "tar" | "gz" | "xz" | "zst" | "bz2" | "7z" | "rar" | "iso" => style.fg(theme.red),
            _ => style,
        };
    }
    if e.is_hidden() {
        style = style.add_modifier(Modifier::DIM);
    }
    style
}

/// One row: mark indicator, icon, name, right-aligned size.
fn entry_line<'a>(e: &Entry, width: usize, theme: &Theme, use_icons: bool, marked: bool, show_size: bool) -> Line<'a> {
    let style = entry_style(e, theme, marked);
    let icon = icons::for_entry(e, use_icons);
    let size = if show_size && !e.is_dir { fs::human_size(e.size) } else { String::new() };
    let mark = if marked { "•" } else { " " };
    // mark(1) + icon(1-2) + space + name + space + size
    let fixed = 1 + icon.width() + 1 + if size.is_empty() { 0 } else { size.width() + 1 };
    let name_w = width.saturating_sub(fixed);
    let name = truncate(&e.name, name_w);
    let pad = name_w.saturating_sub(name.width());
    let mut spans = vec![
        Span::styled(mark.to_string(), Style::default().fg(theme.yellow)),
        Span::styled(format!("{icon} "), style),
        Span::styled(name, style),
    ];
    if !size.is_empty() {
        spans.push(Span::raw(" ".repeat(pad + 1)));
        spans.push(Span::styled(size, Style::default().fg(theme.bright_black)));
    }
    Line::from(spans)
}

fn column_block(theme: &Theme, last: bool) -> Block<'static> {
    let b = Block::new().padding(Padding::horizontal(1));
    if last {
        b
    } else {
        b.borders(Borders::RIGHT).border_style(Style::default().fg(theme.bright_black))
    }
}

fn draw_columns(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect) {
    let (show_parent, show_preview) = match area.width {
        0..=59 => (false, false),
        60..=99 => (false, true),
        _ => (true, true),
    };
    let mut constraints = Vec::new();
    if show_parent {
        constraints.push(Constraint::Percentage(20));
    }
    constraints.push(Constraint::Percentage(if show_preview { if show_parent { 38 } else { 45 } } else { 100 }));
    if show_preview {
        constraints.push(Constraint::Min(0));
    }
    let cols = Layout::horizontal(constraints).split(area);
    let mut idx = 0;

    if show_parent {
        draw_parent(f, app, theme, cols[idx]);
        idx += 1;
    }
    draw_current(f, app, theme, cols[idx], !show_preview);
    idx += 1;
    if show_preview {
        draw_preview(f, app, theme, cols[idx]);
    }
}

fn draw_parent(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let block = column_block(theme, false);
    let inner = block.inner(area);
    let items: Vec<ListItem> = app
        .parent_entries
        .iter()
        .map(|e| ListItem::new(entry_line(e, inner.width as usize, theme, app.icons, false, false)))
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(theme.bright_black).fg(theme.fg).add_modifier(Modifier::BOLD));
    let mut state = ListState::default().with_selected(app.parent_cursor);
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_current(f: &mut Frame, app: &mut App, theme: &Theme, area: Rect, last: bool) {
    let block = column_block(theme, last);
    let inner = block.inner(area);
    app.page_size = inner.height.max(1) as usize;
    if app.entries.is_empty() {
        let text = if app.filter.is_empty() { "empty" } else { "no match" };
        f.render_widget(
            Paragraph::new(text).style(Style::default().fg(theme.bright_black).add_modifier(Modifier::ITALIC)).block(block),
            area,
        );
        return;
    }
    let items: Vec<ListItem> = app
        .entries
        .iter()
        .map(|e| {
            let marked = app.marked.contains(&e.path);
            ListItem::new(entry_line(e, inner.width as usize, theme, app.icons, marked, true))
        })
        .collect();
    let highlight_bg = if app.current().map(|e| app.marked.contains(&e.path)).unwrap_or(false) {
        theme.yellow
    } else {
        theme.blue
    };
    let list = List::new(items)
        .block(block)
        .highlight_style(Style::default().bg(highlight_bg).fg(theme.bg).add_modifier(Modifier::BOLD))
        .scroll_padding(3);
    f.render_stateful_widget(list, area, &mut app.list_state);
}

fn draw_preview(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let block = column_block(theme, true);
    let inner = block.inner(area);
    let dim = Style::default().fg(theme.bright_black);
    match &app.preview {
        Preview::Empty => {}
        Preview::Error(e) => {
            f.render_widget(Paragraph::new(e.as_str()).style(Style::default().fg(theme.red)).block(block), area);
        }
        Preview::Dir(entries) => {
            if entries.is_empty() {
                f.render_widget(Paragraph::new("empty").style(dim.add_modifier(Modifier::ITALIC)).block(block), area);
                return;
            }
            let items: Vec<ListItem> = entries
                .iter()
                .take(inner.height as usize)
                .map(|e| ListItem::new(entry_line(e, inner.width as usize, theme, app.icons, false, true)))
                .collect();
            f.render_widget(List::new(items).block(block), area);
        }
        Preview::Text(lines) => {
            let gutter = lines.len().to_string().len().max(2);
            let text: Vec<Line> = lines
                .iter()
                .take(inner.height as usize)
                .enumerate()
                .map(|(i, l)| {
                    Line::from(vec![
                        Span::styled(format!("{:>gutter$} ", i + 1), dim),
                        Span::raw(truncate(l, (inner.width as usize).saturating_sub(gutter + 1))),
                    ])
                })
                .collect();
            f.render_widget(Paragraph::new(text).block(block), area);
        }
        Preview::Info(rows) => {
            let key_w = rows.iter().map(|(k, _)| k.width()).max().unwrap_or(0);
            let mut text: Vec<Line> = Vec::new();
            if let Some(e) = app.current() {
                text.push(Line::from(Span::styled(
                    format!("{} {}", icons::for_entry(e, app.icons), e.name),
                    Style::default().fg(theme.blue).add_modifier(Modifier::BOLD),
                )));
                text.push(Line::raw(""));
            }
            for (k, v) in rows {
                text.push(Line::from(vec![
                    Span::styled(format!("{k:>key_w$}  "), dim),
                    Span::raw(v.clone()),
                ]));
            }
            f.render_widget(Paragraph::new(text).wrap(Wrap { trim: false }).block(block), area);
        }
    }
}

fn draw_status(f: &mut Frame, app: &App, theme: &Theme, area: Rect) {
    let dim = Style::default().fg(theme.bright_black);
    let accent = Style::default().fg(theme.yellow);
    let (line, cursor_x): (Line, Option<u16>) = match &app.mode {
        Mode::Filter => {
            let l = Line::from(vec![Span::styled(" /", accent), Span::raw(app.filter.clone())]);
            (l, Some(2 + app.filter.width() as u16))
        }
        Mode::Input { kind, buf } => {
            let prompt = match kind {
                InputKind::Rename(_) => " rename: ",
                InputKind::NewFile => " new file: ",
                InputKind::NewDir => " new dir: ",
            };
            let l = Line::from(vec![Span::styled(prompt, accent), Span::raw(buf.clone())]);
            (l, Some((prompt.width() + buf.width()) as u16))
        }
        _ => {
            let left = if let Some(m) = &app.message {
                let style = if m.error {
                    Style::default().fg(theme.red).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.green)
                };
                Line::from(Span::styled(format!(" {}", m.text), style))
            } else {
                let mut spans = vec![Span::raw(" ")];
                for (key, desc) in [("?", "help"), ("/", "filter"), ("a", "new"), ("r", "rename"), ("d", "trash"), ("y/x/p", "copy/cut/paste"), ("q", "quit")] {
                    spans.push(Span::styled(key, accent));
                    spans.push(Span::styled(format!(" {desc}  "), dim));
                }
                Line::from(spans)
            };
            (left, None)
        }
    };

    let mut right = Vec::new();
    if !app.marked.is_empty() {
        right.push(format!("{} marked", app.marked.len()));
    }
    if let Some(clip) = &app.clipboard {
        right.push(format!("{} {}", clip.paths.len(), if clip.cut { "cut" } else { "yanked" }));
    }
    let right = if right.is_empty() { String::new() } else { format!("{} ", right.join("  ")) };
    let right_w = right.width() as u16;
    let cols = Layout::horizontal([Constraint::Min(0), Constraint::Length(right_w.min(area.width))]).split(area);
    f.render_widget(Paragraph::new(line), cols[0]);
    f.render_widget(Paragraph::new(right).style(accent).alignment(Alignment::Right), cols[1]);
    if let Some(x) = cursor_x {
        f.set_cursor_position((area.x + x.min(area.width.saturating_sub(1)), area.y));
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

fn popup_block(theme: &Theme, title: &str) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.blue))
        .title(Span::styled(format!(" {title} "), Style::default().fg(theme.blue).add_modifier(Modifier::BOLD)))
        .padding(Padding::new(2, 2, 1, 1))
        .style(Style::default().bg(theme.bg).fg(theme.fg))
}

fn draw_confirm(f: &mut Frame, theme: &Theme, area: Rect, prompt: &str) {
    let width = (prompt.width() as u16 + 8).clamp(30, area.width.saturating_sub(4).max(30));
    let rect = centered(area, width, 7);
    f.render_widget(Clear, rect);
    let text = vec![
        Line::raw(prompt.to_string()),
        Line::raw(""),
        Line::from(vec![
            Span::styled("y", Style::default().fg(theme.green).add_modifier(Modifier::BOLD)),
            Span::raw(" yes    "),
            Span::styled("n", Style::default().fg(theme.red).add_modifier(Modifier::BOLD)),
            Span::raw(" no"),
        ]),
    ];
    f.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }).block(popup_block(theme, "Confirm")), rect);
}

fn draw_help(f: &mut Frame, theme: &Theme, area: Rect) {
    let key_w = HELP.iter().map(|(k, _)| k.width()).max().unwrap_or(0);
    let height = (HELP.len() as u16 + 5).min(area.height);
    let rect = centered(area, 66, height);
    f.render_widget(Clear, rect);
    let mut text: Vec<Line> = HELP
        .iter()
        .map(|(k, d)| {
            Line::from(vec![
                Span::styled(format!("{k:<key_w$}  "), Style::default().fg(theme.yellow)),
                Span::raw(*d),
            ])
        })
        .collect();
    text.push(Line::raw(""));
    text.push(Line::styled("press any key to close", Style::default().fg(theme.bright_black)));
    f.render_widget(Paragraph::new(text).block(popup_block(theme, "omafile keys")), rect);
}
