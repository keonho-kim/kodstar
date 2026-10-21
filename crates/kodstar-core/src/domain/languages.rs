// Purpose: 언어 식별 값과 문자열 변환.
// Pattern: Simple Module.
// Usage: 설정·grammar 선택·해석에서 사용.
// Related: crates/kodstar-core/src/infrastructure/languages/mod.rs.

//! Stable language identifiers; recognition and semantic capabilities remain separate.

use std::fmt;
use std::str::FromStr;

use super::diagnostics::{Error, Result};

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    JavaScript,
    TypeScript,
    Rust,
    Go,
    Java,
    C,
    Cpp,
    CSharp,
    Php,
    Kotlin,
    Swift,
    Ruby,
    Scala,
    Zig,
    Elixir,
    Erlang,
    CommonLisp,
    Pascal,
    Vue,
    Svelte,
    Astro,
    Ejs,
    Blade,
    Razor,
}

impl Language {
    pub const ALL: [Self; 25] = [
        Self::Python,
        Self::JavaScript,
        Self::TypeScript,
        Self::Rust,
        Self::Go,
        Self::Java,
        Self::C,
        Self::Cpp,
        Self::CSharp,
        Self::Php,
        Self::Kotlin,
        Self::Swift,
        Self::Ruby,
        Self::Scala,
        Self::Zig,
        Self::Elixir,
        Self::Erlang,
        Self::CommonLisp,
        Self::Pascal,
        Self::Vue,
        Self::Svelte,
        Self::Astro,
        Self::Ejs,
        Self::Blade,
        Self::Razor,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::TypeScript => "typescript",
            Self::Rust => "rust",
            Self::Go => "go",
            Self::Java => "java",
            Self::C => "c",
            Self::Cpp => "cpp",
            Self::CSharp => "csharp",
            Self::Php => "php",
            Self::Kotlin => "kotlin",
            Self::Swift => "swift",
            Self::Ruby => "ruby",
            Self::Scala => "scala",
            Self::Zig => "zig",
            Self::Elixir => "elixir",
            Self::Erlang => "erlang",
            Self::CommonLisp => "commonlisp",
            Self::Pascal => "pascal",
            Self::Vue => "vue",
            Self::Svelte => "svelte",
            Self::Astro => "astro",
            Self::Ejs => "ejs",
            Self::Blade => "blade",
            Self::Razor => "razor",
        }
    }

    pub const fn is_component(self) -> bool {
        matches!(
            self,
            Self::Vue | Self::Svelte | Self::Astro | Self::Ejs | Self::Blade | Self::Razor
        )
    }

    pub const fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Python => &["py"],
            Self::JavaScript => &["js", "jsx", "mjs", "cjs"],
            Self::TypeScript => &["ts", "tsx", "mts", "cts"],
            Self::Rust => &["rs"],
            Self::Go => &["go"],
            Self::Java => &["java"],
            Self::C => &["c", "h"],
            Self::Cpp => &["cpp", "cc", "cxx", "hpp", "hh", "hxx", "h"],
            Self::CSharp => &["cs"],
            Self::Php => &["php"],
            Self::Kotlin => &["kt", "kts"],
            Self::Swift => &["swift"],
            Self::Ruby => &["rb", "rake"],
            Self::Scala => &["scala"],
            Self::Zig => &["zig"],
            Self::Elixir => &["ex", "exs"],
            Self::Erlang => &["erl", "hrl", "escript"],
            Self::CommonLisp => &["lisp", "cl", "lsp", "asd"],
            Self::Pascal => &["pas", "pp", "dpr", "dpk", "lpr", "inc"],
            Self::Vue => &["vue"],
            Self::Svelte => &["svelte"],
            Self::Astro => &["astro"],
            Self::Ejs => &["ejs"],
            Self::Blade => &["blade.php"],
            Self::Razor => &["razor", "cshtml"],
        }
    }

    pub const fn case_sensitive(self) -> bool {
        !matches!(self, Self::Pascal)
    }

    pub(crate) fn lookup_key(self, name: &str) -> String {
        if self == Self::CommonLisp {
            super::resolve::lisp::key(name)
        } else if self == Self::Rust {
            name.replace("r#", "")
        } else if self == Self::Kotlin {
            name.replace('`', "")
        } else if self.case_sensitive() {
            name.to_owned()
        } else {
            name.to_ascii_lowercase()
        }
    }
}

impl fmt::Display for Language {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Language {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        Self::ALL
            .into_iter()
            .find(|language| language.as_str() == value)
            .ok_or_else(|| Error::argument(format!("unsupported language override: {value}")))
    }
}
