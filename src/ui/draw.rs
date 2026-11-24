//! Drawing the interface: a header, the sections on the left (each with
//! its icon on a tile), the chosen one on the right with the tabs of the
//! notes open above it, keys and messages at the bottom, windows on top.
//! Each frame also keeps where things are, for the mouse (`App::hits`).

use super::app::{App, Focus, Hit, HomeRow, Modal, PlanRow, SECTIONS, Section, SyncRow, SyncView};
use super::data::{AgentState, SetupRow};
use super::markdown;
use super::theme::{self, mark};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap,
};

/// The spinner of work going on in the background.
const SPIN: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];

pub fn draw(f: &mut Frame, app: &mut App) {
    app.hits.clear();
    let area = f.area();
    let [header, body, footer] = rows3(area);
    draw_header(f, app, header);
    let side_w = if body.width >= 70 { 22 } else { 18.min(body.width / 3) };
    let [side, main] = {
        let c = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(side_w), Constraint::Min(10)])
            .split(body);
        [c[0], c[1]]
    };
    draw_sidebar(f, app, side);
    // work going on: a band at the top, moving while it runs
    let main = if app.jobs.is_empty() || main.height < 6 {
        main
    } else {
        draw_working(f, app, Rect { height: 1, ..main });
        Rect { y: main.y + 1, height: main.height - 1, ..main }
    };
    // the notes open: their tabs above what is shown
    let main = if app.tab_order.is_empty() || main.height < 4 {
        main
    } else {
        draw_tabs(f, app, Rect { height: 1, ..main });
        Rect { y: main.y + 1, height: main.height - 1, ..main }
    };
    if app.open.is_some() {
        draw_editor(f, app, main);
    } else {
        match app.section {
            Section::Home => draw_home(f, app, main),
            Section::Projects => draw_projects(f, app, main),
            Section::Notes => draw_notes(f, app, main),
            Section::Plan => draw_plan(f, app, main),
            Section::Files => draw_files(f, app, main),
            Section::Agents => draw_agents(f, app, main),
            Section::Activity => draw_activity(f, app, main),
            Section::Sync => draw_sync(f, app, main),
            Section::Setup => draw_setup(f, app, main),
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

fn block<'a>(title: &str, focused: bool) -> Block<'a> {
    let title = if focused {
        Span::styled(format!(" {title} "), theme::button_main())
    } else {
        Span::styled(format!(" {title} "), theme::heading())
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::border(focused))
        .title(title)
}

/// A pill: a key or a button in the header and footer.
fn pill<'a>(k: &str) -> Span<'a> {
    Span::styled(format!(" {k} "), theme::key())
}

fn draw_header(f: &mut Frame, app: &mut App, area: Rect) {
    let mut spans = vec![Span::styled(" ◆ PARAGMA ", theme::button_main()), Span::raw("  ")];
    if let Some(p) = &app.project {
        spans.push(Span::styled(p.name.clone(), theme::text().add_modifier(Modifier::BOLD)));
        spans.push(Span::styled("  ›  ", theme::accent()));
    }
    let label = SECTIONS.iter().find(|s| s.0 == app.section).map(|s| s.2).unwrap_or("");
    if let Some(o) = &app.open {
        spans.push(Span::styled(o.rel.trim_end_matches(".md").to_string(), theme::dim()));
    } else {
        spans.push(Span::styled(label, theme::dim()));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
    // on the right: the sync, work going on, the buttons
    let mut right: Vec<(Vec<Span>, Option<&str>)> = Vec::new();
    // work going on is shown as a band over the main area (draw_working)
    if let Some(chip) = sync_chip(app) {
        right.push((vec![chip, Span::raw("  ")], Some("sync")));
    }
    right.push((vec![pill("Ctrl+K"), Span::styled(" commands ", theme::dim())], Some("palette")));
    right.push((vec![pill("/"), Span::styled(" search ", theme::dim())], Some("search")));
    right.push((vec![pill("?"), Span::styled(" keys ", theme::dim())], Some("help")));
    let width = |s: &Vec<Span>| s.iter().map(|x| x.content.chars().count()).sum::<usize>() as u16;
    let total: u16 = right.iter().map(|(s, _)| width(s)).sum();
    let left_w: u16 = 40;
    if area.width < total + left_w {
        // a narrow window: the buttons only
        right.retain(|(_, b)| matches!(b, Some("palette" | "search")));
    }
    let total: u16 = right.iter().map(|(s, _)| width(s)).sum();
    if area.width <= total + 12 {
        return;
    }
    let mut x = area.x + area.width - total;
    for (spans, b) in right {
        let w = width(&spans);
        let r = Rect { x, y: area.y, width: w, height: 1 };
        f.render_widget(Paragraph::new(Line::from(spans)), r);
        if let Some(b) = b {
            app.hits.push((r, Hit::Button(b.into())));
        }
        x += w;
    }
}

/// What is going on in the background: its name, a bar that moves, and
/// how long it has been going.
fn draw_working(f: &mut Frame, app: &App, area: Rect) {
    let Some(j) = app.jobs.first() else { return };
    let ms = j.started.elapsed().as_millis() as usize;
    let secs = ms / 1000;
    let more = if app.jobs.len() > 1 { format!("  (+{} more)", app.jobs.len() - 1) } else { String::new() };
    let left = format!(" {} {}{more}  ", SPIN[ms / 100 % SPIN.len()], j.what);
    let right = format!("  {secs} s ");
    let w = (area.width as usize).saturating_sub(left.chars().count() + right.chars().count());
    // a lit stretch that runs along the bar and comes back
    let seg = (w / 5).clamp(3, 16);
    let span = w.saturating_sub(seg).max(1);
    let t = ms / 60 % (2 * span);
    let at = if t < span { t } else { 2 * span - t };
    let bar: String = (0..w).map(|i| if i >= at && i < at + seg { '━' } else { '─' }).collect();
    let (before, rest) = bar.split_at(bar.char_indices().nth(at).map(|(i, _)| i).unwrap_or(bar.len()));
    let (lit, after) =
        rest.split_at(rest.char_indices().nth(seg.min(rest.chars().count())).map(|(i, _)| i).unwrap_or(rest.len()));
    let line = Line::from(vec![
        Span::styled(left, theme::accent().add_modifier(Modifier::BOLD)),
        Span::styled(before.to_string(), Style::default().fg(theme::FRAME)),
        Span::styled(lit.to_string(), theme::accent().add_modifier(Modifier::BOLD)),
        Span::styled(after.to_string(), Style::default().fg(theme::FRAME)),
        Span::styled(right, theme::dim()),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

/// The sync, said in a few letters: synced, waiting to send, failed.
fn sync_chip<'a>(app: &App) -> Option<Span<'a>> {
    let _ = crate::sync::Config::load(&app.home)?;
    let s = crate::sync::Status::load(&app.home);
    Some(if !s.error.is_empty() || !app.conflicts.is_empty() {
        Span::styled(format!("{} {} sync needs you", mark::SYNC, mark::WARN), theme::bad())
    } else if app.unsent > 0 {
        Span::styled(format!("{} {} {} to send", mark::SYNC, mark::UNSENT, app.unsent), theme::warn())
    } else if s.synced > 0 {
        Span::styled(
            format!("{} synced {}", mark::SYNC, crate::output::ago(s.synced, crate::output::now())),
            theme::ok(),
        )
    } else {
        Span::styled(format!("{} not synced yet", mark::SYNC), theme::dim())
    })
}

fn draw_sidebar(f: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Sidebar && app.open.is_none() && app.modal.is_none();
    let b = block("Paragma", focused);
    let inner = b.inner(area);
    f.render_widget(b, area);
    // a line above the first and between the sections when there is room
    let n = SECTIONS.len();
    let gap = if inner.height as usize > n * 2 { 2 } else { 1 };
    let top = if inner.height as usize > n * gap { 1 } else { 0 };
    for (i, (s, icon, label)) in SECTIONS.iter().enumerate() {
        let y = inner.y + top + (i * gap) as u16;
        if y >= inner.y + inner.height {
            break;
        }
        let r = Rect { x: inner.x, y, width: inner.width, height: 1 };
        let chosen = *s == app.section && app.open.is_none();
        let hovered = app.hover.is_some_and(|(hx, hy)| hy == y && hx >= r.x && hx < r.x + r.width);
        let bg = if hovered && !chosen { Some(theme::HOVER_BG) } else { None };
        if let Some(bg) = bg {
            f.render_widget(Block::default().style(Style::default().bg(bg)), r);
        }
        // the names in capitals and bold: they read larger in a terminal
        let fg = if chosen { theme::accent() } else { theme::text() }.add_modifier(Modifier::BOLD);
        let label_w = (inner.width as usize).saturating_sub(9);
        let mut spans = vec![
            Span::styled(
                if chosen { format!(" {}", mark::BAR) } else { "  ".into() },
                theme::accent().add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" {icon} "), theme::tile(chosen)),
            Span::raw(" "),
            Span::styled(fit(&label.to_uppercase(), label_w), fg),
            Span::styled(format!("{}", i + 1), theme::faint()),
        ];
        if let Some(bg) = bg {
            spans = spans.into_iter().map(|sp| sp.patch_style(Style::default().bg(bg))).collect();
        }
        f.render_widget(Paragraph::new(Line::from(spans)), r);
        app.hits.push((r, Hit::Section(i)));
    }
}

/// The tabs of the notes open: the one shown lit, each with its ×.
fn draw_tabs(f: &mut Frame, app: &mut App, area: Rect) {
    let active = app.active_tab();
    let mut x = area.x;
    let end = area.x + area.width;
    let order = app.tab_order.clone();
    let here = app.project_name().unwrap_or_default();
    for (i, (project, rel)) in order.iter().enumerate() {
        let name = rel.trim_end_matches(".md").rsplit('/').next().unwrap_or(rel).to_string();
        let name = if *project != here { format!("{project}: {name}") } else { name };
        let dirty = if active == Some(i) {
            app.open.as_ref().is_some_and(|o| o.ed.dirty)
        } else {
            app.tabs.iter().any(|t| t.project == *project && t.rel == *rel && t.ed.dirty)
        };
        let label = format!(" {}{} ", fit(&name, 22).trim_end(), if dirty { " ●" } else { "" });
        let w = label.chars().count() as u16;
        if x + w + 3 > end {
            f.render_widget(
                Paragraph::new(Span::styled("…", theme::faint())),
                Rect { x, y: area.y, width: 1, height: 1 },
            );
            break;
        }
        let style = if active == Some(i) { theme::button_main() } else { theme::button() };
        let r = Rect { x, y: area.y, width: w, height: 1 };
        f.render_widget(Paragraph::new(Span::styled(label, style)), r);
        app.hits.push((r, Hit::Tab(i)));
        let c = Rect { x: x + w, y: area.y, width: 2, height: 1 };
        f.render_widget(Paragraph::new(Span::styled("× ", style)), c);
        app.hits.push((c, Hit::TabClose(i)));
        x += w + 3;
    }
}

/// A list of rows that scrolls with its chosen one; each row clickable,
/// the chosen one lit with › before it, the one under the mouse shaded, a
/// scrollbar when it is longer than the room.
fn draw_list(f: &mut Frame, app: &mut App, area: Rect, section: Section, lines: Vec<Line<'static>>, empty: &str) {
    let k = super::app::index(section);
    if lines.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled(format!(" {empty}"), theme::dim())).wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    if area.height == 0 || area.width == 0 {
        return;
    }
    let total = lines.len();
    let long = total > area.height as usize;
    let width = if long { area.width.saturating_sub(1) } else { area.width };
    let height = area.height as usize;
    let cur = app.cur[k];
    if cur < app.top[k] {
        app.top[k] = cur;
    }
    if cur >= app.top[k] + height {
        app.top[k] = cur + 1 - height;
    }
    let focused = app.focus == Focus::Main && app.modal.is_none() && app.open.is_none();
    let dragging = app.drag_over.is_some() && section == Section::Notes;
    for (row, line) in lines.into_iter().enumerate().skip(app.top[k]).take(height) {
        let y = area.y + (row - app.top[k]) as u16;
        let r = Rect { x: area.x, y, width, height: 1 };
        let hovered = app.hover.is_some_and(|(hx, hy)| hy == y && hx >= r.x && hx < r.x + r.width);
        let target = dragging && app.drag_over == Some(row);
        let (bg, style) = if target {
            (None, Some(theme::chosen().add_modifier(Modifier::UNDERLINED)))
        } else if row == cur && focused {
            (None, Some(theme::chosen()))
        } else if row == cur {
            (None, Some(theme::chosen_idle()))
        } else if hovered {
            (Some(theme::HOVER_BG), None)
        } else {
            (None, None)
        };
        if let Some(bg) = bg {
            f.render_widget(Block::default().style(Style::default().bg(bg)), r);
        }
        let bar = if row == cur {
            Span::styled(
                mark::BAR,
                if focused { theme::chosen() } else { theme::accent().add_modifier(Modifier::BOLD) },
            )
        } else {
            Span::raw(" ")
        };
        let mut spans = vec![bar];
        spans.extend(line.spans);
        let mut line = Line::from(spans);
        if let Some(st) = style {
            // the chosen row in one colour over its band, so it reads at once
            line = Line::from(line.spans.into_iter().map(|sp| sp.style(st)).collect::<Vec<_>>());
        } else if let Some(bg) = bg {
            line = line.patch_style(Style::default().bg(bg));
        }
        f.render_widget(Paragraph::new(line), r);
        app.hits.push((r, Hit::Row(row)));
    }
    if long && area.height > 1 && area.width > 1 {
        let mut state = ScrollbarState::new(total.saturating_sub(height) + 1).position(app.top[k]);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(theme::accent())
                .track_style(Style::default().fg(theme::FRAME)),
            area,
            &mut state,
        );
    }
}

/// "Notes  3/12": where the chosen row is.
fn titled(name: &str, app: &App, section: Section) -> String {
    let n = app.rows(section);
    let k = super::app::index(section);
    if n == 0 { name.to_string() } else { format!("{name}  {}/{n}", app.cur[k] + 1) }
}

fn draw_home(f: &mut Frame, app: &mut App, area: Rect) {
    let title = match &app.project {
        Some(p) => format!("Home  ·  {}", p.name),
        None => "Home".into(),
    };
    let b = block(&title, app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let lines: Vec<Line<'static>> = app
        .home_list
        .iter()
        .map(|r| match r {
            HomeRow::Title(t) => Line::from(Span::styled(t.clone(), theme::heading())),
            HomeRow::Line(t) => Line::from(Span::styled(format!("  {t}"), theme::faint())),
            HomeRow::Blank => Line::from(""),
            HomeRow::Goal(g) => Line::from(vec![
                Span::styled("Goal  ", theme::heading()),
                Span::styled(
                    if g.is_empty() { "(none yet: Enter writes it)".into() } else { g.clone() },
                    theme::text(),
                ),
            ]),
            HomeRow::Step(n, t, by) => Line::from(vec![
                Span::styled(format!("  {} ", mark::TODO), theme::accent()),
                Span::styled(format!("{n:>3}. "), theme::faint()),
                Span::styled(t.clone(), theme::text()),
                Span::styled(if by.is_empty() { String::new() } else { format!("   {by}") }, theme::warn()),
            ]),
            HomeRow::Due(rel, d, left) => Line::from(vec![
                Span::styled(format!("  {} ", mark::NOTE), theme::faint()),
                Span::styled(rel.clone(), theme::text()),
                Span::styled(
                    format!("   due {d} ({})", crate::project::items::when(*left)),
                    if *left < 0 { theme::bad() } else { theme::warn() },
                ),
            ]),
            HomeRow::Recent(rel) => Line::from(vec![
                Span::styled(format!("  {} ", mark::NOTES), theme::accent()),
                Span::styled(rel.trim_end_matches(".md").to_string(), theme::text()),
            ]),
            HomeRow::Agent(who, doing) => Line::from(vec![
                Span::styled(format!("  {} ", mark::AGENTS), theme::ok()),
                Span::styled(format!("{who}  "), theme::text().add_modifier(Modifier::BOLD)),
                Span::styled(doing.clone(), theme::dim()),
            ]),
            HomeRow::Sync(t, bad) => Line::from(vec![
                Span::styled(format!("  {} ", mark::SYNC), theme::accent()),
                Span::styled(t.clone(), if *bad { theme::bad() } else { theme::dim() }),
            ]),
            HomeRow::Handoff(h, t) => Line::from(vec![
                Span::styled(format!("  {} ", mark::HANDOFF), theme::faint()),
                Span::styled(format!("{h}  "), theme::dim()),
                Span::styled(crate::output::one_line(t, 160), theme::text()),
            ]),
            HomeRow::Projects => Line::from(vec![
                Span::styled(format!("  {} ", mark::PROJECTS), theme::accent()),
                Span::styled("Choose a project", theme::text().add_modifier(Modifier::BOLD)),
            ]),
        })
        .collect();
    draw_list(f, app, inner, Section::Home, lines, "");
}

fn draw_projects(f: &mut Frame, app: &mut App, area: Rect) {
    let t = titled("Projects", app, Section::Projects);
    let b = block(&t, app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let current = app.project.as_ref().map(|p| p.name.clone()).unwrap_or_default();
    let lines: Vec<Line<'static>> = app
        .projects
        .iter()
        .map(|p| {
            let m = if p.name == current { format!("{} ", mark::CURRENT) } else { "  ".to_string() };
            Line::from(vec![
                Span::styled(m, theme::accent()),
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
    f.render_widget(Paragraph::new(Span::styled("Choose a project first (2: Projects).", theme::dim())), inner);
}

fn draw_notes(f: &mut Frame, app: &mut App, area: Rect) {
    if app.project.is_none() {
        return no_project(f, area, "Notes");
    }
    let c = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);
    let t = titled("Notes", app, Section::Notes);
    let b = block(&t, app.focus == Focus::Main);
    let inner = b.inner(c[0]);
    f.render_widget(b, c[0]);
    let lines: Vec<Line<'static>> = app
        .tree
        .iter()
        .map(|r| {
            let pad = "  ".repeat(r.depth);
            let marks = |r: &super::data::TreeRow| -> Vec<Span<'static>> {
                let mut out = Vec::new();
                if r.conflict {
                    out.push(Span::styled(format!("  {} two computers", mark::WARN), theme::bad()));
                } else if r.unsent {
                    out.push(Span::styled(format!("  {}", mark::UNSENT), theme::warn()));
                }
                out
            };
            if r.folder {
                let open = app.expanded.contains(&r.rel);
                let mut spans = vec![
                    Span::raw(pad),
                    Span::styled(format!("{} ", if open { mark::FOLDER_OPEN } else { mark::FOLDER }), theme::accent()),
                    Span::styled(r.title.clone(), theme::text().add_modifier(Modifier::BOLD)),
                    Span::styled(format!("  {}", r.count), theme::faint()),
                ];
                spans.extend(marks(r));
                Line::from(spans)
            } else {
                let mut spans = vec![
                    Span::styled(format!("{pad}{} ", mark::NOTE), theme::accent()),
                    Span::styled(r.title.clone(), theme::text()),
                    Span::styled(format!("  {}", r.tag), theme::faint()),
                ];
                spans.extend(marks(r));
                Line::from(spans)
            }
        })
        .collect();
    draw_list(f, app, inner, Section::Notes, lines, "No notes yet. Press n for a new note, N for a folder.");
    // the chosen note, as it reads
    let pb = block("Preview  (wheel scrolls it)", false);
    let pin = pb.inner(c[1]);
    f.render_widget(pb, c[1]);
    let Some(row) = app.tree.get(app.cur()).cloned() else { return };
    if row.folder {
        let lines = vec![
            Line::from(Span::styled(
                format!("{} in {}", crate::output::count(row.count as i64, "note"), row.rel),
                theme::dim(),
            )),
            Line::from(""),
            Line::from(Span::styled("Enter opens the folder; n makes a note in it.", theme::faint())),
            Line::from(Span::styled("Drag a note onto it with the mouse to move it here.", theme::faint())),
        ];
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), pin);
        return;
    }
    let Some(p) = app.project.clone() else { return };
    let text = crate::project::files::read_text(&p.dir.join(&row.rel)).unwrap_or_default();
    let note = crate::note::Note::parse(&text);
    let mut lines = vec![Line::from(vec![
        Span::styled(row.title.clone(), theme::heading()),
        Span::styled(format!("   {}", row.tag), theme::faint()),
    ])];
    if row.conflict {
        lines.push(Line::from(Span::styled(
            "The other computer's version of a note both changed: Enter chooses what to keep.",
            theme::bad(),
        )));
    }
    if let Some(d) = note.props.text("due") {
        let when = crate::project::items::days_left(d).map(crate::project::items::when).unwrap_or_default();
        lines.push(Line::from(Span::styled(format!("due {d} ({when})"), theme::warn())));
    }
    lines.push(Line::from(""));
    lines.extend(markdown::render(&note.body));
    // what links here, and where it links
    // read again when another note is shown or the notes changed
    if app.links_for.as_ref().is_none_or(|(r, _, _)| *r != row.rel) {
        let (from, to) = super::data::links_of(&p, &mut app.cache, &row.rel);
        app.links_for = Some((row.rel.clone(), from, to));
    }
    let (from, to) = app.links_for.as_ref().map(|(_, a, b)| (a.clone(), b.clone())).unwrap_or_default();
    if !from.is_empty() || !to.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "─".repeat(pin.width.saturating_sub(1) as usize),
            Style::default().fg(theme::FRAME),
        )));
        if !from.is_empty() {
            let mut s = vec![Span::styled("Linked from  ", theme::heading())];
            for (i, n) in from.iter().take(12).enumerate() {
                if i > 0 {
                    s.push(Span::styled(" · ", theme::faint()));
                }
                s.push(Span::styled(n.clone(), theme::link()));
            }
            lines.push(Line::from(s));
        }
        if !to.is_empty() {
            let mut s = vec![Span::styled("Links to     ", theme::heading())];
            for (i, n) in to.iter().take(12).enumerate() {
                if i > 0 {
                    s.push(Span::styled(" · ", theme::faint()));
                }
                s.push(Span::styled(n.clone(), theme::link()));
            }
            lines.push(Line::from(s));
        }
    }
    if app.preview_of != row.rel {
        app.preview_of = row.rel.clone();
        app.preview_scroll = 0;
    }
    let total = lines.len();
    app.preview_scroll = app.preview_scroll.min(total.saturating_sub(1));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }).scroll((app.preview_scroll as u16, 0)), pin);
    app.hits.push((pin, Hit::Preview));
    if total > pin.height as usize && c[1].height > 2 {
        let mut state = ScrollbarState::new(total).position(app.preview_scroll);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .begin_symbol(None)
                .end_symbol(None)
                .thumb_style(theme::accent())
                .track_style(Style::default().fg(theme::FRAME)),
            c[1],
            &mut state,
        );
    }
}

