//! The languages pg reads code in, by file extension (or name). Files in
//! other languages still show in `pg tree`, without their symbols.
//!
//! Most have a tree-sitter grammar. SQL, Markdown, YAML and the script
//! blocks of Vue and Svelte files are read without one (code/text.rs).

use tree_sitter::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Go,
    Rust,
    Java,
    Kotlin,
    CSharp,
    Php,
    Ruby,
    Bash,
    C,
    Cpp,
    Zig,
    Swift,
    Dart,
    Lua,
    Luau,
    GdScript,
    Wgsl,
    Glsl,
    Hlsl,
    Sql,
    GraphQl,
    Proto,
    Dockerfile,
    Markdown,
    Yaml,
    Vue,
    Svelte,
}

impl Lang {
    pub fn of(path: &str) -> Option<Lang> {
        let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
        let lower = name.to_ascii_lowercase();
        if lower == "dockerfile" || lower == "containerfile" || lower.starts_with("dockerfile.") {
            return Some(Lang::Dockerfile);
        }
        let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
        Some(match ext.as_str() {
            "py" | "pyi" => Lang::Python,
            "js" | "jsx" | "mjs" | "cjs" => Lang::JavaScript,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "go" => Lang::Go,
            "rs" => Lang::Rust,
            "java" => Lang::Java,
            "kt" | "kts" => Lang::Kotlin,
            "cs" => Lang::CSharp,
            "php" => Lang::Php,
            "rb" => Lang::Ruby,
            "sh" | "bash" => Lang::Bash,
            "c" | "h" => Lang::C,
            "cc" | "cpp" | "cxx" | "hpp" | "hh" | "hxx" => Lang::Cpp,
            "zig" => Lang::Zig,
            "swift" => Lang::Swift,
            "dart" => Lang::Dart,
            "lua" => Lang::Lua,
            "luau" => Lang::Luau,
            "gd" => Lang::GdScript,
            "wgsl" => Lang::Wgsl,
            "glsl" | "vert" | "frag" | "comp" | "geom" | "tesc" | "tese" | "rgen" | "rchit" | "rmiss" => Lang::Glsl,
            "hlsl" | "hlsli" | "fxh" => Lang::Hlsl,
            "sql" => Lang::Sql,
            "graphql" | "graphqls" | "gql" => Lang::GraphQl,
            "proto" => Lang::Proto,
            "dockerfile" | "containerfile" => Lang::Dockerfile,
            "md" | "markdown" | "mdx" => Lang::Markdown,
            "yml" | "yaml" => Lang::Yaml,
            "vue" => Lang::Vue,
            "svelte" => Lang::Svelte,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Lang::Python => "python",
            Lang::JavaScript => "javascript",
            Lang::TypeScript => "typescript",
            Lang::Tsx => "tsx",
            Lang::Go => "go",
            Lang::Rust => "rust",
            Lang::Java => "java",
            Lang::Kotlin => "kotlin",
            Lang::CSharp => "c#",
            Lang::Php => "php",
            Lang::Ruby => "ruby",
            Lang::Bash => "bash",
            Lang::C => "c",
            Lang::Cpp => "c++",
            Lang::Zig => "zig",
            Lang::Swift => "swift",
            Lang::Dart => "dart",
            Lang::Lua => "lua",
            Lang::Luau => "luau",
            Lang::GdScript => "gdscript",
            Lang::Wgsl => "wgsl",
            Lang::Glsl => "glsl",
            Lang::Hlsl => "hlsl",
            Lang::Sql => "sql",
            Lang::GraphQl => "graphql",
            Lang::Proto => "protobuf",
            Lang::Dockerfile => "dockerfile",
            Lang::Markdown => "markdown",
            Lang::Yaml => "yaml",
            Lang::Vue => "vue",
            Lang::Svelte => "svelte",
        }
    }

    /// The language of code embedded in another file's strings, by the
    /// marker it is written with: /* wgsl */ `...` or wgsl`...`.
    pub fn of_marker(marker: &str) -> Option<Lang> {
        Some(match marker.trim().to_ascii_lowercase().as_str() {
            "wgsl" => Lang::Wgsl,
            "glsl" | "vert" | "frag" => Lang::Glsl,
            "hlsl" => Lang::Hlsl,
            "sql" => Lang::Sql,
            "graphql" | "gql" => Lang::GraphQl,
            _ => return None,
        })
    }

    /// Its tree-sitter grammar; None for the languages read without one.
    pub fn grammar(self) -> Option<Language> {
        Some(match self {
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Go => tree_sitter_go::LANGUAGE.into(),
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::Java => tree_sitter_java::LANGUAGE.into(),
            Lang::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
            Lang::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
            Lang::Php => tree_sitter_php::LANGUAGE_PHP.into(),
            Lang::Ruby => tree_sitter_ruby::LANGUAGE.into(),
            Lang::Bash => tree_sitter_bash::LANGUAGE.into(),
            Lang::C => tree_sitter_c::LANGUAGE.into(),
            Lang::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Lang::Zig => tree_sitter_zig::LANGUAGE.into(),
            Lang::Swift => tree_sitter_swift::LANGUAGE.into(),
            Lang::Dart => tree_sitter_dart::LANGUAGE.into(),
            Lang::Lua => tree_sitter_lua::LANGUAGE.into(),
            Lang::Luau => tree_sitter_luau::LANGUAGE.into(),
            Lang::GdScript => tree_sitter_gdscript::LANGUAGE.into(),
            Lang::Wgsl => tree_sitter_wgsl_bevy::LANGUAGE.into(),
            Lang::Glsl => tree_sitter_glsl::LANGUAGE_GLSL.into(),
            // C++ reads HLSL well enough (its constant buffers: code/text.rs)
            Lang::Hlsl => tree_sitter_cpp::LANGUAGE.into(),
            Lang::GraphQl => tree_sitter_graphql::LANGUAGE.into(),
            Lang::Proto => tree_sitter_proto::LANGUAGE.into(),
            Lang::Dockerfile => tree_sitter_containerfile::LANGUAGE.into(),
            Lang::Sql | Lang::Markdown | Lang::Yaml | Lang::Vue | Lang::Svelte => return None,
        })
    }
}
