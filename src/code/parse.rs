//! Symbols of a source file: functions, methods, classes, structs,
//! interfaces, enums... with their lines, signature, the names they call,
//! and a hash of their body that survives a rename or a move.
//!
//! One walk serves most grammars: tree-sitter grammars name their
//! definition nodes alike (function_declaration, class_definition,
//! function_item, method_declaration, ...) and give them a `name` field.
//! WGSL, GraphQL, Protobuf and Dockerfiles name theirs otherwise
//! (`special`). Code embedded in JavaScript and TypeScript strings marked
//! with its language (/* wgsl */ `...`, sql`...`) is read as that language,
//! its symbols under the name the string is kept in (SEA_SHADER.groundAt).

use super::lang::Lang;
use super::text;
use tree_sitter::{Node, Parser};

#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    pub name: String,
    /// With the enclosing symbols: "Cart.Total".
    pub qualified: String,
    /// function, method, class, struct, interface, trait, enum, impl, module, type
    pub kind: &'static str,
    /// First and last line, from 1.
    pub start: usize,
    pub end: usize,
    /// The first line of the definition.
    pub signature: String,
    /// The comment right above it, first line.
    pub doc: String,
    /// The names it calls.
    pub calls: Vec<String>,
    /// Its text without its name and spacing: the same after a rename or a move.
    pub body_hash: u64,
    /// More of what it is about, for its meaning only (a section's text).
    pub about: String,
}

/// Calls kept per symbol.
const MAX_CALLS: usize = 200;

pub fn symbols(lang: Lang, source: &str) -> Vec<Symbol> {
    symbols_at(lang, source, 0)
}

fn symbols_at(lang: Lang, source: &str, depth: usize) -> Vec<Symbol> {
    match lang {
        Lang::Markdown => return text::sections(source),
        Lang::Yaml => return text::yaml_jobs(source),
        Lang::Sql => return text::sql_objects(source),
        Lang::Vue | Lang::Svelte => {
            let (ts, scripts) = text::scripts_only(source);
            return symbols_at(if ts { Lang::TypeScript } else { Lang::JavaScript }, &scripts, depth);
        }
        _ => {}
    }
    let Some(grammar) = lang.grammar() else { return Vec::new() };
    let mut parser = Parser::new();
    if parser.set_language(&grammar).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else { return Vec::new() };
    let mut w = Walk { lang, src: source, parents: Vec::new(), out: Vec::new(), embedded: Vec::new() };
    w.walk(tree.root_node(), 0);
    let mut out = w.out;
    if lang == Lang::Dockerfile {
        text::stages_end(&mut out, source);
    }
    if lang == Lang::Hlsl {
        out.extend(text::hlsl_buffers(source));
    }
    if depth < 2 {
        for e in &w.embedded {
            out.extend(embedded_symbols(e, source, depth));
        }
    }
    out
}

/// A string holding code in another language, found in the walk.
struct Embedded {
    /// None: kept as a symbol, its inside not read (css).
    lang: Option<Lang>,
    marker: String,
    /// Byte range of the string's text, without its quotes.
    start: usize,
    end: usize,
    /// Byte ranges of its ${...} parts.
    holes: Vec<(usize, usize)>,
    /// The constant it is kept in, if any.
    holder: Option<Holder>,
    /// The symbols around it (a function returning it).
    prefix: String,
}

struct Holder {
    name: String,
    start: usize,
    end: usize,
    signature: String,
    doc: String,
}

struct Walk<'a> {
    lang: Lang,
    src: &'a str,
    parents: Vec<(String, &'static str)>,
    out: Vec<Symbol>,
    embedded: Vec<Embedded>,
}

impl Walk<'_> {
    fn walk(&mut self, node: Node, depth: usize) {
        if depth > 200 {
            return;
        }
        if matches!(self.lang, Lang::JavaScript | Lang::TypeScript | Lang::Tsx) && node.kind() == "template_string" {
            if let Some(e) = embedded_at(node, self.src, &self.parents) {
                self.embedded.push(e);
            }
            return;
        }
        if self.lang == Lang::Wgsl && node.kind() == "ERROR" {
            // the grammar predates module-level const and alias
            self.out.extend(wgsl_in_error(node, self.src, &self.parents));
        }
        let found = definition(self.lang, node, self.src, &self.parents);
        if let Some(s) = &found {
            self.parents.push((s.name.clone(), s.kind));
        }
        let mut cursor = node.walk();
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        for child in children {
            self.walk(child, depth + 1);
        }
        if let Some(s) = found {
            self.parents.pop();
            self.out.push(s);
        }
    }
}