fn draw_plan(f: &mut Frame, app: &mut App, area: Rect) {
    if app.project.is_none() {
        return no_project(f, area, "Plan");
    }
    let t = titled("Plan", app, Section::Plan);
    let b = block(&t, app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let claims: std::collections::HashMap<usize, String> =
        app.agents.iter().flat_map(|a| a.steps.iter().map(move |s| (*s, a.who.clone()))).collect();
    let lines: Vec<Line<'static>> = app
        .plan_rows()
        .into_iter()
        .map(|r| match r {
            PlanRow::Goal(g) => Line::from(vec![
                Span::styled("Goal  ", theme::heading()),
                Span::styled(if g.is_empty() { "(none yet)".into() } else { g }, theme::text()),
            ]),
            PlanRow::Step(n, s) => {
                let (m, style) = if s.done {
                    (mark::DONE, theme::dim().add_modifier(Modifier::CROSSED_OUT))
                } else if s.deferred {
                    (mark::LATER, theme::dim())
                } else {
                    (mark::TODO, theme::text())
                };
                Line::from(vec![
                    Span::styled(format!("  {m} "), if s.done { theme::ok() } else { theme::accent() }),
                    Span::styled(format!("{n:>3}. "), theme::faint()),
                    Span::styled(s.text.clone(), style),
                    Span::styled(if s.deferred { "  (later)".to_string() } else { String::new() }, theme::faint()),
                    Span::styled(
                        claims.get(&n).filter(|_| !s.done).map(|w| format!("   taken by {w}")).unwrap_or_default(),
                        theme::warn(),
                    ),
                ])
            }
            PlanRow::Question(n, q) => Line::from(vec![
                Span::styled(
                    if q.answer.is_some() { format!("  {} ", mark::DONE) } else { format!("  {} ", mark::QUESTION) },
                    theme::warn(),
                ),
                Span::styled(format!("{n}. {}", q.text), theme::text()),
                Span::styled(q.answer.map(|a| format!("  → {a}")).unwrap_or_default(), theme::dim()),
            ]),
            PlanRow::Handoff(h) => Line::from(vec![
                Span::styled(format!("  {} ", mark::HANDOFF), theme::faint()),
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
    let t = titled("Files kept with notes", app, Section::Files);
    let b = block(&t, app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let lines: Vec<Line<'static>> = app
        .files
        .iter()
        .map(|r| {
            Line::from(vec![
                Span::styled(format!("{} ", mark::FILES), theme::accent()),
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
        "No files kept with notes. Press a, or drop a file on the window (a workbook, a contract, a deck...).",
    );
}

fn draw_agents(f: &mut Frame, app: &mut App, area: Rect) {
    if app.project.is_none() {
        return no_project(f, area, "Agents");
    }
    let t = titled("Agents: the conversations working here", app, Section::Agents);
    let b = block(&t, app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let now = crate::output::now();
    let mut lines: Vec<Line<'static>> = vec![Line::from(vec![
        Span::styled("✎ ", theme::accent()),
        Span::styled("Tell every conversation here…", theme::text().add_modifier(Modifier::BOLD)),
        Span::styled("   they see it at their next pg command", theme::faint()),
    ])];
    for a in &app.agents {
        let (m, st, label) = match a.state {
            AgentState::AtWork => (mark::ON, theme::ok(), "at work"),
            AgentState::Stopped => (mark::WARN, theme::warn(), "stopped, no handoff"),
            AgentState::HandedOff => (mark::DONE, theme::dim(), "handed off"),
            AgentState::Quiet => (mark::OFF, theme::faint(), "looked only"),
        };
        let steps = if a.steps.is_empty() {
            String::new()
        } else {
            format!("   step {}", a.steps.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(", "))
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{m} "), st),
            Span::styled(format!("{:<22}", fit(&a.who, 22)), theme::text().add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:<20}", label), st),
            Span::styled(format!("{:>12}  ", crate::output::ago(a.last, now)), theme::faint()),
            Span::styled(a.doing.clone(), theme::dim()),
            Span::styled(steps, theme::warn()),
        ]));
    }
    if app.agents.is_empty() {
        lines.push(Line::from(Span::styled("  No conversation worked here in the last two days.", theme::faint())));
    }
    draw_list(f, app, inner, Section::Agents, lines, "");
}

fn draw_activity(f: &mut Frame, app: &mut App, area: Rect) {
    if app.project.is_none() {
        return no_project(f, area, "Activity");
    }
    let t = titled("Activity: what agents and pg did here", app, Section::Activity);
    let b = block(&t, app.focus == Focus::Main);
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

fn on_off(on: bool) -> Span<'static> {
    if on {
        Span::styled(format!("{} on ", mark::ON), theme::ok())
    } else {
        Span::styled(format!("{} off", mark::OFF), theme::faint())
    }
}

fn draw_sync(f: &mut Frame, app: &mut App, area: Rect) {
    let repos = matches!(app.sync_view, SyncView::Repos { .. });
    let title = match &app.sync_view {
        SyncView::Repos { moving: true, .. } => "Sync  ›  Move to GitHub: choose a repository",
        SyncView::Repos { .. } => "Sync  ›  GitHub: choose a repository",
        SyncView::Main => "Sync and backup",
    };
    let b = block(title, app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let mut head: Vec<Line<'static>> = Vec::new();
    if let SyncView::Repos { login, filter, repos, .. } = &app.sync_view {
        head.push(Line::from(vec![
            Span::styled("signed in as  ", theme::faint()),
            Span::styled(login.clone(), theme::text().add_modifier(Modifier::BOLD)),
            Span::styled(format!("   {} repositories you can write to", repos.len()), theme::faint()),
        ]));
        head.push(Line::from(vec![
            Span::styled("type to narrow  ", theme::faint()),
            Span::styled(format!("› {filter}"), theme::accent().add_modifier(Modifier::BOLD)),
            Span::styled("▏", theme::accent()),
            Span::styled("     Esc back", theme::faint()),
        ]));
        head.push(Line::from(""));
    }
    let h = head.len() as u16;
    f.render_widget(Paragraph::new(head), Rect { height: h.min(inner.height), ..inner });
    let list = Rect { y: inner.y + h.min(inner.height), height: inner.height.saturating_sub(h), ..inner };
    let bold = theme::text().add_modifier(Modifier::BOLD);
    let row = |name: &str, rest: Vec<Span<'static>>| -> Line<'static> {
        let mut spans = vec![Span::styled(format!("  {name:<30}"), bold)];
        spans.extend(rest);
        Line::from(spans)
    };
    let lines: Vec<Line<'static>> = app
        .sync_rows()
        .into_iter()
        .map(|r| match r {
            SyncRow::Head(t) => Line::from(Span::styled(t, theme::heading())),
            SyncRow::Info(t, bad) => {
                Line::from(Span::styled(format!("  {t}"), if bad { theme::bad() } else { theme::dim() }))
            }
            SyncRow::SyncNow => {
                row("Sync now", vec![Span::styled("send what changed here, bring in the others'", theme::faint())])
            }
            SyncRow::Connect => row(
                "Connect",
                vec![Span::styled(
                    "GitHub, another git host, a cloud folder (OneDrive, Google Drive…), WebDAV",
                    theme::faint(),
                )],
            ),
            SyncRow::Transfer => row(
                "Move to another place",
                vec![Span::styled("any kind; copied first, checked, then the old place left", theme::faint())],
            ),
            SyncRow::Disconnect => {
                row("Disconnect", vec![Span::styled("stop syncing on this computer", theme::faint())])
            }
            SyncRow::Env(on) => row(
                ".env files (API keys)",
                vec![on_off(on), Span::styled("   never to a public repository", theme::faint())],
            ),
            SyncRow::Files(on) => row("Files kept with notes", vec![on_off(on)]),
            SyncRow::Limit(mb) => row(
                "File size limit",
                vec![
                    Span::styled(format!("{mb} MB a file"), theme::accent()),
                    Span::styled("   Enter: the next size", theme::faint()),
                ],
            ),
            SyncRow::BackupNow => row(
                "Back up now",
                vec![Span::styled("the whole memory in one zip, in a folder you choose", theme::faint())],
            ),
            SyncRow::Daily(on, folder) => row(
                "Every day",
                vec![
                    on_off(on),
                    Span::styled(
                        if on {
                            format!("   into {folder}, the last 7 kept")
                        } else {
                            "   a backup a day, by itself".into()
                        },
                        theme::faint(),
                    ),
                ],
            ),
            SyncRow::Restore => row(
                "Restore a backup",
                vec![Span::styled("the backups on this computer, or a zip you name", theme::faint())],
            ),
            SyncRow::Conflict(rel) => Line::from(vec![
                Span::styled(format!("  {} ", mark::WARN), theme::bad()),
                Span::styled(format!("{:<28}", data_name(&rel)), bold),
                Span::styled("Enter chooses what to keep", theme::bad()),
            ]),
            SyncRow::NewRepo => Line::from(vec![
                Span::styled("  + ", theme::accent().add_modifier(Modifier::BOLD)),
                Span::styled("A new private repository", bold),
                Span::styled("   paragma-memory, or a name you choose", theme::faint()),
            ]),
            SyncRow::Repo(r) => Line::from(vec![
                Span::styled(format!("  {} ", if r.own { mark::PROJECTS } else { mark::FILES }), theme::accent()),
                Span::styled(format!("{:<44}", fit(&r.full_name, 44)), theme::text()),
                Span::styled(
                    if r.private { "private" } else { "public" },
                    if r.private { theme::ok() } else { theme::bad() },
                ),
            ]),
        })
        .collect();
    let empty = if repos { "No repository matches what is typed." } else { "" };
    draw_list(f, app, list, Section::Sync, lines, empty);
}

/// A conflict copy's note, for people.
fn data_name(rel: &str) -> String {
    super::data::conflict_of(rel).unwrap_or_else(|| rel.to_string()).trim_end_matches(".md").to_string()
}

fn draw_setup(f: &mut Frame, app: &mut App, area: Rect) {
    let b = block("Setup: Paragma in your coding agents", app.focus == Focus::Main);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let bold = theme::text().add_modifier(Modifier::BOLD);
    let lines: Vec<Line<'static>> = app
        .setup
        .iter()
        .map(|r| match r {
            SetupRow::UpdateAll => Line::from(vec![
                Span::styled(format!(" {} ", mark::SETUP), theme::tile(false)),
                Span::styled("  Update everything to this version", bold),
                Span::styled(
                    "   the instructions, hooks and MCP servers pg set up; your own lines stay",
                    theme::faint(),
                ),
            ]),
            SetupRow::Head(t) => Line::from(Span::styled(*t, theme::heading())),
            SetupRow::Agent { name, found, installed, instructions, hooks, place, .. } => {
                let seen = if found.is_empty() {
                    if *installed { "found".to_string() } else { "not found".to_string() }
                } else {
                    found.join(", ")
                };
                let mut spans = vec![
                    Span::styled(format!("  {:<14}", name), bold),
                    Span::styled(
                        format!("{:<14}", fit(&seen, 13)),
                        if *installed { theme::dim() } else { theme::faint() },
                    ),
                    Span::styled("instructions ", theme::faint()),
                    on_off(*instructions),
                    Span::styled("   hooks ", theme::faint()),
                ];
                spans.push(match hooks {
                    Some(on) => on_off(*on),
                    None => Span::styled(" —  ", theme::faint()),
                });
                spans.push(Span::styled(format!("   {place}"), theme::faint()));
                Line::from(spans)
            }
            SetupRow::Mcp { name, on, .. } => Line::from(vec![
                Span::styled(format!("  {:<28}", name), bold),
                Span::styled("MCP server ", theme::faint()),
                on_off(*on),
            ]),
            SetupRow::Doctor => Line::from(vec![
                Span::styled(format!("  {:<28}", "Check this project"), bold),
                Span::styled("pg doctor: notes, code and indexes", theme::faint()),
            ]),
        })
        .collect();
    let list = Rect { height: inner.height.saturating_sub(2), ..inner };
    draw_list(f, app, list, Section::Setup, lines, "");
    if inner.height > 3 {
        let about = Line::from(vec![
            Span::styled(format!("Paragma {}", env!("CARGO_PKG_VERSION")), theme::accent()),
            Span::styled(format!("   memory  {}", app.home.root.display()), theme::faint()),
        ]);
        let y = inner.y + inner.height - 1;
        f.render_widget(Paragraph::new(about), Rect { y, height: 1, ..inner });
    }
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
    f.render_widget(Paragraph::new(Span::styled(text, theme::button())), r);
    app.hits.push((r, Hit::Button(name.into())));
    *x += w + 1;
}

fn draw_editor(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(o) = app.open.as_ref() else { return };
    let title = format!(
        "{} {}{}{}{}",
        if o.reading { "Reading" } else { "Editing" },
        o.rel.trim_end_matches(".md"),
        if o.ed.dirty { "  ● unsaved" } else { "" },
        if o.outside { format!("  {} changed outside", mark::WARN) } else { String::new() },
        if o.resolving.is_some() { "  · merging the other computer's version" } else { "" }
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
        button(f, app, &mut x, y, end, "Edit  e", "edit");
        button(f, app, &mut x, y, end, "Close  Esc", "close");
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
    // line numbers beside the text when writing, as wide as the last one
    let gutter = if reading || inner.width < 30 { 0 } else { (o.ed.lines.len().to_string().len() + 2) as u16 };
    let gutter_area = Rect {
        x: inner.x,
        y: inner.y + 2,
        width: gutter,
        height: inner.height.saturating_sub(if finding { 3 } else { 2 }),
    };
    let text_area = Rect {
        x: inner.x + gutter,
        y: inner.y + 2,
        width: inner.width.saturating_sub(gutter),
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
    let cur_row = o.ed.cur.row;
    for (vi, (r, a, b)) in rows.iter().enumerate().skip(o.ed.scroll).take(height) {
        let y = text_area.y + (vi - o.ed.scroll) as u16;
        // the line the cursor is on, shaded; its number lit
        if *r == cur_row {
            f.render_widget(
                Block::default().style(Style::default().bg(theme::HOVER_BG)),
                Rect { x: gutter_area.x, y, width: gutter + text_area.width, height: 1 },
            );
        }
        if gutter > 0 {
            let first_of_line = *a == 0;
            let n = if first_of_line {
                format!("{:>w$} ", r + 1, w = gutter as usize - 2)
            } else {
                " ".repeat(gutter as usize - 1)
            };
            let style = if *r == cur_row { theme::accent().bg(theme::HOVER_BG) } else { theme::faint() };
            f.render_widget(
                Paragraph::new(Span::styled(n, style)),
                Rect { x: gutter_area.x, y, width: gutter, height: 1 },
            );
        }
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
        let mut line = Line::from(spans);
        if *r == cur_row {
            line = line.patch_style(Style::default().bg(theme::HOVER_BG));
        }
        f.render_widget(Paragraph::new(line), Rect { x: text_area.x, y, width: text_area.width, height: 1 });
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
            Span::styled(" Find ", theme::button_main()),
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
        out.push(pill(k));
        out.push(Span::styled(format!(" {what}  "), theme::dim()));
    }
    out
}

fn draw_footer(f: &mut Frame, app: &mut App, area: Rect) {
    if let (Some(from), Some(over)) = (app.drag_from, app.drag_over)
        && let (Some(a), Some(b)) = (app.tree.get(from), app.tree.get(over))
    {
        let to = if b.folder { b.rel.clone() } else { super::app::parent(&b.rel) };
        let to = if to.is_empty() { "the top".to_string() } else { to };
        let line = Line::from(vec![
            Span::styled(" moving ", theme::button_main()),
            Span::styled(format!(" {} → {to}   (let go to move it)", a.rel.trim_end_matches(".md")), theme::accent()),
        ]);
        f.render_widget(Paragraph::new(line), area);
        return;
    }
    if let Some((text, bad, at)) = &app.status
        && at.elapsed().as_secs() < 8
    {
        let style = if *bad { theme::bad() } else { theme::ok() };
        f.render_widget(Paragraph::new(Span::styled(format!(" {text}"), style)), area);
        return;
    }
    let hint: Vec<(&str, &str)> = if app.open.as_ref().is_some_and(|o| o.reading) {
        vec![("e", "edit"), ("↑↓", "scroll"), ("Ctrl+PgDn", "next tab"), ("Esc", "close")]
    } else if app.open.is_some() {
        vec![
            ("^S", "save"),
            ("^Z", "undo"),
            ("^C ^X ^V", "copy cut paste"),
            ("Tab", "next cell"),
            ("[[", "link"),
            ("^K", "commands"),
            ("Esc", "close"),
        ]
    } else {
        match app.section {
            Section::Home => {
                vec![("Enter", "open"), ("n", "new note"), ("Ctrl+K", "commands"), ("/", "search"), ("q", "quit")]
            }
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
                ("drag", "move"),
            ],
            Section::Plan => vec![
                ("Space", "done / open"),
                ("a", "add step"),
                ("e", "edit"),
                ("l", "later"),
                ("d", "drop"),
                ("Enter", "answer a question"),
            ],
            Section::Files => {
                vec![("Enter", "open"), ("a", "keep a file"), ("g", "go to its note"), ("drop", "a file")]
            }
            Section::Agents => vec![("Enter", "what it did"), ("t", "tell them all"), ("F5", "refresh")],
            Section::Activity => vec![("u", "undo what pg did"), ("F5", "refresh")],
            Section::Sync if matches!(app.sync_view, SyncView::Repos { .. }) => {
                vec![("type", "narrow"), ("Enter", "choose"), ("Esc", "back")]
            }
            Section::Sync => vec![("Enter", "do it")],
            Section::Setup => vec![("Enter", "set up / update"), ("h", "hooks on/off"), ("m", "MCP on/off")],
        }
    };
    f.render_widget(Paragraph::new(Line::from(keys(&hint))), area);
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width.saturating_sub(4));
    let h = h.min(area.height.saturating_sub(2));
    Rect { x: area.x + (area.width - w) / 2, y: area.y + (area.height - h) / 2, width: w, height: h }
}

/// The buttons of a window, on its last line; the first one lit.
fn buttons(f: &mut Frame, app: &mut App, inner: Rect, labels: &[String], cur: usize) {
    if inner.height == 0 {
        return;
    }
    let y = inner.y + inner.height - 1;
    let mut x = inner.x + 1;
    for (i, label) in labels.iter().enumerate() {
        let w = label.chars().count() as u16;
        if x + w > inner.x + inner.width {
            break;
        }
        let rr = Rect { x, y, width: w, height: 1 };
        let style = if i == cur { theme::button_main() } else { theme::button() };
        f.render_widget(Paragraph::new(Span::styled(label.clone(), style)), rr);
        app.hits.push((rr, Hit::ModalButton(i)));
        x += w + 2;
    }
}

fn draw_modal(f: &mut Frame, app: &mut App, area: Rect) {
    let Some(m) = app.modal.clone() else { return };
    match m {
        Modal::Input { title, text, ask } => {
            let text = if ask == super::app::Ask::DavPassword { "•".repeat(text.chars().count()) } else { text };
            let r = centered(area, 76, 5);
            f.render_widget(Clear, r);
            let b = block(&title, true);
            let inner = b.inner(r);
            f.render_widget(b, r);
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(format!(" {} ", mark::BAR), theme::accent().add_modifier(Modifier::BOLD)),
                    Span::styled(text.clone(), theme::text()),
                ])),
                Rect { height: 1, ..inner },
            );
            f.render_widget(
                Paragraph::new(Span::styled(" Enter ok · Esc cancel · Ctrl+V paste", theme::faint())),
                Rect { y: inner.y + 2.min(inner.height.saturating_sub(1)), height: 1, ..inner },
            );
            f.set_cursor_position((inner.x + 3 + text.chars().count() as u16, inner.y));
        }
        Modal::Confirm { title, lines, .. } => {
            let r = centered(area, 76, lines.len() as u16 + 5);
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
            buttons(f, app, inner, &[" Yes  y ".to_string(), " No  n ".to_string()], 0);
        }
        Modal::Choose { title, lines, options, cur } => {
            let h = lines.len() as u16 + options.len() as u16 + 4;
            let r = centered(area, 80, h);
            f.render_widget(Clear, r);
            let b = block(&title, true);
            let inner = b.inner(r);
            f.render_widget(b, r);
            let body: Vec<Line> =
                lines.iter().map(|l| Line::from(Span::styled(format!(" {l}"), theme::dim()))).collect();
            let bh = (lines.len() as u16).min(inner.height);
            f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), Rect { height: bh, ..inner });
            // the ways to go on, one a line, numbered
            for (i, (label, _)) in options.iter().enumerate() {
                let y = inner.y + bh + 1 + i as u16;
                if y >= inner.y + inner.height {
                    break;
                }
                let rr = Rect { x: inner.x + 1, y, width: inner.width.saturating_sub(2), height: 1 };
                let text = format!(" {} {}  {label} ", if i == cur { mark::BAR } else { " " }, i + 1);
                let style = if i == cur { theme::chosen() } else { theme::text() };
                f.render_widget(Paragraph::new(Span::styled(text, style)), rr);
                app.hits.push((rr, Hit::ModalButton(i)));
            }
        }
        Modal::Folders(p) => folders(f, app, area, &p),
        Modal::Changed { .. } => {
            let r = centered(area, 80, 8);
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
            f.render_widget(Paragraph::new(lines), Rect { height: 2.min(inner.height), ..inner });
            let labels: Vec<String> = [" Merge  m ", " Reload theirs  r ", " Keep mine  k ", " Cancel  Esc "]
                .iter()
                .map(|s| s.to_string())
                .collect();
            buttons(f, app, inner, &labels, 0);
        }
        Modal::Palette { text, items, cur } => {
            let rows: Vec<(String, String, bool)> = items
                .iter()
                .map(|i| (i.label.clone(), i.hint.clone(), matches!(i.act, super::app::PAct::Note(_))))
                .collect();
            finder(f, app, area, "Commands and notes", &text, &rows, cur, "↑↓ choose · Enter do it · Esc close");
        }
        Modal::Search { text, results, cur, note } => {
            let rows: Vec<(String, String, bool)> = results
                .iter()
                .map(|r| {
                    let head = if r.heading.is_empty() { String::new() } else { format!(" › {}", r.heading) };
                    (format!("{}{head}", r.path), format!("{}  {}", r.tag, r.snippet), true)
                })
                .collect();
            finder(f, app, area, "Search the memory", &text, &rows, cur, &note);
        }
        Modal::Message { title, lines } => {
            let r = centered(area, 96, lines.len() as u16 + 4);
            f.render_widget(Clear, r);
            let b = block(&title, true);
            let inner = b.inner(r);
            f.render_widget(b, r);
            let mut body: Vec<Line> =
                lines.iter().map(|l| Line::from(Span::styled(format!(" {l}"), theme::text()))).collect();
            body.push(Line::from(""));
            body.push(Line::from(Span::styled(" any key closes", theme::faint())));
            f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), inner);
        }
        Modal::Help => {
            let rows: Vec<(&str, &str)> = vec![
                ("Ctrl+K", "commands and notes: type a part of the name"),
                ("/", "search the memory as you type"),
                ("1 … 9", "a section: Home, Projects, Notes, Plan, Files, Agents, Activity, Sync, Setup"),
                ("Tab", "between the sections and the list"),
                ("Enter / double click", "open, choose"),
                ("Esc", "back;  q quits"),
                ("drag, drop", "a note onto a folder moves it; a file dropped on the window is kept with a note"),
                ("", ""),
                ("Notes open", ""),
                ("Ctrl+PgDn / PgUp", "the next / last tab;  Ctrl+W closes one;  a section shown keeps them open"),
                ("Ctrl+S", "save (when an agent changed the note meanwhile: merge, reload or keep yours)"),
                ("Ctrl+Z / Ctrl+Y", "undo / redo"),
                ("Ctrl+C / X / V", "copy / cut / paste (the whole line when nothing is selected)"),
                ("Shift+arrows, mouse", "select;  Ctrl+arrows: word by word;  Ctrl+A: all"),
                ("Ctrl+F, F3", "find, the next one"),
                ("Ctrl+D, Ctrl+T", "today's date;  a table (Tab goes to the next cell)"),
                ("Ctrl+Enter (Alt+X)", "a to-do ticked or opened"),
                ("[[", "a link to another note: its name offered"),
                ("Ctrl+O, Ctrl+P", "keep a file with this note;  read it formatted"),
            ];
            let r = centered(area, 100, rows.len() as u16 + 2);
            f.render_widget(Clear, r);
            let b = block("Keys", true);
            let inner = b.inner(r);
            f.render_widget(b, r);
            let lines: Vec<Line> = rows
                .into_iter()
                .map(|(k, v)| {
                    if v.is_empty() {
                        Line::from(Span::styled(format!(" {k}"), theme::heading()))
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

/// A window with a line to type in and what matches below it.
#[allow(clippy::too_many_arguments)]
fn finder(
    f: &mut Frame,
    app: &mut App,
    area: Rect,
    title: &str,
    text: &str,
    rows: &[(String, String, bool)],
    cur: usize,
    note: &str,
) {
    // as tall as what is found, between 8 rows and the window's height
    let h = (rows.len() as u16 + 5).max(8).min(area.height.saturating_sub(4).max(3));
    let r = centered(area, 100, h);
    f.render_widget(Clear, r);
    let b = block(title, true);
    let inner = b.inner(r);
    f.render_widget(b, r);
    if inner.height == 0 {
        return;
    }
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(format!(" {} ", mark::BAR), theme::accent().add_modifier(Modifier::BOLD)),
            Span::styled(text.to_string(), theme::text().add_modifier(Modifier::BOLD)),
        ])),
        Rect { height: 1, ..inner },
    );
    f.set_cursor_position((inner.x + 3 + text.chars().count() as u16, inner.y));
    if inner.height < 3 {
        return;
    }
    f.render_widget(
        Paragraph::new(Span::styled("─".repeat(inner.width as usize), Style::default().fg(theme::FRAME))),
        Rect { y: inner.y + 1, height: 1, ..inner },
    );
    let list = Rect { y: inner.y + 2, height: inner.height.saturating_sub(3), ..inner };
    let top = cur.saturating_sub(list.height.saturating_sub(1) as usize);
    for (i, (label, hint, note_like)) in rows.iter().enumerate().skip(top).take(list.height as usize) {
        let y = list.y + (i - top) as u16;
        let rr = Rect { y, height: 1, ..list };
        let lw = (label.chars().count() + 4).min(list.width as usize * 3 / 5);
        let icon = if *note_like { mark::NOTE } else { mark::BAR };
        let line = Line::from(vec![
            Span::styled(if i == cur { format!(" {} ", mark::BAR) } else { format!(" {icon} ") }, theme::accent()),
            Span::styled(fit(label, lw), theme::text().add_modifier(Modifier::BOLD)),
            Span::styled(format!(" {hint}"), theme::faint()),
        ]);
        let line = if i == cur {
            Line::from(line.spans.into_iter().map(|s| s.style(theme::chosen())).collect::<Vec<_>>())
        } else {
            line
        };
        f.render_widget(Paragraph::new(line), rr);
        app.hits.push((rr, Hit::Row(i)));
    }
    let y = inner.y + inner.height - 1;
    f.render_widget(Paragraph::new(Span::styled(format!(" {note}"), theme::faint())), Rect { y, height: 1, ..inner });
}

