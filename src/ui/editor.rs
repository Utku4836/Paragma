//! The note editor's text: lines, a cursor, a selection, undo, and the
//! tools of notes (to-dos, tables, dates, links). No drawing here: the
//! screen (ui/app.rs) shows it and turns keys and clicks into these calls.
//! Keys as in VS Code and Notepad: Ctrl+C/X/V, Ctrl+Z/Y, Shift+arrows.

use crate::project::items;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub row: usize,
    /// In characters.
    pub col: usize,
}

#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    lines: Vec<String>,
    cur: Pos,
}

/// What the last edit was, so typing is undone a word at a time.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    Type,
    Delete,
    Other,
}

#[derive(Clone, Debug)]
pub struct Editor {
    pub lines: Vec<String>,
    pub cur: Pos,
    /// Where a selection started; the cursor is its other end.
    pub anchor: Option<Pos>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last: Kind,
    /// The column the cursor goes back to on the way up and down.
    goal: Option<usize>,
    /// The first visual row shown.
    pub scroll: usize,
    pub dirty: bool,
}

const UNDO: usize = 500;

fn len(s: &str) -> usize {
    s.chars().count()
}

/// The byte place of a character column.
fn byte(s: &str, col: usize) -> usize {
    s.char_indices().nth(col).map(|(i, _)| i).unwrap_or(s.len())
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl Editor {
    pub fn new(text: &str) -> Editor {
        let mut lines: Vec<String> = text.replace("\r\n", "\n").split('\n').map(str::to_string).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Editor {
            lines,
            cur: Pos::default(),
            anchor: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last: Kind::Other,
            goal: None,
            scroll: 0,
            dirty: false,
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn line_len(&self, row: usize) -> usize {
        self.lines.get(row).map(|l| len(l)).unwrap_or(0)
    }

    fn clamp(&self, p: Pos) -> Pos {
        let row = p.row.min(self.lines.len() - 1);
        Pos { row, col: p.col.min(self.line_len(row)) }
    }

    // ---------- undo ----------

    fn save(&mut self, kind: Kind) {
        // typing goes on in one step until a space or another kind of edit
        let same = kind != Kind::Other && kind == self.last;
        if !same {
            self.undo.push(Snapshot { lines: self.lines.clone(), cur: self.cur });
            if self.undo.len() > UNDO {
                self.undo.remove(0);
            }
        }
        self.last = kind;
        self.redo.clear();
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undo.pop() else { return false };
        self.redo.push(Snapshot { lines: self.lines.clone(), cur: self.cur });
        self.lines = s.lines;
        self.cur = self.clamp(s.cur);
        self.anchor = None;
        self.last = Kind::Other;
        self.dirty = true;
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else { return false };
        self.undo.push(Snapshot { lines: self.lines.clone(), cur: self.cur });
        self.lines = s.lines;
        self.cur = self.clamp(s.cur);
        self.anchor = None;
        self.last = Kind::Other;
        self.dirty = true;
        true
    }

    // ---------- selection ----------

    /// The selection, start first; None when nothing is selected.
    pub fn selection(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        if a == self.cur {
            return None;
        }
        Some(if a < self.cur { (a, self.cur) } else { (self.cur, a) })
    }

    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection()?;
        if a.row == b.row {
            let l = &self.lines[a.row];
            return Some(l[byte(l, a.col)..byte(l, b.col)].to_string());
        }
        let mut out = self.lines[a.row][byte(&self.lines[a.row], a.col)..].to_string();
        for r in a.row + 1..b.row {
            out.push('\n');
            out.push_str(&self.lines[r]);
        }
        out.push('\n');
        out.push_str(&self.lines[b.row][..byte(&self.lines[b.row], b.col)]);
        Some(out)
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(Pos::default());
        let last = self.lines.len() - 1;
        self.cur = Pos { row: last, col: self.line_len(last) };
    }

    /// Takes the selected text out; whether there was one.
    fn cut_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        let head = self.lines[a.row][..byte(&self.lines[a.row], a.col)].to_string();
        let tail = self.lines[b.row][byte(&self.lines[b.row], b.col)..].to_string();
        self.lines.splice(a.row..=b.row, std::iter::once(format!("{head}{tail}")));
        self.cur = a;
        self.anchor = None;
        true
    }

    /// Ctrl+X: the selected text, taken out (the whole line when nothing
    /// is selected, as in VS Code).
    pub fn cut(&mut self) -> Option<String> {
        if self.selection().is_none() {
            let row = self.cur.row;
            let text = format!("{}\n", self.lines[row]);
            self.save(Kind::Other);
            if self.lines.len() > 1 {
                self.lines.remove(row);
            } else {
                self.lines[0].clear();
            }
            self.cur = self.clamp(Pos { row, col: 0 });
            return Some(text);
        }
        let text = self.selected_text();
        self.save(Kind::Other);
        self.cut_selection();
        text
    }

    /// Ctrl+C: the selected text (the whole line when nothing is selected).
    pub fn copy(&self) -> String {
        self.selected_text().unwrap_or_else(|| format!("{}\n", self.lines[self.cur.row]))
    }

    // ---------- typing ----------

    /// Text typed or pasted at the cursor, in place of the selection.
    pub fn insert(&mut self, text: &str) {
        let kind =
            if text.chars().count() == 1 && !text.contains(char::is_whitespace) { Kind::Type } else { Kind::Other };
        self.save(kind);
        self.cut_selection();
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let row = self.cur.row;
        let at = byte(&self.lines[row], self.cur.col);
        let tail = self.lines[row][at..].to_string();
        self.lines[row].truncate(at);
        let mut parts = text.split('\n');
        let first = parts.next().unwrap_or("");
        self.lines[row].push_str(first);
        let mut r = row;
        let mut col = self.line_len(row);
        for p in parts {
            r += 1;
            self.lines.insert(r, p.to_string());
            col = len(p);
        }
        self.lines[r].push_str(&tail);
        self.cur = Pos { row: r, col };
        self.goal = None;
        if text.contains(char::is_whitespace) {
            self.last = Kind::Other;
        }
    }

    /// Enter: a new line; a list or a to-do goes on ("- ", "1. ", "- [ ] "),
    /// an empty item ends the list.
    pub fn newline(&mut self) {
        let line = self.lines[self.cur.row].clone();
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        let marker = list_marker(trimmed);
        if let Some(m) = &marker
            && trimmed.trim() == m.trim()
            && self.selection().is_none()
        {
            // an empty item: the list ends here
            self.save(Kind::Other);
            self.lines[self.cur.row] = indent.to_string();
            self.cur.col = len(indent);
            return;
        }
        let next = match marker {
            Some(m) => format!("\n{indent}{}", next_marker(&m)),
            None => format!("\n{indent}"),
        };
        self.insert(&next);
    }

    pub fn backspace(&mut self) {
        if self.selection().is_some() {
            self.save(Kind::Other);
            self.cut_selection();
            return;
        }
        self.anchor = None;
        if self.cur.col > 0 {
            self.save(Kind::Delete);
            let l = &mut self.lines[self.cur.row];
            let (a, b) = (byte(l, self.cur.col - 1), byte(l, self.cur.col));
            l.replace_range(a..b, "");
            self.cur.col -= 1;
        } else if self.cur.row > 0 {
            self.save(Kind::Other);
            let line = self.lines.remove(self.cur.row);
            self.cur.row -= 1;
            self.cur.col = self.line_len(self.cur.row);
            self.lines[self.cur.row].push_str(&line);
        }
        self.goal = None;
    }

    pub fn delete(&mut self) {
        if self.selection().is_some() {
            self.save(Kind::Other);
            self.cut_selection();
            return;
        }
        self.anchor = None;
        if self.cur.col < self.line_len(self.cur.row) {
            self.save(Kind::Delete);
            let l = &mut self.lines[self.cur.row];
            let (a, b) = (byte(l, self.cur.col), byte(l, self.cur.col + 1));
            l.replace_range(a..b, "");
        } else if self.cur.row + 1 < self.lines.len() {
            self.save(Kind::Other);
            let next = self.lines.remove(self.cur.row + 1);
            self.lines[self.cur.row].push_str(&next);
        }
    }

    // ---------- moving ----------

    fn start_moving(&mut self, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cur);
            }
        } else {
            self.anchor = None;
        }
        self.last = Kind::Other;
    }

    pub fn left(&mut self, select: bool, word: bool) {
        // without Shift a selection collapses to its start
        if !select && let Some((a, _)) = self.selection() {
            self.cur = a;
            self.anchor = None;
            return;
        }
        self.start_moving(select);
        if self.cur.col == 0 {
            if self.cur.row > 0 {
                self.cur.row -= 1;
                self.cur.col = self.line_len(self.cur.row);
            }
        } else if word {
            let chars: Vec<char> = self.lines[self.cur.row].chars().collect();
            let mut c = self.cur.col;
            while c > 0 && !word_char(chars[c - 1]) {
                c -= 1;
            }
            while c > 0 && word_char(chars[c - 1]) {
                c -= 1;
            }
            self.cur.col = c;
        } else {
            self.cur.col -= 1;
        }
        self.goal = None;
    }

    pub fn right(&mut self, select: bool, word: bool) {
        if !select && let Some((_, b)) = self.selection() {
            self.cur = b;
            self.anchor = None;
            return;
        }
        self.start_moving(select);
        let n = self.line_len(self.cur.row);
        if self.cur.col >= n {
            if self.cur.row + 1 < self.lines.len() {
                self.cur.row += 1;
                self.cur.col = 0;
            }
        } else if word {
            let chars: Vec<char> = self.lines[self.cur.row].chars().collect();
            let mut c = self.cur.col;
            while c < n && word_char(chars[c]) {
                c += 1;
            }
            while c < n && !word_char(chars[c]) {
                c += 1;
            }
            self.cur.col = c;
        } else {
            self.cur.col += 1;
        }
        self.goal = None;
    }

    pub fn up(&mut self, select: bool, by: usize) {
        self.start_moving(select);
        let goal = *self.goal.get_or_insert(self.cur.col);
        if self.cur.row == 0 {
            self.cur.col = 0;
        } else {
            self.cur.row = self.cur.row.saturating_sub(by);
            self.cur.col = goal.min(self.line_len(self.cur.row));
        }
    }

    pub fn down(&mut self, select: bool, by: usize) {
        self.start_moving(select);
        let goal = *self.goal.get_or_insert(self.cur.col);
        let last = self.lines.len() - 1;
        if self.cur.row == last {
            self.cur.col = self.line_len(last);
        } else {
            self.cur.row = (self.cur.row + by).min(last);
            self.cur.col = goal.min(self.line_len(self.cur.row));
        }
    }

    /// Home: the first character that is not a space, then the very start.
    pub fn home(&mut self, select: bool, document: bool) {
        self.start_moving(select);
        if document {
            self.cur = Pos::default();
        } else {
            let line = &self.lines[self.cur.row];
            let first = line.chars().take_while(|c| c.is_whitespace()).count();
            self.cur.col = if self.cur.col == first { 0 } else { first };
        }
        self.goal = None;
    }

    pub fn end(&mut self, select: bool, document: bool) {
        self.start_moving(select);
        if document {
            self.cur.row = self.lines.len() - 1;
        }
        self.cur.col = self.line_len(self.cur.row);
        self.goal = None;
    }

    /// A click (or a drag, `select`) at a place of the text.
    pub fn click(&mut self, at: Pos, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cur);
            }
        } else {
            self.anchor = None;
        }
        self.cur = self.clamp(at);
        self.goal = None;
        self.last = Kind::Other;
    }

    /// A double click: the word there.
    pub fn select_word(&mut self, at: Pos) {
        let at = self.clamp(at);
        let chars: Vec<char> = self.lines[at.row].chars().collect();
        let (mut a, mut b) = (at.col, at.col);
        while a > 0 && word_char(chars[a - 1]) {
            a -= 1;
        }
        while b < chars.len() && word_char(chars[b]) {
            b += 1;
        }
        self.anchor = Some(Pos { row: at.row, col: a });
        self.cur = Pos { row: at.row, col: b };
    }

    // ---------- the tools of notes ----------

    /// Ctrl+Enter: the to-do on this line ticked or opened again; a line
    /// that is not one becomes one.
    pub fn toggle_todo(&mut self) {
        let row = self.cur.row;
        let line = self.lines[row].clone();
        let t = line.trim_start();
        let indent = &line[..line.len() - t.len()];
        let new = if let Some(rest) = t.strip_prefix("- [ ] ") {
            format!("{indent}- [x] {rest}")
        } else if let Some(rest) = t.strip_prefix("- [x] ").or_else(|| t.strip_prefix("- [X] ")) {
            format!("{indent}- [ ] {rest}")
        } else if let Some(rest) = t.strip_prefix("- ") {
            format!("{indent}- [ ] {rest}")
        } else {
            format!("{indent}- [ ] {t}")
        };
        self.save(Kind::Other);
        let grew = len(&new) as isize - len(&line) as isize;
        self.lines[row] = new;
        self.cur.col = ((self.cur.col as isize + grew).max(0) as usize).min(self.line_len(row));
    }

    /// Today's date (Ctrl+D), as notes and pg write --due take it.
    pub fn insert_date(&mut self, today: &str) {
        self.insert(today);
    }

    /// A new table (Ctrl+T) with the cursor in its first cell.
    pub fn insert_table(&mut self) {
        let row = self.cur.row;
        let at_start = self.lines[row].trim().is_empty();
        let table = "| Column | Column |\n|--------|--------|\n|        |        |";
        if !at_start {
            self.end(false, false);
            self.insert("\n");
        }
        let top = self.cur.row;
        self.insert(table);
        self.cur = Pos { row: top, col: 2 };
        self.anchor = Some(Pos { row: top, col: 2 });
        self.cur.col = 8;
    }

    /// The table the cursor is in: its first and last rows.
    fn table_rows(&self) -> Option<(usize, usize)> {
        let is_row = |r: usize| self.lines.get(r).is_some_and(|l| l.trim_start().starts_with('|'));
        if !is_row(self.cur.row) {
            return None;
        }
        let mut a = self.cur.row;
        while a > 0 && is_row(a - 1) {
            a -= 1;
        }
        let mut b = self.cur.row;
        while is_row(b + 1) {
            b += 1;
        }
        Some((a, b))
    }

    /// Tab in a table: the columns lined up, the cursor in the next cell (a
    /// new row after the last one); Shift+Tab the cell before. Not in a
    /// table: two spaces (Tab) or false (Shift+Tab).
    pub fn tab(&mut self, back: bool) -> bool {
        let Some((a, b)) = self.table_rows() else {
            if back {
                return false;
            }
            self.insert("  ");
            return true;
        };
        // the cell the cursor is in
        let row_text = self.lines[self.cur.row].clone();
        let before: String = row_text.chars().take(self.cur.col).collect();
        let cell = before.matches('|').count().saturating_sub(1);
        let body = self.lines[a..=b].join("\n");
        let Some(t) = items::table(&body, None) else {
            self.insert("  ");
            return true;
        };
        let cols = t.header.len().max(1);
        let rows_total = t.rows.len() + 2; // header, rule, rows
        let mut new = t.clone();
        let here = self.cur.row - a; // 0 header, 1 rule, 2.. rows
        let (mut tr, mut tc) = (here, cell);
        if back {
            if tc > 0 {
                tc -= 1;
            } else if tr > 0 {
                tr = if tr == 2 { 0 } else { tr - 1 };
                tc = cols - 1;
            }
        } else if tc + 1 < cols {
            tc += 1;
        } else {
            tr = if tr == 0 { 2 } else { tr + 1 };
            tc = 0;
            if tr >= rows_total {
                new.rows.push(vec![String::new(); cols]);
            }
        }
        if tr == 1 {
            tr = if back { 0 } else { 2 };
        }
        let rendered = items::render(&new);
        self.save(Kind::Other);
        self.lines.splice(a..=b, rendered.iter().cloned());
        let line = &self.lines[a + tr];
        // the cursor after the "| " of cell tc
        let mut bars = 0;
        let mut col = 0;
        for (i, ch) in line.chars().enumerate() {
            if ch == '|' {
                if bars == tc {
                    col = i + 2;
                    break;
                }
                bars += 1;
            }
        }
        self.cur = Pos { row: a + tr, col: col.min(len(line)) };
        // the cell's text selected, to type over it
        let cell_end = line.chars().skip(col).position(|c| c == '|').map(|p| col + p).unwrap_or(len(line));
        let text_end = line.chars().take(cell_end).collect::<String>().trim_end().chars().count();
        self.anchor = None;
        if text_end > col {
            self.anchor = Some(self.cur);
            self.cur.col = text_end;
        }
        true
    }

    /// The word being typed after "[[" (to offer notes for a link), if any.
    pub fn link_prefix(&self) -> Option<String> {
        let line = &self.lines[self.cur.row];
        let before: String = line.chars().take(self.cur.col).collect();
        let at = before.rfind("[[")?;
        let typed = &before[at + 2..];
        (!typed.contains("]]") && !typed.contains('\n')).then(|| typed.to_string())
    }

    /// The link chosen for the "[[" being typed: "[[name]]".
    pub fn complete_link(&mut self, name: &str) {
        let Some(typed) = self.link_prefix() else { return };
        self.save(Kind::Other);
        for _ in 0..len(&typed) {
            let l = &mut self.lines[self.cur.row];
            let (a, b) = (byte(l, self.cur.col - 1), byte(l, self.cur.col));
            l.replace_range(a..b, "");
            self.cur.col -= 1;
        }
        let row = self.cur.row;
        let l = &self.lines[row];
        let closes = l.chars().skip(self.cur.col).take(2).collect::<String>() == "]]";
        let text = if closes { name.to_string() } else { format!("{name}]]") };
        let at = byte(&self.lines[row], self.cur.col);
        self.lines[row].insert_str(at, &text);
        self.cur.col += len(&text) + if closes { 2 } else { 0 };
    }

    // ---------- finding ----------

    /// The next place of `query` after the cursor (from the top again at
    /// the end), any case: it is selected.
    pub fn find(&mut self, query: &str, back: bool) -> bool {
        if query.is_empty() {
            return false;
        }
        let q = query.to_lowercase();
        let n = self.lines.len();
        let start = self.selection().map(|(a, b)| if back { a } else { b }).unwrap_or(self.cur);
        for k in 0..=n {
            let r = if back { (start.row + n * 2 - k) % n } else { (start.row + k) % n };
            let low = self.lines[r].to_lowercase();
            let chars: Vec<(usize, char)> = low.char_indices().collect();
            let mut hits: Vec<usize> = Vec::new();
            for (ci, (bi, _)) in chars.iter().enumerate() {
                if low[*bi..].starts_with(&q) {
                    hits.push(ci);
                }
            }
            let pick = if back {
                hits.into_iter().rev().find(|c| k > 0 || *c < start.col)
            } else {
                hits.into_iter().find(|c| k > 0 || *c >= start.col)
            };
            if let Some(c) = pick {
                self.anchor = Some(Pos { row: r, col: c });
                self.cur = Pos { row: r, col: c + len(query) };
                return true;
            }
        }
        false
    }

    // ---------- the screen's rows ----------

    /// The visual rows of the text at a width (long lines wrap at a space
    /// when they can): (line, first column, last column).
    pub fn rows(&self, width: usize) -> Vec<(usize, usize, usize)> {
        let width = width.max(8);
        let mut out = Vec::new();
        for (r, line) in self.lines.iter().enumerate() {
            let chars: Vec<char> = line.chars().collect();
            if chars.len() <= width {
                out.push((r, 0, chars.len()));
                continue;
            }
            let mut start = 0;
            while start < chars.len() {
                let mut end = (start + width).min(chars.len());
                if end < chars.len()
                    && let Some(sp) = chars[start..end].iter().rposition(|c| *c == ' ')
                    && sp > 0
                {
                    end = start + sp + 1;
                }
                out.push((r, start, end));
                start = end;
            }
        }
        out
    }

    /// The visual row and column of the cursor.
    pub fn cursor_row(&self, rows: &[(usize, usize, usize)]) -> (usize, usize) {
        let mut best = 0;
        for (i, (r, a, b)) in rows.iter().enumerate() {
            if *r == self.cur.row
                && self.cur.col >= *a
                && (self.cur.col < *b || (self.cur.col == *b && rows.get(i + 1).is_none_or(|n| n.0 != *r)))
            {
                return (i, self.cur.col - a);
            }
            if *r == self.cur.row {
                best = i;
            }
        }
        (best, 0)
    }

    /// The text place of a visual row and column (a click).
    pub fn place(&self, rows: &[(usize, usize, usize)], vrow: usize, vcol: usize) -> Pos {
        match rows.get(vrow) {
            Some((r, a, b)) => Pos { row: *r, col: (a + vcol).min(*b) },
            None => {
                let last = self.lines.len() - 1;
                Pos { row: last, col: self.line_len(last) }
            }
        }
    }

    /// The scroll that keeps the cursor in view in `height` rows.
    pub fn follow(&mut self, rows: &[(usize, usize, usize)], height: usize) {
        let (vr, _) = self.cursor_row(rows);
        if vr < self.scroll {
            self.scroll = vr;
        } else if vr >= self.scroll + height.max(1) {
            self.scroll = vr + 1 - height.max(1);
        }
    }
}

/// A list's marker at the start of a line ("- ", "* ", "1. ", "- [ ] ").
fn list_marker(t: &str) -> Option<String> {
    for m in ["- [ ] ", "- [x] ", "- [X] ", "- ", "* ", "+ "] {
        if t.starts_with(m) {
            return Some(m.to_string());
        }
    }
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !digits.is_empty() && t[digits.len()..].starts_with(". ") {
        return Some(format!("{digits}. "));
    }
    None
}

/// The marker of the next item: a to-do stays one, open; a number counts up.
fn next_marker(m: &str) -> String {
    if m.starts_with("- [") {
        return "- [ ] ".into();
    }
    match m.trim_end_matches(". ").parse::<usize>() {
        Ok(n) if m.ends_with(". ") => format!("{}. ", n + 1),
        _ => m.to_string(),
    }
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/ui/editor.rs"]
mod tests;
