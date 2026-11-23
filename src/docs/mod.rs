//! Office documents read like code: by parts. An Excel workbook by its
//! sheets (and a range of one: `Sheet1!A1:D20`), a Word document by its
//! headings, a PowerPoint deck by its slides. They are zip files of XML;
//! nothing else is installed to read them.
//!
//! ```text
//! pg read finans/budget.xlsx#Sheet1!A1:D20
//! pg read contract.docx#Payment
//! pg read deck.pptx#3
//! ```

use anyhow::{Context, Result, bail};
use quick_xml::Reader;
use quick_xml::events::Event;
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

/// The kinds of documents read by parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Xlsx,
    Docx,
    Pptx,
}

impl Kind {
    pub fn of(path: &Path) -> Option<Kind> {
        match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
            "xlsx" | "xlsm" => Some(Kind::Xlsx),
            "docx" => Some(Kind::Docx),
            "pptx" => Some(Kind::Pptx),
            _ => None,
        }
    }
}

/// A part of a document: a sheet, a section under a heading, a slide.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub name: String,
    pub text: String,
}

/// Cells shown of a sheet at most when no range is given.
const SHEET_ROWS: usize = 60;
const SHEET_COLS: usize = 12;
/// A document's file read at most (a zip bomb is not opened).
const MOST: u64 = 200 * 1024 * 1024;

fn archive(path: &Path) -> Result<zip::ZipArchive<std::fs::File>> {
    let f = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    zip::ZipArchive::new(f).with_context(|| format!("{} is not an Office document (not a zip)", path.display()))
}

fn entry(zip: &mut zip::ZipArchive<std::fs::File>, name: &str) -> Option<String> {
    let mut e = zip.by_name(name).ok()?;
    if e.size() > MOST {
        return None;
    }
    let mut s = String::new();
    e.read_to_string(&mut s).ok()?;
    Some(s)
}

/// The parts of a document, in order.
pub fn parts(path: &Path) -> Result<Vec<Part>> {
    match Kind::of(path) {
        Some(Kind::Xlsx) => xlsx_parts(path),
        Some(Kind::Docx) => docx_parts(path),
        Some(Kind::Pptx) => pptx_parts(path),
        None => bail!("{} is not a document pg reads by parts (xlsx, docx, pptx)", path.display()),
    }
}

/// All the text of a document (for search), each part under its name.
pub fn text(path: &Path) -> Result<String> {
    Ok(parts(path)?.into_iter().map(|p| format!("## {}\n{}", p.name, p.text)).collect::<Vec<_>>().join("\n\n"))
}

/// One part, by its name (any case), its number (from 1), or for a sheet a
/// range of it ("Sheet1!A1:D20", "A1:D20" of the first sheet).
pub fn part(path: &Path, which: &str) -> Result<Part> {
    let which = which.trim();
    if Kind::of(path) == Some(Kind::Xlsx) && which.contains(':') {
        let (sheet, range) = which.rsplit_once('!').unwrap_or(("", which));
        return sheet_range(path, sheet, range);
    }
    let all = parts(path)?;
    let names: Vec<String> = all.iter().map(|p| p.name.clone()).collect();
    if let Ok(n) = which.parse::<usize>()
        && n >= 1
        && n <= all.len()
    {
        return Ok(all[n - 1].clone());
    }
    let lower = which.to_lowercase();
    if let Some(p) = all.iter().find(|p| p.name.to_lowercase() == lower) {
        return Ok(p.clone());
    }
    let hits: Vec<&Part> = all.iter().filter(|p| p.name.to_lowercase().contains(&lower)).collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        _ => bail!("{} has no part {which:?}; its parts: {}", path.display(), names.join(", ")),
    }
}

// ---------- Excel ----------

/// The sheets' names and their files in the workbook, in order.
fn sheets(zip: &mut zip::ZipArchive<std::fs::File>) -> Vec<(String, String)> {
    let workbook = entry(zip, "xl/workbook.xml").unwrap_or_default();
    let rels = entry(zip, "xl/_rels/workbook.xml.rels").unwrap_or_default();
    let mut targets: HashMap<String, String> = HashMap::new();
    for attrs in elements(&rels, "Relationship") {
        if let (Some(id), Some(t)) = (attrs.get("Id"), attrs.get("Target")) {
            let t = t.trim_start_matches('/').trim_start_matches("xl/");
            targets.insert(id.clone(), format!("xl/{t}"));
        }
    }
    elements(&workbook, "sheet")
        .into_iter()
        .filter_map(|a| Some((a.get("name")?.clone(), targets.get(a.get("r:id")?)?.clone())))
        .collect()
}