/// The folder picker: where it is and what that holds, then the ways on:
/// here, a new folder, up, and the folders in it.
fn folders(f: &mut Frame, app: &mut App, area: Rect, p: &super::folders::Picker) {
    use super::folders::Row;
    use crate::sync::browse::Holds;
    let rows = p.rows();
    let h = (rows.len() as u16 + 6).max(10).min(area.height.saturating_sub(4).max(3));
    let r = centered(area, 100, h);
    f.render_widget(Clear, r);
    let title = if app.pending.moving {
        "Move the memory to: choose its folder"
    } else {
        "Keep the memory in: choose its folder"
    };
    let b = block(title, true);
    let inner = b.inner(r);
    f.render_widget(b, r);
    if inner.height < 4 {
        return;
    }
    let state = if !p.error.is_empty() {
        theme::bad()
    } else {
        match p.holds {
            Holds::Nothing => theme::ok(),
            Holds::Memory(_) => theme::accent(),
            Holds::Other => theme::warn(),
        }
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(" {}", fit(&p.shown(), inner.width as usize - 2)),
                theme::text().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(format!(" {}", fit(&p.said(), inner.width as usize - 2)), state)),
        ]),
        Rect { height: 2, ..inner },
    );
    let list = Rect { y: inner.y + 3, height: inner.height.saturating_sub(4), ..inner };
    let top = p.cur.saturating_sub(list.height.saturating_sub(1) as usize);
    for (i, row) in rows.iter().enumerate().skip(top).take(list.height as usize) {
        let rr = Rect { y: list.y + (i - top) as u16, height: 1, ..list };
        let (icon, label, style) = match row {
            Row::Here => (mark::BAR, "Put the memory here".to_string(), theme::accent().add_modifier(Modifier::BOLD)),
            Row::New => ("+", "A new folder here".to_string(), theme::text()),
            Row::Up => ("..", "Up".to_string(), theme::dim()),
            Row::Dir(d) => (mark::FOLDER, d.clone(), theme::text()),
        };
        let line = format!(" {icon} {}", fit(&label, list.width as usize - 4));
        let style = if i == p.cur { theme::chosen() } else { style };
        f.render_widget(Paragraph::new(Span::styled(line, style)), rr);
        app.hits.push((rr, Hit::ModalButton(i)));
    }
    let y = inner.y + inner.height - 1;
    f.render_widget(
        Paragraph::new(Span::styled(
            " ↑↓ choose · Enter open or choose · ← up · n a new folder · Esc cancel",
            theme::faint(),
        )),
        Rect { y, height: 1, ..inner },
    );
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