/// The symbol a node defines, if it is a definition.
fn definition(lang: Lang, node: Node, src: &str, parents: &[(String, &'static str)]) -> Option<Symbol> {
    if let Some((kind, name)) = special(lang, node, src) {
        return Some(make(node, node, src, parents, kind, name));
    }
    if !generic(lang) {
        return None;
    }
    let k = node.kind();
    // const handler = () => {...}, handler: function () {...}
    let value_is_function = |n: Node| {
        n.child_by_field_name("value")
            .is_some_and(|v| matches!(v.kind(), "arrow_function" | "function_expression" | "function"))
    };
    let mut kind = kind_of(k)?;
    if k == "variable_declarator" || k == "pair" {
        if !value_is_function(node) {
            return None;
        }
        kind = "function";
    }
    // C and C++ struct/class specifiers are also how a variable's type is
    // written: only the ones with a body define something
    // and `mod x;` in Rust only says where a module is
    if (k.ends_with("_specifier") || k == "mod_item") && node.child_by_field_name("body").is_none() {
        return None;
    }
    // Dart: the body follows the signature as its sibling
    let body =
        if k.ends_with("_signature") { node.next_named_sibling().filter(|b| b.kind().contains("body")) } else { None };
    let name = name_of(node, src)?;
    if name.is_empty() || name.len() > 120 {
        return None;
    }
    let inside_type =
        parents.last().is_some_and(|p| matches!(p.1, "class" | "struct" | "interface" | "trait" | "impl" | "enum"));
    if kind == "function" && inside_type {
        kind = "method";
    }
    Some(make(node, body.unwrap_or(node), src, parents, kind, name))
}

/// Whether the generic walk reads a language: the C-like shading languages
/// yes, the others name their definitions their own way.
fn generic(lang: Lang) -> bool {
    !matches!(lang, Lang::Wgsl | Lang::GraphQl | Lang::Proto | Lang::Dockerfile)
}

fn make(
    node: Node,
    last: Node,
    src: &str,
    parents: &[(String, &'static str)],
    kind: &'static str,
    name: String,
) -> Symbol {
    let text = &src[node.start_byte()..last.end_byte()];
    let qualified =
        parents.iter().map(|p| p.0.as_str()).chain(std::iter::once(name.as_str())).collect::<Vec<_>>().join(".");
    Symbol {
        signature: text.lines().next().unwrap_or("").trim().chars().take(120).collect(),
        doc: doc_above(node, src),
        calls: calls_in(last, src),
        body_hash: hash(&text.replacen(&name, "", 1)),
        about: String::new(),
        start: node.start_position().row + 1,
        end: last.end_position().row + 1,
        qualified,
        name,
        kind,
    }
}

/// Definitions of the languages that name theirs their own way: (kind, name).
fn special(lang: Lang, node: Node, src: &str) -> Option<(&'static str, String)> {
    let k = node.kind();
    let text = |n: Node| src[n.byte_range()].trim().to_string();
    let child = child_of;
    let field = |of: Node, f: &str| of.child_by_field_name(f).map(text);
    let named = |kind: &'static str| Some((kind, field(node, "name").or_else(|| child(node, &["name"]).map(text))?));
    match lang {
        Lang::Wgsl => match k {
            "function_declaration" => named("function"),
            "struct_declaration" => named("struct"),
            "type_alias_declaration" | "type_alias" => named("type"),
            "global_variable_declaration" => {
                let decl = child(node, &["variable_declaration"])?;
                let name = field(child(decl, &["variable_identifier_declaration"])?, "name")?;
                Some((if text(node).contains("@binding") { "binding" } else { "variable" }, name))
            }
            "global_constant_declaration" => {
                Some(("constant", field(child(node, &["variable_identifier_declaration"])?, "name")?))
            }
            _ => None,
        },
        Lang::GraphQl => match k {
            "object_type_definition"
            | "input_object_type_definition"
            | "union_type_definition"
            | "scalar_type_definition"
            | "object_type_extension" => named("type"),
            "interface_type_definition" => named("interface"),
            "enum_type_definition" => named("enum"),
            "operation_definition" => {
                let kind = match child(node, &["operation_type"]).map(text).as_deref() {
                    Some("mutation") => "mutation",
                    Some("subscription") => "subscription",
                    _ => "query",
                };
                Some((kind, child(node, &["name"]).map(text)?))
            }
            "fragment_definition" => Some(("fragment", child(child(node, &["fragment_name"])?, &["name"]).map(text)?)),
            _ => None,
        },
        Lang::Proto => {
            let (kind, name_kind) = match k {
                "message" => ("message", "message_name"),
                "enum" => ("enum", "enum_name"),
                "service" => ("service", "service_name"),
                "rpc" => ("method", "rpc_name"),
                _ => return None,
            };
            Some((kind, child(node, &[name_kind]).map(text)?))
        }
        Lang::Dockerfile if k == "from_instruction" => {
            Some(("stage", child(node, &["image_alias"]).or_else(|| child(node, &["image_spec"])).map(text)?))
        }
        _ => None,
    }
}

fn child_of<'t>(of: Node<'t>, kinds: &[&str]) -> Option<Node<'t>> {
    let mut c = of.walk();
    of.named_children(&mut c).find(|x| kinds.contains(&x.kind()))
}

/// `const X = ...;` and `alias X = ...;` at module level, which the WGSL
/// grammar reads as errors.
fn wgsl_in_error(node: Node, src: &str, parents: &[(String, &'static str)]) -> Vec<Symbol> {
    let mut out = Vec::new();
    let mut cursor = node.walk();
    let kids: Vec<Node> = node.named_children(&mut cursor).collect();
    for (i, n) in kids.iter().enumerate() {
        let kind = match &src[n.byte_range()] {
            "const" | "override" => "constant",
            "alias" => "type",
            _ => continue,
        };
        // the name right after it, on the same line
        let Some(name) = kids
            .get(i + 1)
            .filter(|m| m.kind() == "identifier" && m.start_position().row == n.start_position().row)
            .map(|m| src[m.byte_range()].to_string())
        else {
            continue;
        };
        let row = n.start_position().row;
        let line = src.lines().nth(row).unwrap_or("").trim();
        out.push(Symbol {
            qualified: parents
                .iter()
                .map(|p| p.0.as_str())
                .chain(std::iter::once(name.as_str()))
                .collect::<Vec<_>>()
                .join("."),
            kind,
            start: row + 1,
            end: row + 1,
            signature: line.chars().take(120).collect(),
            doc: String::new(),
            calls: Vec::new(),
            body_hash: hash(&line.replacen(&name, "", 1)),
            about: String::new(),
            name,
        });
    }
    out
}

/// A template string marked as code in another language: /* wgsl */ `...`
/// (a comment right before it) or wgsl`...` (a tag).
fn embedded_at(node: Node, src: &str, parents: &[(String, &'static str)]) -> Option<Embedded> {
    let mut at = node;
    let marker = match node.parent() {
        Some(p) if p.kind() == "call_expression" && p.child_by_field_name("arguments") == Some(node) => {
            at = p;
            let f = p.child_by_field_name("function")?;
            src[f.byte_range()].rsplit('.').next().unwrap_or("").to_string()
        }
        _ => {
            let c = node.prev_named_sibling().filter(|c| c.kind() == "comment")?;
            let t = src[c.byte_range()].trim_start_matches("/*").trim_end_matches("*/").trim().to_string();
            if t.is_empty() || t.contains(char::is_whitespace) || c.end_position().row + 1 < node.start_position().row {
                return None;
            }
            t
        }
    };
    let lang = Lang::of_marker(&marker);
    if lang.is_none() && !matches!(marker.to_ascii_lowercase().as_str(), "css" | "scss") {
        return None;
    }
    let mut holes = Vec::new();
    let mut cursor = node.walk();
    for c in node.named_children(&mut cursor) {
        if c.kind() == "template_substitution" {
            holes.push((c.start_byte(), c.end_byte()));
        }
    }
    // kept in a constant (const X = /* wgsl */ `...`), or returned from a function
    let holder = at.parent().filter(|d| d.kind() == "variable_declarator").and_then(|d| {
        let name = d.child_by_field_name("name").filter(|n| n.kind() == "identifier")?;
        let decl = d.parent().filter(|p| p.kind().contains("declaration")).unwrap_or(d);
        let decl = decl.parent().filter(|p| p.kind() == "export_statement").unwrap_or(decl);
        Some(Holder {
            name: src[name.byte_range()].to_string(),
            start: decl.start_position().row + 1,
            end: decl.end_position().row + 1,
            signature: src[decl.byte_range()].lines().next().unwrap_or("").trim().chars().take(120).collect(),
            doc: doc_above(decl, src),
        })
    });
    Some(Embedded {
        lang,
        marker,
        start: node.start_byte() + 1,
        end: node.end_byte().saturating_sub(1).max(node.start_byte() + 1),
        holes,
        holder,
        prefix: parents.iter().map(|p| p.0.as_str()).collect::<Vec<_>>().join("."),
    })
}

/// The symbols of an embedded string: the constant holding it, and what is
/// defined inside, under that constant's name (or the function's around it).
fn embedded_symbols(e: &Embedded, src: &str, depth: usize) -> Vec<Symbol> {
    let mut out = Vec::new();
    // the rest of the file blanked, so lines stay where they are
    let blank = |b: u8| if b == b'\n' { b'\n' } else { b' ' };
    let mut inner: Vec<u8> = src.bytes().map(blank).collect();
    inner[e.start..e.end].copy_from_slice(&src.as_bytes()[e.start..e.end]);
    let mut uses = Vec::new();
    for &(a, b) in &e.holes {
        let first: String = src[a..b]
            .trim_start_matches("${")
            .trim_start()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if !first.is_empty() && !uses.contains(&first) {
            uses.push(first);
        }
        // alone on its line it is a block of code (blanked); inside an
        // expression (array<f32, ${N}>) a name stands in for it
        let line_start = src[..a].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line_end = src[b..].find('\n').map(|p| p + b).unwrap_or(src.len());
        let alone = src[line_start..a].trim().is_empty() && src[b..line_end].trim().is_empty();
        for (i, (slot, &c)) in inner[a..b].iter_mut().zip(&src.as_bytes()[a..b]).enumerate() {
            *slot = if c == b'\n' {
                b'\n'
            } else if alone {
                b' '
            } else if i == 0 {
                b'x'
            } else {
                b'_'
            };
        }
    }
    let prefix = match &e.holder {
        Some(h) if e.prefix.is_empty() => h.name.clone(),
        Some(h) => format!("{}.{}", e.prefix, h.name),
        None => e.prefix.clone(),
    };
    if let Some(h) = &e.holder {
        out.push(Symbol {
            name: h.name.clone(),
            qualified: prefix.clone(),
            kind: match e.lang {
                Some(Lang::Wgsl | Lang::Glsl | Lang::Hlsl) => "shader",
                Some(_) => "query",
                None => "style",
            },
            start: h.start,
            end: h.end,
            signature: h.signature.clone(),
            doc: if h.doc.is_empty() { format!("{} code", e.marker) } else { h.doc.clone() },
            calls: uses,
            body_hash: hash(&src[e.start..e.end]),
            about: String::new(),
        });
    }
    let (Some(lang), Ok(inner)) = (e.lang, String::from_utf8(inner)) else { return out };
    for mut s in symbols_at(lang, &inner, depth + 1) {
        if !prefix.is_empty() {
            s.qualified = format!("{prefix}.{}", s.qualified);
        }
        out.push(s);
    }
    out
}

/// What kind of symbol a node kind defines, if any.
fn kind_of(k: &str) -> Option<&'static str> {
    if matches!(k, "variable_declarator" | "pair") {
        return Some("function"); // only when its value is a function, see above
    }
    let definition = ["_declaration", "_definition", "_item", "_spec", "_statement", "_specifier", "_signature"]
        .iter()
        .any(|e| k.ends_with(e))
        || matches!(k, "function" | "method" | "class" | "module" | "singleton_method" | "constructor_declaration");
    if !definition
        || k.contains("call")
        || k.contains("import")
        || k.contains("use_")
        || k.contains("expression")
        || k == "type_specifier"
    {
        return None;
    }
    let has = |w: &str| k.contains(w);
    Some(if has("method") || has("constructor") {
        "method"
    } else if has("function") || k == "test_declaration" {
        "function"
    } else if has("class") || has("object_declaration") || has("record") {
        "class"
    } else if has("struct") || has("union") {
        "struct"
    } else if has("interface") || has("protocol") {
        "interface"
    } else if has("trait") || has("mixin") || has("extension") {
        "trait"
    } else if has("enum") {
        "enum"
    } else if has("impl") {
        "impl"
    } else if has("module") || has("namespace") || k == "mod_item" {
        "module"
    } else if k == "type_spec" || k == "type_item" || has("type_alias") {
        "type"
    } else if k == "const_item" || k == "static_item" {
        "const" // Rust: tree_sitter_rust::LANGUAGE
    } else if k == "macro_definition" {
        "macro"
    } else {
        return None;
    })
}

/// A definition's name: its `name` field, else the name inside its
/// declarator (C, C++), else its type (Rust impl blocks).
fn name_of(node: Node, src: &str) -> Option<String> {
    let text = |n: Node| src[n.byte_range()].trim().to_string();
    if let Some(n) = node.child_by_field_name("name") {
        return Some(text(n));
    }
    let mut d = node.child_by_field_name("declarator");
    for _ in 0..6 {
        let Some(n) = d else { break };
        if n.kind().contains("identifier") {
            return Some(text(n));
        }
        d = n.child_by_field_name("declarator").or_else(|| n.child_by_field_name("name"));
    }
    if node.kind() == "impl_item" {
        let ty = node.child_by_field_name("type").map(text)?;
        return Some(match node.child_by_field_name("trait").map(text) {
            Some(t) => format!("{ty} {t}"),
            None => ty,
        });
    }
    let mut cursor = node.walk();
    let found = node
        .named_children(&mut cursor)
        .take(4)
        .find(|c| c.kind().contains("identifier") || matches!(c.kind(), "name" | "word" | "constant"));
    found.map(text)
}

/// The comment just above a definition, its first line.
fn doc_above(node: Node, src: &str) -> String {
    let mut prev = node.prev_named_sibling();
    let mut lines = Vec::new();
    while let Some(p) = prev {
        if !p.kind().contains("comment") || node.start_position().row > p.end_position().row + 1 + lines.len() {
            break;
        }
        lines.push(src[p.byte_range()].to_string());
        prev = p.prev_named_sibling();
    }
    let first = lines.last().map(String::as_str).unwrap_or("");
    first
        .trim_start_matches(['/', '*', '#', '-', ' ', '!'])
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(160)
        .collect()
}

/// The names a definition calls (the last part: "Record" for audit.Record).
fn calls_in(node: Node, src: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut stack = vec![node];
    while let Some(n) = stack.pop() {
        let k = n.kind();
        // a type made is its constructor called: new Refund(), new Refund<T>()
        // (JS, TS, Java, C#), Refund { .. } (Rust), Refund{..} (Go)
        let made =
            matches!(k, "new_expression" | "object_creation_expression" | "struct_expression" | "composite_literal");
        if made || (k.contains("call") || k.contains("invocation")) && !k.contains("signature") {
            let callee = n
                .child_by_field_name("function")
                .or_else(|| n.child_by_field_name("constructor"))
                .or_else(|| made.then(|| n.child_by_field_name("type")).flatten())
                .or_else(|| n.child_by_field_name("name"))
                .or_else(|| n.child_by_field_name("method"))
                .or_else(|| n.named_child(0));
            if let Some(c) = callee {
                // without its generic arguments: parse::<T>, Vec::<u8>::new keeps new
                let full = &src[c.byte_range()];
                let t = strip_generics(full);
                let t = t.trim_end_matches(':');
                let last = t.rsplit(['.', ':', '>', ' ']).next().unwrap_or(t).trim();
                if !last.is_empty()
                    && last.len() < 60
                    && last.chars().all(|c| c.is_alphanumeric() || c == '_')
                    && !out.iter().any(|o| o == last)
                {
                    out.push(last.to_string());
                    if out.len() == MAX_CALLS {
                        break;
                    }
                }
            }
        }
        let mut cursor = n.walk();
        stack.extend(n.named_children(&mut cursor));
    }
    out
}

/// A callee without its generic arguments: "parse::<u32>" is "parse::",
/// "Vec::<u8>::new" is "Vec::::new", "make<T>" is "make".
fn strip_generics(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut depth = 0usize;
    for c in t.chars() {
        match c {
            '<' => depth += 1,
            '>' if depth > 0 => depth -= 1,
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out
}

/// FNV-1a over the text without spacing: stable across versions of pg.
pub fn hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes().filter(|b| !b.is_ascii_whitespace()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[cfg(all(test, paragma_local))]
#[rustfmt::skip]
#[path = "../../unit/src/code/parse.rs"]
mod tests;