fn shared_strings(zip: &mut zip::ZipArchive<std::fs::File>) -> Vec<String> {
    let xml = entry(zip, "xl/sharedStrings.xml").unwrap_or_default();
    let mut out = Vec::new();
    let mut r = Reader::from_str(&xml);
    let mut cur: Option<String> = None;
    let mut in_t = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"si" => cur = Some(String::new()),
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"t" => in_t = true,
            Ok(Event::End(e)) if e.local_name().as_ref() == b"t" => in_t = false,
            Ok(Event::Text(t)) if in_t => {
                if let (Some(c), Ok(s)) = (cur.as_mut(), t.unescape()) {
                    c.push_str(&s);
                }
            }
            Ok(Event::End(e)) if e.local_name().as_ref() == b"si" => out.push(cur.take().unwrap_or_default()),
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// "C12" -> (row 12, column 3), from 1.
fn cell_ref(r: &str) -> Option<(usize, usize)> {
    let letters: String = r.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
    let digits = &r[letters.len()..];
    if letters.is_empty() {
        return None;
    }
    let col = letters.to_ascii_uppercase().bytes().fold(0usize, |n, b| n * 26 + (b - b'A' + 1) as usize);
    Some((digits.parse().ok()?, col))
}

fn col_name(mut c: usize) -> String {
    let mut s = Vec::new();
    while c > 0 {
        let m = (c - 1) % 26;
        s.push(b'A' + m as u8);
        c = (c - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

/// The cells of a sheet with a value: (row, column) -> text. Formulas give
/// the value Excel saved with them.
fn cells(xml: &str, strings: &[String]) -> HashMap<(usize, usize), String> {
    let mut out = HashMap::new();
    let mut r = Reader::from_str(xml);
    let mut at: Option<(usize, usize)> = None;
    let mut kind = String::new();
    let mut in_v = false;
    let mut value = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"c" => {
                let a = attrs_of(&e);
                at = a.get("r").and_then(|x| cell_ref(x));
                kind = a.get("t").cloned().unwrap_or_default();
                value.clear();
            }
            Ok(Event::Start(e)) if matches!(e.local_name().as_ref(), b"v" | b"t") => in_v = true,
            Ok(Event::End(e)) if matches!(e.local_name().as_ref(), b"v" | b"t") => in_v = false,
            Ok(Event::Text(t)) if in_v => {
                if let Ok(s) = t.unescape() {
                    value.push_str(&s);
                }
            }
            Ok(Event::End(e)) if e.local_name().as_ref() == b"c" => {
                if let Some(p) = at.take() {
                    let v = match kind.as_str() {
                        "s" => {
                            value.trim().parse::<usize>().ok().and_then(|i| strings.get(i).cloned()).unwrap_or_default()
                        }
                        "b" => {
                            if value.trim() == "1" {
                                "TRUE".into()
                            } else {
                                "FALSE".into()
                            }
                        }
                        _ => value.clone(),
                    };
                    if !v.is_empty() {
                        out.insert(p, v);
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Cells as a Markdown table with the column letters and row numbers.
fn grid(cells: &HashMap<(usize, usize), String>, rows: (usize, usize), cols: (usize, usize)) -> String {
    let mut lines = Vec::new();
    let head: Vec<String> = (cols.0..=cols.1).map(col_name).collect();
    lines.push(format!("|   | {} |", head.join(" | ")));
    lines.push(format!("|---|{}|", vec!["---"; head.len()].join("|")));
    for row in rows.0..=rows.1 {
        let vals: Vec<String> = (cols.0..=cols.1)
            .map(|c| cells.get(&(row, c)).map(|v| v.replace('|', "\\|").replace('\n', " ")).unwrap_or_default())
            .collect();
        if vals.iter().all(String::is_empty) {
            continue;
        }
        lines.push(format!("| {row} | {} |", vals.join(" | ")));
    }
    lines.join("\n")
}

fn xlsx_parts(path: &Path) -> Result<Vec<Part>> {
    let mut zip = archive(path)?;
    let strings = shared_strings(&mut zip);
    let mut out = Vec::new();
    for (name, file) in sheets(&mut zip) {
        let xml = entry(&mut zip, &file).unwrap_or_default();
        let c = cells(&xml, &strings);
        if c.is_empty() {
            out.push(Part { name, text: "(empty)".into() });
            continue;
        }
        let (r1, r2) = (c.keys().map(|k| k.0).min().unwrap_or(1), c.keys().map(|k| k.0).max().unwrap_or(1));
        let (c1, c2) = (c.keys().map(|k| k.1).min().unwrap_or(1), c.keys().map(|k| k.1).max().unwrap_or(1));
        let shown_rows = (r1, r2.min(r1 + SHEET_ROWS - 1));
        let shown_cols = (c1, c2.min(c1 + SHEET_COLS - 1));
        let mut text = grid(&c, shown_rows, shown_cols);
        if r2 > shown_rows.1 || c2 > shown_cols.1 {
            text.push_str(&format!(
                "\n(used: {}{}:{}{}; a part of it: #{name}!A1:D20)",
                col_name(c1),
                r1,
                col_name(c2),
                r2
            ));
        }
        out.push(Part { name, text });
    }
    Ok(out)
}

fn sheet_range(path: &Path, sheet: &str, range: &str) -> Result<Part> {
    let mut zip = archive(path)?;
    let strings = shared_strings(&mut zip);
    let all = sheets(&mut zip);
    let Some((name, file)) = (if sheet.is_empty() {
        all.first().cloned()
    } else {
        all.iter().find(|(n, _)| n.eq_ignore_ascii_case(sheet.trim_matches('\''))).cloned()
    }) else {
        bail!(
            "{} has no sheet {sheet:?}; its sheets: {}",
            path.display(),
            all.iter().map(|s| s.0.as_str()).collect::<Vec<_>>().join(", ")
        );
    };
    let (a, b) = range.split_once(':').unwrap_or((range, range));
    let (Some((r1, c1)), Some((r2, c2))) = (cell_ref(a.trim()), cell_ref(b.trim())) else {
        bail!("give a range as A1:D20, not {range:?}");
    };
    let xml = entry(&mut zip, &file).unwrap_or_default();
    let c = cells(&xml, &strings);
    Ok(Part { name: format!("{name}!{range}"), text: grid(&c, (r1.min(r2), r1.max(r2)), (c1.min(c2), c1.max(c2))) })
}

// ---------- Word ----------

/// The outline level of each paragraph style (0: a top heading), by its
/// id, following the style it is based on. A heading is known by its
/// level, not by its name: Word names styles in its own language
/// ("Balk1" in Turkish, "berschrift1" in German).
fn outline_levels(styles: &str) -> HashMap<String, usize> {
    let mut own: HashMap<String, (Option<usize>, Option<String>)> = HashMap::new();
    let mut r = Reader::from_str(styles);
    let mut cur: Option<String> = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"style" => {
                let a = attrs_of(&e);
                cur = a.get("w:styleId").cloned();
                if let Some(id) = &cur {
                    own.entry(id.clone()).or_default();
                }
            }
            Ok(Event::End(e)) if e.local_name().as_ref() == b"style" => cur = None,
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if cur.is_some() => {
                let a = attrs_of(&e);
                let id = cur.clone().unwrap_or_default();
                match e.local_name().as_ref() {
                    b"outlineLvl" => {
                        if let Some(v) = a.get("w:val").and_then(|v| v.parse::<usize>().ok()) {
                            own.entry(id).or_default().0 = Some(v);
                        }
                    }
                    b"basedOn" => own.entry(id).or_default().1 = a.get("w:val").cloned(),
                    _ => {}
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let mut out = HashMap::new();
    for id in own.keys() {
        let mut at = id.clone();
        for _ in 0..10 {
            match own.get(&at) {
                Some((Some(level), _)) => {
                    if *level < 9 {
                        out.insert(id.clone(), *level);
                    }
                    break;
                }
                Some((None, Some(base))) => at = base.clone(),
                _ => break,
            }
        }
    }
    out
}

fn docx_parts(path: &Path) -> Result<Vec<Part>> {
    let mut zip = archive(path)?;
    let xml = entry(&mut zip, "word/document.xml").context("no word/document.xml in it")?;
    let levels = outline_levels(&entry(&mut zip, "word/styles.xml").unwrap_or_default());
    let mut r = Reader::from_str(&xml);
    let mut paragraphs: Vec<(Option<usize>, String)> = Vec::new(); // heading level, text
    let mut cur = String::new();
    let mut level: Option<usize> = None;
    let mut in_t = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().as_ref() == b"pStyle" => {
                if let Some(v) = attrs_of(&e).get("w:val") {
                    level = levels.get(v).copied().or(level);
                }
            }
            // a paragraph's own outline level
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().as_ref() == b"outlineLvl" => {
                if let Some(v) = attrs_of(&e).get("w:val").and_then(|v| v.parse::<usize>().ok())
                    && v < 9
                {
                    level = Some(v);
                }
            }
            Ok(Event::Start(e)) if e.local_name().as_ref() == b"t" => in_t = true,
            Ok(Event::End(e)) if e.local_name().as_ref() == b"t" => in_t = false,
            Ok(Event::Empty(e)) if e.local_name().as_ref() == b"tab" => cur.push('\t'),
            Ok(Event::Empty(e)) if e.local_name().as_ref() == b"br" => cur.push('\n'),
            Ok(Event::Text(t)) if in_t => {
                if let Ok(s) = t.unescape() {
                    cur.push_str(&s);
                }
            }
            Ok(Event::End(e)) if e.local_name().as_ref() == b"p" => {
                paragraphs.push((level.take(), std::mem::take(&mut cur)));
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let mut out: Vec<Part> = Vec::new();
    let mut name = String::from("(start)");
    let mut text = String::new();
    for (lvl, p) in paragraphs {
        if lvl.is_some() && !p.trim().is_empty() {
            if !text.trim().is_empty() || name != "(start)" {
                out.push(Part { name: name.clone(), text: text.trim().to_string() });
            }
            name = p.trim().to_string();
            text.clear();
        } else if !p.trim().is_empty() {
            text.push_str(p.trim_end());
            text.push('\n');
        }
    }
    if !text.trim().is_empty() || name != "(start)" {
        out.push(Part { name, text: text.trim().to_string() });
    }
    Ok(out)
}

// ---------- PowerPoint ----------

fn pptx_parts(path: &Path) -> Result<Vec<Part>> {
    let mut zip = archive(path)?;
    let mut slides: Vec<(usize, String)> = (0..zip.len())
        .filter_map(|i| {
            let name = zip.by_index(i).ok()?.name().ok()?.to_string();
            let n = name.strip_prefix("ppt/slides/slide")?.strip_suffix(".xml")?.parse::<usize>().ok()?;
            Some((n, name))
        })
        .collect();
    slides.sort();
    let mut out = Vec::new();
    for (n, file) in slides {
        let xml = entry(&mut zip, &file).unwrap_or_default();
        let mut r = Reader::from_str(&xml);
        let mut lines: Vec<String> = Vec::new();
        let mut cur = String::new();
        let mut in_t = false;
        loop {
            match r.read_event() {
                Ok(Event::Start(e)) if e.local_name().as_ref() == b"t" => in_t = true,
                Ok(Event::End(e)) if e.local_name().as_ref() == b"t" => in_t = false,
                Ok(Event::Text(t)) if in_t => {
                    if let Ok(s) = t.unescape() {
                        cur.push_str(&s);
                    }
                }
                Ok(Event::End(e)) if e.local_name().as_ref() == b"p" => {
                    if !cur.trim().is_empty() {
                        lines.push(std::mem::take(&mut cur));
                    }
                    cur.clear();
                }
                Ok(Event::Eof) | Err(_) => break,
                _ => {}
            }
        }
        let title = lines.first().cloned().unwrap_or_default();
        out.push(Part {
            name: format!("{n}"),
            text: if lines.is_empty() { format!("(slide {n}: no text)") } else { lines.join("\n") },
        });
        let _ = title;
    }
    Ok(out)
}

// ---------- XML ----------

fn attrs_of(e: &quick_xml::events::BytesStart) -> HashMap<String, String> {
    e.attributes()
        .flatten()
        .filter_map(|a| {
            let k = String::from_utf8(a.key.as_ref().to_vec()).ok()?;
            let v = a.unescape_value().ok()?.to_string();
            Some((k, v))
        })
        .collect()
}

/// The attributes of every element with this local name.
fn elements(xml: &str, name: &str) -> Vec<HashMap<String, String>> {
    let mut out = Vec::new();
    let mut r = Reader::from_str(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.local_name().as_ref() == name.as_bytes() => {
                out.push(attrs_of(&e));
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/docs/mod.rs"]
mod tests;
