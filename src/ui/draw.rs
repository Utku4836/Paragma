//! Drawing the interface: a header, the sections on the left, the chosen
//! one on the right, keys and messages at the bottom, windows on top. Each
//! frame also keeps where things are, for the mouse (`App::hits`).

use super::app::{App, Focus, Hit, Modal, PlanRow, SECTIONS, SETTINGS, Section};
use super::markdown;
use super::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

pub fn draw(f: &mut Frame, app: &mut App) {
    app.hits.clear();
    let area = f.area();
    let [header, body, footer] = rows3(area);
    draw_header(f, app, header);
    let [side, main] = {
        let c = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(20), Constraint::Min(20)])
            .split(body);
        [c[0], c[1]]
    };
    draw_sidebar(f, app, side);
    if app.open.is_some() {
        draw_editor(f, app, main);
    } else {
        match app.section {
            Section::Projects => draw_projects(f, app, main),
            Section::Notes => draw_notes(f, app, main),
            Section::Plan => draw_plan(f, app, main),
            Section::Files => draw_files(f, app, main),
            Section::Activity => draw_activity(f, app, main),
            Section::Sync => draw_sync(f, app, main),
            Section::Settings => draw_settings(f, app, main),
        }
    }
    draw_footer(f, app, footer);
    if app.modal.is_some() {
        draw_modal(f, app, area);
    }
}

fn rows3(area: Rect) -> [Rect; 3] {
    let c = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3), Constraint::Length(1)])
        .split(area);
    [c[0], c[1], c[2]]
}

fn block<'a>(title: &'a str, focused: bool) -> Block<'a> {
    Block::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(theme::border(focused)).title(
        Span::styled(
            format!(" {title} "),
            if focused { theme::accent().add_modifier(Modifier::BOLD) } else { theme::dim() },
        ),
    )
}

