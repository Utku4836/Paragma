//! Symbols of a source file: functions, methods, classes, structs,
//! interfaces, enums... with their lines, signature, the names they call,
//! and a hash of their body that survives a rename or a move.
//!
//! One walk serves every grammar: tree-sitter grammars name their
//! definition nodes alike (function_declaration, class_definition,
//! function_item, method_declaration, ...) and give them a `name` field.

use super::lang::Lang;
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
}

/// Calls kept per symbol.
const MAX_CALLS: usize = 60;

pub fn symbols(lang: Lang, source: &str) -> Vec<Symbol> {
    let mut parser = Parser::new();
    if parser.set_language(&lang.grammar()).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(source, None) else { return Vec::new() };
    let mut out = Vec::new();
    walk(tree.root_node(), source, &mut Vec::new(), &mut out, 0);
    out
}

fn walk(node: Node, src: &str, parents: &mut Vec<(String, &'static str)>, out: &mut Vec<Symbol>, depth: usize) {
    if depth > 200 {
        return;
    }
    let found = definition(node, src, parents);
    if let Some(s) = &found {
        parents.push((s.name.clone(), s.kind));
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        walk(child, src, parents, out, depth + 1);
    }
    if let Some(s) = found {
        parents.pop();
        out.push(s);
    }
}

/// The symbol a node defines, if it is a definition.
fn definition(node: Node, src: &str, parents: &[(String, &'static str)]) -> Option<Symbol> {
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
    let last = body.unwrap_or(node);
    let text = &src[node.start_byte()..last.end_byte()];
    let qualified =
        parents.iter().map(|p| p.0.as_str()).chain(std::iter::once(name.as_str())).collect::<Vec<_>>().join(".");
    Some(Symbol {
        signature: text.lines().next().unwrap_or("").trim().chars().take(120).collect(),
        doc: doc_above(node, src),
        calls: calls_in(last, src),
        body_hash: hash(&text.replacen(&name, "", 1)),
        start: node.start_position().row + 1,
        end: last.end_position().row + 1,
        qualified,
        name,
        kind,
    })
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
        if (k.contains("call") || k.contains("invocation")) && !k.contains("signature") {
            let callee = n
                .child_by_field_name("function")
                .or_else(|| n.child_by_field_name("name"))
                .or_else(|| n.child_by_field_name("method"))
                .or_else(|| n.named_child(0));
            if let Some(c) = callee {
                let t = &src[c.byte_range()];
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

/// FNV-1a over the text without spacing: stable across versions of pg.
fn hash(text: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in text.bytes().filter(|b| !b.is_ascii_whitespace()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(lang: Lang, src: &str) -> Vec<String> {
        let mut s: Vec<String> =
            symbols(lang, src).into_iter().map(|s| format!("{} {}", s.kind, s.qualified)).collect();
        s.sort();
        s
    }

    #[test]
    fn every_language_gives_its_symbols() {
        let cases: &[(Lang, &str, &[&str])] = &[
            (
                Lang::Go,
                "package p\n// Refund pays back.\nfunc Refund(a int) error { return audit.Record(a) }\ntype Cart struct{}\nfunc (c *Cart) Total() int { return 0 }\n",
                &["function Refund", "method Total", "type Cart"],
            ),
            (
                Lang::Python,
                "class Cart:\n    def total(self):\n        return sum(1)\n\ndef refund():\n    pass\n",
                &["class Cart", "function refund", "method Cart.total"],
            ),
            (
                Lang::Rust,
                "struct Cart;\nimpl Cart {\n    fn total(&self) -> i64 { helper() }\n}\nfn refund() {}\n",
                &["function refund", "impl Cart", "method Cart.total", "struct Cart"],
            ),
            (
                Lang::TypeScript,
                "export class Cart { total(): number { return 0 } }\nexport function refund() {}\nconst pay = () => 1;\ninterface Item { id: string }\n",
                &["class Cart", "function pay", "function refund", "interface Item", "method Cart.total"],
            ),
            (
                Lang::JavaScript,
                "function refund() { audit.record() }\nclass Cart { total() { return 0 } }\n",
                &["class Cart", "function refund", "method Cart.total"],
            ),
            (
                Lang::Java,
                "class Cart { int total() { return 0; } Cart() {} }\n",
                &["class Cart", "method Cart.Cart", "method Cart.total"],
            ),
            (Lang::CSharp, "class Cart { int Total() { return 0; } }\n", &["class Cart", "method Cart.Total"]),
            (
                Lang::C,
                "int refund(int a) { return a; }\nstruct cart { int n; };\n",
                &["function refund", "struct cart"],
            ),
            (
                Lang::Cpp,
                "class Cart { public: int total() { return 0; } };\nint refund() { return 1; }\n",
                &["class Cart", "function refund", "method Cart.total"],
            ),
            (Lang::Ruby, "class Cart\n  def total\n    0\n  end\nend\n", &["class Cart", "method Cart.total"]),
            (
                Lang::Php,
                "<?php\nclass Cart { function total() { return 0; } }\nfunction refund() {}\n",
                &["class Cart", "function refund", "method Cart.total"],
            ),
            (
                Lang::Kotlin,
                "class Cart { fun total(): Int = 0 }\nfun refund() {}\n",
                &["class Cart", "function refund", "method Cart.total"],
            ),
            (
                Lang::Swift,
                "class Cart { func total() -> Int { return 0 } }\nfunc refund() {}\n",
                &["class Cart", "function refund", "method Cart.total"],
            ),
            (
                Lang::Lua,
                "function refund() end\nlocal function total() end\nfunction M.pay() end\n",
                &["function M.pay", "function refund", "function total"],
            ),
            (
                Lang::Luau,
                "function refund() end\nlocal function total(): number return 0 end\n",
                &["function refund", "function total"],
            ),
            (
                Lang::GdScript,
                "extends Node\nfunc refund():\n\tpass\nclass Cart:\n\tfunc total():\n\t\treturn 0\n",
                &["class Cart", "function refund", "method Cart.total"],
            ),
            (Lang::Bash, "refund() { echo hi; }\nfunction total { echo 0; }\n", &["function refund", "function total"]),
            (
                Lang::Zig,
                "fn refund() void {}\npub fn total() i32 { return 0; }\n",
                &["function refund", "function total"],
            ),
            (
                Lang::Dart,
                "class Cart { int total() { return 0; } }\nvoid refund() {}\n",
                &["class Cart", "function refund", "method Cart.total"],
            ),
        ];
        let mut wrong = Vec::new();
        for (lang, src, want) in cases {
            let got = names(*lang, src);
            if got != want.iter().map(|s| s.to_string()).collect::<Vec<_>>() {
                wrong.push(format!("{}: {got:?}", lang.name()));
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn lines_doc_calls_and_a_hash_that_survives_a_rename() {
        let a = symbols(
            Lang::Go,
            "package p\n\n// Refund pays back.\nfunc Refund(a int) error {\n\treturn audit.Record(a)\n}\n",
        );
        let b = symbols(Lang::Go, "package p\nfunc PayBack(a int) error {\n    return audit.Record(a)\n}\n");
        assert_eq!((a[0].start, a[0].end), (4, 6));
        assert_eq!(a[0].doc, "Refund pays back.");
        assert_eq!(a[0].calls, vec!["Record"]);
        assert_eq!(a[0].body_hash, b[0].body_hash);
    }
}
