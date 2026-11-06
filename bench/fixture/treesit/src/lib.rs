//! Counts what a piece of Rust source defines.

/// How many functions `source` defines: free functions and methods.
pub fn count_functions(source: &str) -> usize {
    let _ = source;
    todo!("parse the source with tree-sitter")
}