fn draw_header(f: &mut Frame, app: &mut App, area: Rect) {
    let mut spans = vec![
        Span::styled(
            " PARAGMA ",
            Style::default().fg(ratatui::style::Color::Black).bg(theme::ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
    ];
    if let Some(p) = &app.project {
        spans.push(Span::styled(p.name.clone(), theme::text().add_modifier(Modifier::BOLD)));
        spans.push(Span::styled("  ›  ", theme::faint()));
    }
    let label = SECTIONS.iter().find(|s| s.0 == app.section).map(|s| s.2).unwrap_or("");
    spans.push(Span::styled(label, theme::dim()));
    if let Some(o) = &app.open {
        spans.push(Span::styled("  ›  ", theme::faint()));
        spans.push(Span::styled(o.rel.trim_end_matches(".md").to_string(), theme::dim()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
    // the buttons on the right
    let right = " ⌕ search   ? help   ✕ quit ";
    let w = right.chars().count() as u16;
    if area.width > w + 30 {
        let r = Rect { x: area.x + area.width - w, y: area.y, width: w, height: 1 };
        f.render_widget(Paragraph::new(Span::styled(right, theme::dim())), r);
        app.hits.push((Rect { x: r.x, width: 11, ..r }, Hit::Button("search".into())));
        app.hits.push((Rect { x: r.x + 11, width: 9, ..r }, Hit::Button("help".into())));
        app.hits.push((Rect { x: r.x + 20, width: 8, ..r }, Hit::Button("quit".into())));
    }
}

fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Sidebar && app.open.is_none();
    let b = block("Paragma", focused);
    let inner = b.inner(area);
    f.render_widget(b, area);
    for (i, (s, icon, label)) in SECTIONS.iter().enumerate() {
        let y = inner.y + i as u16 * 2;
        if y >= inner.y + inner.height {
            break;
        }
        let chosen = *s == app.section;
        let style = if chosen { theme::chosen() } else { theme::text() };
        let text = format!(" {icon}  {label:<11}{}", i + 1);
        let r = Rect { x: inner.x, y, width: inner.width, height: 1 };
        f.render_widget(Paragraph::new(Span::styled(fit(&text, inner.width as usize), style)), r);
        app.hits.push((r, Hit::Section(i)));
    }
}

/// A list of rows that scrolls with its chosen one; each row clickable.
fn draw_list(f: &mut Frame, app: &mut App, area: Rect, section: Section, lines: Vec<Line<'static>>, empty: &str) {
    let k = SECTIONS.iter().position(|s| s.0 == section).unwrap_or(0);
    if lines.is_empty() {
        f.render_widget(Paragraph::new(Span::styled(empty.to_string(), theme::dim())).wrap(Wrap { trim: false }), area);
        return;
    }
    let height = area.height as usize;
    let cur = app.cur[k];
    if cur < app.top[k] {
        app.top[k] = cur;
    }
    if cur >= app.top[k] + height {
        app.top[k] = cur + 1 - height;
    }
    let focused = app.focus == Focus::Main && app.modal.is_none();
    for (row, line) in lines.into_iter().enumerate().skip(app.top[k]).take(height) {
        let y = area.y + (row - app.top[k]) as u16;
        let r = Rect { x: area.x, y, width: area.width, height: 1 };
        let line = if row == cur {
            let style = if focused { theme::chosen() } else { Style::default().bg(theme::FAINT) };
            line.style(style)
        } else {
            line
        };
        f.render_widget(Paragraph::new(line), r);
        app.hits.push((r, Hit::Row(row)));
    }
}

fn draw_projects(f: &mut Frame, app: &mut App, area: Rect) {
    let b = block("Projects", app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let current = app.project.as_ref().map(|p| p.name.clone()).unwrap_or_default();
    let lines: Vec<Line<'static>> = app
        .projects
        .iter()
        .map(|p| {
            let mark = if p.name == current { "● " } else { "  " };
            Line::from(vec![
                Span::styled(mark, theme::accent()),
                Span::styled(format!("{:<28}", p.name), theme::text().add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:>5} notes   ", p.notes), theme::dim()),
                Span::styled(
                    format!("{:>3} open steps   ", p.open),
                    if p.open > 0 { theme::accent() } else { theme::dim() },
                ),
                Span::styled(
                    if p.last.is_empty() { String::new() } else { format!("last handoff {}", p.last) },
                    theme::faint(),
                ),
            ])
        })
        .collect();
    draw_list(f, app, inner, Section::Projects, lines, "No projects yet. Press n to make one.");
}

fn no_project(f: &mut Frame, area: Rect, title: &str) {
    let b = block(title, true);
    let inner = b.inner(area);
    f.render_widget(b, area);
    f.render_widget(Paragraph::new(Span::styled("Choose a project first (1: Projects).", theme::dim())), inner);
}

fn draw_notes(f: &mut Frame, app: &mut App, area: Rect) {
    if app.project.is_none() {
        return no_project(f, area, "Notes");
    }
    let c = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);
    let b = block("Notes", app.focus == Focus::Main);
    let inner = b.inner(c[0]);
    f.render_widget(b, c[0]);
    let lines: Vec<Line<'static>> = app
        .tree
        .iter()
        .map(|r| {
            let pad = "  ".repeat(r.depth);
            if r.folder {
                let open = app.expanded.contains(&r.rel);
                Line::from(vec![
                    Span::raw(pad),
                    Span::styled(if open { "▾ " } else { "▸ " }, theme::accent()),
                    Span::styled(r.title.clone(), theme::text().add_modifier(Modifier::BOLD)),
                    Span::styled(format!("  {}", r.count), theme::faint()),
                ])
            } else {
                Line::from(vec![
                    Span::raw(format!("{pad}  ")),
                    Span::styled(r.title.clone(), theme::text()),
                    Span::styled(format!("  {}", r.tag), theme::faint()),
                ])
            }
        })
        .collect();
    draw_list(f, app, inner, Section::Notes, lines, "No notes yet. Press n for a new note, N for a folder.");
    // the chosen note, as it reads
    let pb = block("Preview", false);
    let pin = pb.inner(c[1]);
    f.render_widget(pb, c[1]);
    let Some(row) = app.tree.get(app.cur()).cloned() else { return };
    if row.folder {
        let text = format!("{} notes in {}.\n\nEnter opens the folder; n makes a note in it.", row.count, row.rel);
        f.render_widget(Paragraph::new(Span::styled(text, theme::dim())).wrap(Wrap { trim: false }), pin);
        return;
    }
    let Some(p) = &app.project else { return };
    let text = crate::project::files::read_text(&p.dir.join(&row.rel)).unwrap_or_default();
    let note = crate::note::Note::parse(&text);
    let mut lines = vec![Line::from(vec![
        Span::styled(row.title.clone(), theme::accent().add_modifier(Modifier::BOLD)),
        Span::styled(format!("   {}", row.tag), theme::faint()),
    ])];
    if let Some(d) = note.props.text("due") {
        let when = crate::project::items::days_left(d).map(crate::project::items::when).unwrap_or_default();
        lines.push(Line::from(Span::styled(format!("due {d} ({when})"), theme::warn())));
    }
    lines.push(Line::from(""));
    lines.extend(markdown::render(&note.body));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), pin);
}

fn draw_plan(f: &mut Frame, app: &mut App, area: Rect) {
    if app.project.is_none() {
        return no_project(f, area, "Plan");
    }
    let b = block("Plan", app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let lines: Vec<Line<'static>> = app
        .plan_rows()
        .into_iter()
        .map(|r| match r {
            PlanRow::Goal(g) => Line::from(vec![
                Span::styled("Goal  ", theme::accent().add_modifier(Modifier::BOLD)),
                Span::styled(if g.is_empty() { "(none yet)".into() } else { g }, theme::text()),
            ]),
            PlanRow::Step(n, s) => {
                let (mark, style) = if s.done {
                    ("☑", theme::dim().add_modifier(Modifier::CROSSED_OUT))
                } else if s.deferred {
                    ("◌", theme::dim())
                } else {
                    ("☐", theme::text())
                };
                Line::from(vec![
                    Span::styled(format!("  {mark} "), if s.done { theme::ok() } else { theme::accent() }),
                    Span::styled(format!("{n:>3}. "), theme::faint()),
                    Span::styled(s.text.clone(), style),
                    Span::styled(if s.deferred { "  (later)".to_string() } else { String::new() }, theme::faint()),
                ])
            }
            PlanRow::Question(n, q) => Line::from(vec![
                Span::styled(if q.answer.is_some() { "  ✓ " } else { "  ? " }, theme::warn()),
                Span::styled(format!("{n}. {}", q.text), theme::text()),
                Span::styled(q.answer.map(|a| format!("  → {a}")).unwrap_or_default(), theme::dim()),
            ]),
            PlanRow::Handoff(h) => Line::from(vec![
                Span::styled("  ⇢ ", theme::faint()),
                Span::styled(format!("{}  ", h.heading), theme::dim()),
                Span::styled(crate::output::one_line(&h.text, 120), theme::faint()),
            ]),
        })
        .collect();
    draw_list(f, app, inner, Section::Plan, lines, "");
}

fn draw_files(f: &mut Frame, app: &mut App, area: Rect) {
    if app.project.is_none() {
        return no_project(f, area, "Files");
    }
    let b = block("Files kept with notes", app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let lines: Vec<Line<'static>> = app
        .files
        .iter()
        .map(|r| {
            Line::from(vec![
                Span::styled("▣ ", theme::accent()),
                Span::styled(format!("{:<40}", fit(&r.rel, 40)), theme::text()),
                Span::styled(format!("{:>9}  ", crate::project::attach::size(r.size)), theme::dim()),
                Span::styled(format!("{}  ", r.date), theme::faint()),
                Span::styled(format!("with {}", r.note), theme::dim()),
                Span::styled(
                    if r.versions > 0 { format!("  v{}", r.versions + 1) } else { String::new() },
                    theme::accent(),
                ),
                Span::styled(
                    std::path::Path::new(&r.source)
                        .parent()
                        .map(|d| format!("   from {}", d.display()))
                        .unwrap_or_default(),
                    theme::faint(),
                ),
            ])
        })
        .collect();
    draw_list(
        f,
        app,
        inner,
        Section::Files,
        lines,
        "No files kept with notes. Press a to keep one (a workbook, a contract, a deck...).",
    );
}

fn draw_activity(f: &mut Frame, app: &mut App, area: Rect) {
    if app.project.is_none() {
        return no_project(f, area, "Activity");
    }
    let b = block("Activity: what agents and pg did here", app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let now = crate::output::now();
    let lines: Vec<Line<'static>> = app
        .activity
        .iter()
        .map(|a| {
            Line::from(vec![
                Span::styled(format!("{:>12}  ", crate::output::ago(a.at, now)), theme::faint()),
                Span::styled(format!("{:<18}", fit(&a.who, 18)), if a.auto { theme::accent() } else { theme::dim() }),
                Span::styled(a.what.clone(), theme::text()),
                Span::styled(if a.undo.is_some() { "   u: undo".to_string() } else { String::new() }, theme::warn()),
            ])
        })
        .collect();
    draw_list(f, app, inner, Section::Activity, lines, "Nothing done here in the last two weeks.");
}

fn draw_sync(f: &mut Frame, app: &mut App, area: Rect) {
    let b = block("Sync", app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let mut head: Vec<Line<'static>> = Vec::new();
    match crate::sync::Config::load(&app.home) {
        Some(c) => {
            let s = crate::sync::Status::load(&app.home);
            head.push(Line::from(vec![
                Span::styled("repository  ", theme::faint()),
                Span::styled(c.place(), theme::text().add_modifier(Modifier::BOLD)),
                Span::styled(
                    if c.public { "  public" } else { "  private" },
                    if c.public { theme::bad() } else { theme::ok() },
                ),
            ]));
            let now = crate::output::now();
            head.push(Line::from(vec![
                Span::styled("last sync   ", theme::faint()),
                Span::styled(
                    if s.synced > 0 {
                        format!("{}: {}", crate::output::ago(s.synced, now), s.did)
                    } else {
                        "not yet".into()
                    },
                    theme::dim(),
                ),
            ]));
            if !s.error.is_empty() {
                head.push(Line::from(Span::styled(format!("last try failed: {}", s.error), theme::bad())));
            }
        }
        None => {
            head.push(Line::from(Span::styled("Not connected: the memory is only on this computer.", theme::dim())))
        }
    }
    head.push(Line::from(""));
    let h = head.len() as u16;
    f.render_widget(Paragraph::new(head), Rect { height: h.min(inner.height), ..inner });
    let list = Rect { y: inner.y + h, height: inner.height.saturating_sub(h), ..inner };
    let lines: Vec<Line<'static>> = app
        .sync_rows()
        .into_iter()
        .map(|(a, v)| {
            Line::from(vec![
                Span::styled(format!("  {:<28}", a), theme::text().add_modifier(Modifier::BOLD)),
                Span::styled(v, theme::dim()),
            ])
        })
        .collect();
    draw_list(f, app, list, Section::Sync, lines, "");
}

fn draw_settings(f: &mut Frame, app: &mut App, area: Rect) {
    let b = block("Settings", app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let lines: Vec<Line<'static>> = SETTINGS
        .iter()
        .map(|(a, d)| {
            Line::from(vec![
                Span::styled(format!("  {a:<34}"), theme::text().add_modifier(Modifier::BOLD)),
                Span::styled(*d, theme::dim()),
            ])
        })
        .collect();
    let list = Rect { height: inner.height.saturating_sub(4), ..inner };
    draw_list(f, app, list, Section::Settings, lines, "");
    let about = vec![
        Line::from(Span::styled(format!("Paragma {}", env!("CARGO_PKG_VERSION")), theme::faint())),
        Line::from(Span::styled(format!("memory  {}", app.home.root.display()), theme::faint())),
    ];
    let y = inner.y + inner.height.saturating_sub(2);
    f.render_widget(Paragraph::new(about), Rect { y, height: 2, ..inner });
}

/// A button of a toolbar: its label, kept clickable; left out when it
/// does not fit before `end`.
fn button(f: &mut Frame, app: &mut App, x: &mut u16, y: u16, end: u16, label: &str, name: &str) {
    let text = format!(" {label} ");
    let w = text.chars().count() as u16;
    if *x + w > end {
        return;
    }
    let r = Rect { x: *x, y, width: w, height: 1 };
    f.render_widget(Paragraph::new(Span::styled(text, Style::default().fg(theme::TEXT).bg(theme::FAINT))), r);
    app.hits.push((r, Hit::Button(name.into())));
    *x += w + 1;
}

fn draw_editor(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(o) = app.open.as_ref() else { return };
    let title = format!(
        "{} {}{}{}",
        if o.reading { "◉" } else { "✎" },
        o.rel.trim_end_matches(".md"),
        if o.ed.dirty { "  ● unsaved" } else { "" },
        if o.outside { "  ⚠ changed outside" } else { "" }
    );
    let reading = o.reading;
    let b = block(&title, true);
    let inner = b.inner(area);
    f.render_widget(b, area);
    // the toolbar
    let mut x = inner.x;
    let y = inner.y;
    let end = inner.x + inner.width;
    if inner.height == 0 {
        return;
    }
    if reading {
        button(f, app, &mut x, y, end, "✎ Edit  e", "edit");
        button(f, app, &mut x, y, end, "✕ Close  Esc", "close");
    } else {
        button(f, app, &mut x, y, end, "Save ^S", "save");
        button(f, app, &mut x, y, end, "Find ^F", "find");
        button(f, app, &mut x, y, end, "Date ^D", "date");
        button(f, app, &mut x, y, end, "Table ^T", "table");
        button(f, app, &mut x, y, end, "To-do ^⏎", "todo");
        button(f, app, &mut x, y, end, "Attach ^O", "attach");
        button(f, app, &mut x, y, end, "Read ^P", "read");
    }
    let Some(o) = app.open.as_mut() else { return };
    let finding = o.find.is_some();
    let text_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: inner.width,
        height: inner.height.saturating_sub(if finding { 3 } else { 2 }),
    };
    app.ed_area = text_area;
    app.hits.push((text_area, Hit::Editor));
    let Some(o) = app.open.as_mut() else { return };
    if reading {
        let lines = markdown::render(&o.ed.text());
        let max = lines.len().saturating_sub(1);
        o.ed.scroll = o.ed.scroll.min(max);
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((o.ed.scroll as u16, 0)), text_area);
        return;
    }
    let width = text_area.width as usize;
    let height = text_area.height as usize;
    let rows = o.ed.rows(width);
    o.ed.follow(&rows, height);
    let sel = o.ed.selection();
    let mut fenced = false;
    let mut fence_at: Vec<bool> = Vec::with_capacity(o.ed.lines.len());
    for l in &o.ed.lines {
        fence_at.push(fenced);
        if l.trim_start().starts_with("```") {
            fenced = !fenced;
        }
    }
    for (vi, (r, a, b)) in rows.iter().enumerate().skip(o.ed.scroll).take(height) {
        let line = &o.ed.lines[*r];
        let chars: Vec<char> = line.chars().collect();
        let part: String = chars[*a..*b].iter().collect();
        let base = markdown::edit_style(line, fence_at[*r]);
        let spans: Vec<Span> = match sel {
            Some((s, e)) if *r >= s.row && *r <= e.row => {
                let from = if *r == s.row { s.col.max(*a).min(*b) } else { *a };
                let to = if *r == e.row { e.col.max(*a).min(*b) } else { *b };
                let pre: String = chars[*a..from].iter().collect();
                let mid: String = chars[from..to].iter().collect();
                let post: String = chars[to..*b].iter().collect();
                // a selected line end shows as one cell
                let mid = if mid.is_empty() && *r != e.row && *b == chars.len() { " ".to_string() } else { mid };
                vec![Span::styled(pre, base), Span::styled(mid, theme::selected()), Span::styled(post, base)]
            }
            _ => vec![Span::styled(part, base)],
        };
        let y = text_area.y + (vi - o.ed.scroll) as u16;
        f.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect { x: text_area.x, y, width: text_area.width, height: 1 },
        );
    }
    let (cr, cc) = o.ed.cursor_row(&rows);
    if !finding && cr >= o.ed.scroll && cr < o.ed.scroll + height {
        f.set_cursor_position((text_area.x + cc as u16, text_area.y + (cr - o.ed.scroll) as u16));
        // the notes a [[link]] can go to
        if !o.links.is_empty() {
            let w = o.links.iter().map(|l| l.chars().count()).max().unwrap_or(10).min(50) as u16 + 4;
            let x = (text_area.x + cc as u16).min(text_area.x + text_area.width.saturating_sub(w));
            let y = text_area.y + (cr - o.ed.scroll) as u16 + 1;
            let h = (o.links.len() as u16 + 2).min(text_area.height.saturating_sub(y - text_area.y));
            let r = Rect { x, y, width: w, height: h };
            f.render_widget(Clear, r);
            let lines: Vec<Line> = o
                .links
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    Line::from(Span::styled(l.clone(), if i == o.link_cur { theme::chosen() } else { theme::text() }))
                })
                .collect();
            f.render_widget(Paragraph::new(lines).block(block("link", true)), r);
        }
    }
    if let Some(q) = &o.find {
        let y = inner.y + inner.height.saturating_sub(1);
        let r = Rect { x: inner.x, y, width: inner.width, height: 1 };
        let line = Line::from(vec![
            Span::styled(" Find ", Style::default().fg(ratatui::style::Color::Black).bg(theme::ACCENT)),
            Span::styled(format!(" {q}"), theme::text()),
            Span::styled("   Enter next · Shift+Enter before · Esc close", theme::faint()),
        ]);
        f.render_widget(Paragraph::new(line), r);
        f.set_cursor_position((inner.x + 7 + q.chars().count() as u16, y));
    }
}

fn keys<'a>(pairs: &[(&'a str, &'a str)]) -> Vec<Span<'a>> {
    let mut out = Vec::new();
    for (k, what) in pairs {
        out.push(Span::styled(format!(" {k} "), theme::key()));
        out.push(Span::styled(format!(" {what}  "), theme::dim()));
    }
    out
}

fn draw_footer(f: &mut Frame, app: &mut App, area: Rect) {
    if let Some((text, bad, at)) = &app.status
        && at.elapsed().as_secs() < 8
    {
        let style = if *bad { theme::bad() } else { theme::ok() };
        f.render_widget(Paragraph::new(Span::styled(format!(" {text}"), style)), area);
        return;
    }
    let hint: Vec<(&str, &str)> = if app.open.as_ref().is_some_and(|o| o.reading) {
        vec![("e", "edit"), ("↑↓", "scroll"), ("Esc", "close")]
    } else if app.open.is_some() {
        vec![
            ("^S", "save"),
            ("^Z", "undo"),
            ("^C ^X ^V", "copy cut paste"),
            ("Tab", "next cell"),
            ("[[", "link"),
            ("Esc", "close"),
        ]
    } else {
        match app.section {
            Section::Projects => {
                vec![("Enter", "open"), ("n", "new project"), ("/", "search"), ("Tab", "sections"), ("q", "quit")]
            }
            Section::Notes => vec![
                ("Enter", "edit"),
                ("→", "read"),
                ("n", "new note"),
                ("N", "folder"),
                ("r", "rename/move"),
                ("a", "keep a file"),
                ("d", "archive"),
                ("/", "search"),
            ],
            Section::Plan => vec![
                ("Space", "done / open"),
                ("a", "add step"),
                ("e", "edit"),
                ("l", "later"),
                ("d", "drop"),
                ("Enter", "answer a question"),
            ],
            Section::Files => vec![("Enter", "open"), ("a", "keep a file"), ("g", "go to its note")],
            Section::Activity => vec![("u", "undo what pg did"), ("F5", "refresh")],
            Section::Sync => vec![("Enter", "do it")],
            Section::Settings => vec![("Enter", "do it")],
        }
    };
    f.render_widget(Paragraph::new(Line::from(keys(&hint))), area);
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(4));
    let h = h.min(area.height.saturating_sub(2));
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

fn draw_modal(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(m) = app.modal.clone() else { return };
    match m {
        Modal::Input { title, text, .. } => {
            let r = centered(area, 70, 5);
            f.render_widget(Clear, r);
            let b = block(&title, true);
            let inner = b.inner(r);
            f.render_widget(b, r);
            f.render_widget(
                Paragraph::new(Span::styled(format!(" {text}"), theme::text())),
                Rect { height: 1, ..inner },
            );
            f.render_widget(
                Paragraph::new(Span::styled(" Enter ok · Esc cancel · Ctrl+V paste", theme::faint())),
                Rect { y: inner.y + 2, height: 1, ..inner },
            );
            f.set_cursor_position((inner.x + 1 + text.chars().count() as u16, inner.y));
        }
        Modal::Confirm { title, lines, .. } => {
            let r = centered(area, 72, lines.len() as u16 + 5);
            f.render_widget(Clear, r);
            let b = block(&title, true);
            let inner = b.inner(r);
            f.render_widget(b, r);
            let body: Vec<Line> =
                lines.iter().map(|l| Line::from(Span::styled(format!(" {l}"), theme::dim()))).collect();
            f.render_widget(
                Paragraph::new(body).wrap(Wrap { trim: false }),
                Rect { height: inner.height.saturating_sub(1), ..inner },
            );
            let y = inner.y + inner.height.saturating_sub(1);
            let mut x = inner.x + 1;
            for (i, (label, _)) in [(" Yes  y ", 0), (" No  n ", 1)].iter().enumerate() {
                let w = label.chars().count() as u16;
                if x + w > inner.x + inner.width || inner.height == 0 {
                    break;
                }
                let rr = Rect { x, y, width: w, height: 1 };
                let style = if i == 0 {
                    Style::default().fg(ratatui::style::Color::Black).bg(theme::ACCENT)
                } else {
                    Style::default().fg(theme::TEXT).bg(theme::FAINT)
                };
                f.render_widget(Paragraph::new(Span::styled(*label, style)), rr);
                app.hits.push((rr, Hit::ModalButton(i)));
                x += w + 2;
            }
        }
        Modal::Changed { .. } => {
            let r = centered(area, 76, 8);
            f.render_widget(Clear, r);
            let b = block("This note was changed outside meanwhile", true);
            let inner = b.inner(r);
            f.render_widget(b, r);
            let lines = vec![
                Line::from(Span::styled(
                    " An agent (or another window) changed it since you opened it.",
                    theme::warn(),
                )),
                Line::from(Span::styled(
                    " Merge keeps both changes; where both changed the same lines, they are marked.",
                    theme::dim(),
                )),
            ];
            f.render_widget(Paragraph::new(lines), Rect { height: 2, ..inner });
            let y = inner.y + inner.height.saturating_sub(1);
            let mut x = inner.x + 1;
            for (i, label) in [" Merge  m ", " Reload theirs  r ", " Keep mine  k ", " Cancel  Esc "].iter().enumerate()
            {
                let w = label.chars().count() as u16;
                if x + w > inner.x + inner.width || inner.height == 0 {
                    break;
                }
                let rr = Rect { x, y, width: w, height: 1 };
                let style = if i == 0 {
                    Style::default().fg(ratatui::style::Color::Black).bg(theme::ACCENT)
                } else {
                    Style::default().fg(theme::TEXT).bg(theme::FAINT)
                };
                f.render_widget(Paragraph::new(Span::styled(*label, style)), rr);
                app.hits.push((rr, Hit::ModalButton(i)));
                x += w + 2;
            }
        }
        Modal::Pick { title, items, cur } => {
            let h = (items.len() as u16 + 2).min(area.height.saturating_sub(4)).max(4);
            let r = centered(area, 80, h);
            f.render_widget(Clear, r);
            let b = block(&title, true);
            let inner = b.inner(r);
            f.render_widget(b, r);
            let top = cur.saturating_sub(inner.height.saturating_sub(1) as usize);
            for (i, (path, tag)) in items.iter().enumerate().skip(top).take(inner.height as usize) {
                let y = inner.y + (i - top) as u16;
                let rr = Rect { y, height: 1, ..inner };
                let line = Line::from(vec![
                    Span::styled(format!(" {path}"), theme::text()),
                    Span::styled(format!("  {tag}"), theme::faint()),
                ]);
                let line = if i == cur { line.style(theme::chosen()) } else { line };
                f.render_widget(Paragraph::new(line), rr);
                app.hits.push((rr, Hit::Row(i)));
            }
        }
        Modal::Message { title, lines } => {
            let r = centered(area, 90, lines.len() as u16 + 4);
            f.render_widget(Clear, r);
            let b = block(&title, true);
            let inner = b.inner(r);
            f.render_widget(b, r);
            let body: Vec<Line> =
                lines.iter().map(|l| Line::from(Span::styled(format!(" {l}"), theme::text()))).collect();
            f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), inner);
        }
        Modal::Help => {
            let rows: Vec<(&str, &str)> = vec![
                ("Tab", "between the sections and the list"),
                ("1 … 7", "a section: Projects, Notes, Plan, Files, Activity, Sync, Settings"),
                ("/", "search the memory"),
                ("Enter / double click", "open, choose"),
                ("Esc", "back"),
                ("q", "quit"),
                ("", ""),
                ("In the editor", ""),
                ("Ctrl+S", "save (when an agent changed the note meanwhile: merge, reload or keep yours)"),
                ("Ctrl+Z / Ctrl+Y", "undo / redo"),
                ("Ctrl+C / X / V", "copy / cut / paste (the whole line when nothing is selected)"),
                ("Shift+arrows, mouse", "select;  Ctrl+arrows: word by word;  Ctrl+A: all"),
                ("Ctrl+F, F3", "find, the next one"),
                ("Ctrl+D", "today's date"),
                ("Ctrl+T, Tab", "a table; Tab goes to the next cell and lines the columns up"),
                ("Ctrl+Enter (Alt+X)", "a to-do ticked or opened"),
                ("[[", "a link to another note: its name offered"),
                ("Ctrl+O", "keep a file with this note"),
                ("Ctrl+P", "read it formatted"),
            ];
            let r = centered(area, 96, rows.len() as u16 + 2);
            f.render_widget(Clear, r);
            let b = block("Keys", true);
            let inner = b.inner(r);
            f.render_widget(b, r);
            let lines: Vec<Line> = rows
                .into_iter()
                .map(|(k, v)| {
                    if v.is_empty() {
                        Line::from(Span::styled(format!(" {k}"), theme::accent().add_modifier(Modifier::BOLD)))
                    } else {
                        Line::from(vec![
                            Span::styled(format!(" {k:<22}"), theme::accent()),
                            Span::styled(v, theme::text()),
                        ])
                    }
                })
                .collect();
            f.render_widget(Paragraph::new(lines), inner);
        }
    }
}

/// A text cut to a width, with "…".
fn fit(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n <= w {
        return format!("{s}{}", " ".repeat(w - n));
    }
    let mut out: String = s.chars().take(w.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/ui/draw.rs"]
mod tests;
