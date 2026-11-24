<p align="center">
  <img src="docs/assets/kodstar-boot-logo.png" width="680" alt="kod with a raised gold outline star, with kodstar in small gray type below" />
</p>

<p align="center">
  <strong>Follow the code. Keep the history.</strong><br />
  A local CLI for exploring code relationships and keeping source history.
</p>

<p align="center">
  <a href="rust-toolchain.toml"><img src="https://img.shields.io/badge/Rust-1.99.1-D6AA33?style=flat-square&amp;logo=rust&amp;logoColor=white&amp;labelColor=161B18" alt="Rust toolchain 1.99.1" /></a>
  <a href="#supported-languages"><img src="https://img.shields.io/badge/platform-macOS%20%2B%20Linux-D6AA33?style=flat-square&amp;labelColor=161B18" alt="macOS and Linux" /></a>
  <a href="#features"><img src="https://img.shields.io/badge/analysis-local-D6AA33?style=flat-square&amp;labelColor=161B18" alt="Local analysis" /></a>
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#command-reference">Commands</a> ·
  <a href="#examples">Examples</a> ·
  <a href="#supported-languages">Languages</a> ·
  <a href="#documentation-and-development">Documentation</a>
</p>

---

**kod<sup>☆</sup>** is a Rust CLI for exploring source-code relationships.
It extracts syntax facts with official **Tree-sitter** parsers, enriches C/C++ calls with embedded **Clang**, and stores relationships and analysis state in **SQLite**.
Follow calls, references, and dependencies, while automatic checkpoints preserve changes observed as you use the CLI.

## Features

- **Explore code relationships** — Find symbol definitions, call targets, incoming references, and dependency paths.
- **Inspect change impact** — Trace the impact of a symbol or changes since a saved checkpoint.
- **Incremental synchronization** — After initialization, regular commands check for changes and update the analysis.
- **Automatic local history** — Name checkpoints, compare changes, restore files, and work with local branches.
- **Project exploration** — Find dependencies across directories and packages, trace import cycles, and search by identifier or path words.
- **Configurable resource budgets** — Set worker counts and memory budgets, backed by a disk-based index.
- **Tool integration** — Use text, JSON, JSONL, or TSV output, file watching, and stdio sessions.

## Quick start

### 1. Install

Source builds need `rustup`, the [pinned Rust toolchain](rust-toolchain.toml), C/C++ build tools, and **LLVM/Clang 23.1.4 development headers and static libraries**. Set `LLVM_CONFIG_PATH` to the matching `llvm-config` when needed. The installed CLI includes its analyzer and builtin headers; it runs without an external Clang executable or extracted runtime.
See [development setup](docs/development.md#개발-환경) for platform-specific requirements.

```bash
git clone https://github.com/keonho-kim/kodstar.git
cd kodstar
./install.sh
```

The installer builds the CLI in a temporary directory, installs the executable at `$HOME/.local/bin/kodstar`,
and prepares `$HOME/.kodstar` with installation metadata. It removes temporary build output when it exits.
Project history and analysis databases are created when you run `kodstar init`.

The installer also configures your shell's PATH.
Open a new terminal, or run the PATH command printed by the installer, to use `kodstar` from any directory.
To update an existing installation, get the latest source and run `./install.sh` again.

<details>
<summary>Installation path and shell setup</summary>

PATH setup covers sh, Bash, and Zsh. Fish is configured when it is your shell or its configuration file exists.
Installation is separate from project initialization. To install an existing executable, use
`./install.sh --binary /path/to/kodstar`; relative paths are resolved from your current directory.
The source checkout, Rust toolchain, and Cargo's shared dependency download cache are managed separately from the installed CLI.

</details>

### 2. Initialize a project

Enter the project you want to analyze and create its history repository and initial index together.

```bash
cd /work/your-project
kodstar init .
```

The first initialization shows the actual target path and asks for confirmation. Initialization at `/` is refused.
For automation, review the path before using `kodstar init /work/your-project --yes`.

### 3. Explore your code

Replace the example file paths and function names with ones from your project.
Inside a project, kod<sup>☆</sup> selects the registered ancestor root, so you can use relative paths.

```bash
# List symbols defined in a file
kodstar symbols src/api.rs

# Inspect caller's dependencies and how they were resolved
kodstar deps src/api.rs caller
kodstar explain src/api.rs caller

# Survey the project and find where to start
kodstar overview
kodstar search 'order storage' --ranked

# Search definitions by exact name
kodstar search target
```

## Command reference

Replace `<file>`, `<symbol>`, and other placeholders with your own values. Square brackets mark optional arguments.
A `<revision>` is a checkpoint ID or a saved checkpoint name.

### Initialize and explore

| Command | Purpose |
|---|---|
| `kodstar init [path]` | Initialize local history and analyze the project; update an existing project. |
| `kodstar init [path] --full` | Re-extract all source files into a new analysis generation. |
| `kodstar init [path] --verify` | Verify source content hashes during synchronization. |
| `kodstar symbols <file>` | List definitions in a file. |
| `kodstar list <file>` | List directly connected files. |
| `kodstar search <name>` | Find definitions by exact name. |
| `kodstar search <prefix> --prefix` | Find definitions whose names start with a prefix. |
| `kodstar search <words> --ranked` | Rank definitions using identifier, qualified name, and path words. Show direct relationships for the leading candidates. |
| `kodstar overview` | Summarize directories, package dependencies, frequently used symbols, and import cycles. |
| `kodstar related <file> <symbol>` | Show incoming and outgoing relationships. |
| `kodstar deps <file> <symbol>` | Show outgoing dependencies. |
| `kodstar refs <file> <symbol>` | Show incoming references. |
| `kodstar explain <file> <symbol>` | Explain definitions, relationship evidence, and explicit ADR or RFC references. |
| `kodstar impact <file> [symbol]` | Find static impact candidates for a file or symbol. |
| `kodstar impact --since <revision>` | Inspect the impact of changes since a checkpoint. |
| `kodstar path <file::symbol> <file::symbol>` | Find a shortest directed relationship path between two symbols. |
| `kodstar path <file::symbol> <file::symbol> --undirected` | Find a shortest path that can change direction, including paths through a shared dependency. |

### Local history and branches

| Command | Purpose |
|---|---|
| `kodstar history` | List checkpoints reachable from the current branch, newest first. |
| `kodstar history save <name>` | Capture the current source state and name its checkpoint. |
| `kodstar history diff` | Compare the current checkpoint with its parent. |
| `kodstar history diff --from <revision> --to <revision>` | Compare two saved checkpoints; both options are required together. |
| `kodstar history diff --graph [--from <revision> --to <revision>]` | Show changes in relationship targets, resolution, and counts. |
| `kodstar history restore <revision> [path]` | Restore all tracked files or a selected path, preserving current changes first. |
| `kodstar history upgrade` | Verify, upgrade, and repack history storage. |
| `kodstar branch` | List local branches and their checkpoints. |
| `kodstar branch create <name>` | Preserve current changes and create a branch without switching to it. |
| `kodstar branch switch <name>` | Preserve current changes and switch branches. |
| `kodstar branch delete <name>` | Delete a branch reference other than the current branch. |

### Status, diagnostics, and caches

| Command | Purpose |
|---|---|
| `kodstar status` | Show project, checkpoint, analysis generation, freshness, and coverage. |
| `kodstar doctor` | Diagnose history and index integrity, compatibility, and analysis failures. |
| `kodstar languages` | Show actual language capabilities and limitations. |
| `kodstar cache list` | List cache locations, sizes, and usage state. |
| `kodstar cache clean` | Remove the current project's inactive analysis cache. |
| `kodstar cache clean --all` | Remove inactive analysis caches for all projects. |
| `kodstar cache gc` | Remove eligible caches according to retention and size policies. |
| `kodstar cache gc --dry-run` | Preview cache cleanup candidates. |

### Watching, integration, and installation

| Command | Purpose |
|---|---|
| `kodstar watch` | Watch saved files and synchronize changes until stopped. |
| `kodstar serve --stdio` | Serve structured requests over stdin/stdout for external tools. |
| `kodstar uninstall` | Remove the installation, managed settings, caches, and all registered project histories after confirmation. |
| `kodstar --help` | Show the command list and general help. |
| `kodstar <command> --help` | Show detailed behavior, arguments, options, and examples; `-h` gives a summary. |
| `kodstar --version` | Show CLI and engine versions. |

Query commands support `--format text|json|jsonl|tsv`, `--rev <revision>` for saved analysis,
and `--no-refresh` to skip change checks. Use `--root <path>` on project commands to select a project explicitly.
For subcommands, put options after the subcommand, for example `kodstar history diff --root /work/your-project`.

Progress and stage timings are printed to stderr by default in terminals, pipes, and redirected runs. They show source capture,
parsing and native analysis, relationship resolution, and index publication. JSON stdout stays
machine-readable. Native analysis reports the affected file and reason when context is missing
or a file task reaches a resource limit. Memory is scheduled by phase; native tasks wait for
workspace and release it when their file finishes.
Use `kodstar --verbose false related src/api.rs caller` to suppress routine progress and timings,
or `kodstar --verbose true related src/api.rs caller` to enable them explicitly. The option is global
and also works after a subcommand. Errors and analysis warnings remain visible; result output and
structured stdio progress frames are unaffected. No log files are created.
See the [CLI specification](kodstar-spec.md#7-cli-명령어-전체) for complete behavior and limits.

## Examples

### Follow references and change impact

```bash
# Find references to target, including individual use sites
kodstar refs src/api.rs target --sites

# Inspect the impact of changing target
kodstar impact src/api.rs target

# Produce structured results for other tools
kodstar deps src/api.rs caller --format json
```

Results distinguish resolved targets, candidates, ambiguity, unresolved references, and index freshness.
Outside the project, select it with `--root /work/your-project`.

### Keep local history as you work

Regular commands check for source changes and preserve them in checkpoints.
When nothing has changed, no checkpoint is added.

```bash
# Name the current checkpoint
kodstar history save baseline

# After editing code, run another query to synchronize changes
kodstar search target

# Name the updated checkpoint, compare, and inspect change impact
kodstar history save after-edit
kodstar history diff --from baseline --to after-edit
kodstar history diff --graph --from baseline --to after-edit
kodstar impact --since baseline

# Query a saved checkpoint and restore a file
kodstar symbols src/api.rs --rev baseline
kodstar history restore baseline src/api.rs
```

Use local branches to keep separate lines of work.

```bash
kodstar branch create experiment
kodstar branch switch experiment
kodstar branch
```

Restore and branch switching preserve current changes first. When resuming an interrupted operation,
kod<sup>☆</sup> stops if it detects conflicting external edits. See the [local history contract](kodstar-spec.md#7-cli-명령어-전체) for details.

### Watch files and connect external tools

```bash
# Watch changes to saved files
kodstar watch

# Start a stdio session for external tools
kodstar serve --stdio
```

JSON, JSONL, TSV, and stdio contracts are defined in the [specification](kodstar-spec.md).

## Supported languages

kod<sup>☆</sup> provides selected static analysis rules for **18 languages** and **7 HTML, component, and template formats**.

| Category | Languages and formats |
|---|---|
| Systems and native | Rust, Go, C, C++, Zig |
| Application languages | Java, Kotlin, Scala, C#, Swift |
| Dynamic languages and web | Python, JavaScript, TypeScript, Ruby, PHP |
| BEAM·Lisp | Elixir, Erlang, Common Lisp |
| HTML, components and templates | HTML, Vue, Svelte, Astro, EJS, Blade, Razor |

Analysis uses static syntax, explicit bindings, and language-specific resolution rules.
Run `kodstar languages --format json` for each language's capabilities and limitations,
and see the [language reference](kodstar-spec.md#3-지원-언어) for supported inputs.

React uses the JSX/TSX analyzer. Hono route and middleware registrations connect to handlers and JSX or static HTML responses. Static htmx attributes in HTML, JSX, and supported templates become HTTP route candidates; `hx-target="#id"` can reference a unique ID in the same file.

```bash
kodstar related index.html index.html --relation http_request --include-candidates
kodstar deps src/server.tsx show --relation renders
kodstar --licenses
```

HTTP matches describe static dependencies. They do not establish the deployed origin, middleware execution order, dynamic routes, or browser DOM state. C/C++ semantic gaps are reported by `doctor`; missing project SDKs and unsupported compilation context retain the available syntax analysis.

Tree-sitter builds the base graph and recovers reliable declarations from partially parsed files.
Uncertain scopes and providers retain candidate relationships. Partial syntax coverage returns
`complete: false`; `kodstar doctor` lists affected files. Embedded Clang adds C/C++ semantic evidence;
missing SDKs or native task limits preserve the syntax graph and report separate coverage counts.
Unreadable inputs contribute no stale symbols. Source checkpoints preserve their original bytes.

Supported platforms are **macOS and Linux**. **Windows is not supported.**

## Storage and maintenance

| Data | Location | Purpose |
|---|---|---|
| Project history | `<project>/.kodstar/repository.sqlite` | Preserved source content, checkpoints, names, and branches |
| Analysis cache | `$HOME/.kodstar` | Rebuildable relationship indexes and cache management metadata |

`cache clean` and `cache gc` preserve source history.
To configure the memory budget and worker count:

```bash
kodstar init . --jobs 2 --memory-budget 128MiB
kodstar doctor
```

<details>
<summary>Upgrade existing history and rebuild the cache</summary>

`kodstar history upgrade` explicitly converts history formats 1–4 to format 5.
For format 5, it repacks stored content and reclaims free pages while preserving checkpoints, source content, names, and branches.
History uses Zstd level 1 compression, bounded previous-content dictionaries, and FastCDC chunking for large files.
If the analysis cache format is incompatible, explicitly clean it and initialize again.

```bash
kodstar history upgrade
kodstar cache clean --root .
kodstar init .
```

See [storage consistency](kodstar-spec.md#16-저장-일관성과-장애-시-동작) for storage formats and recovery behavior,
and the [development guide](docs/development.md) for build and verification procedures.

</details>

<details>
<summary>Uninstall and remove data</summary>

**After confirmation, uninstall removes the executable, managed PATH entries, caches, default global configuration,
and checkpoint history for every registered project.**
It preserves source files, other shell configuration, development checkouts, build outputs, and the Rust toolchain.

```bash
kodstar uninstall
```

For automation, use `kodstar uninstall --yes`. Active sessions, modified installation records, or unknown data
cause uninstall to stop with a diagnostic. If a project has moved, use kod<sup>☆</sup> once at its new location to update its registration before uninstalling.

</details>

## Documentation and development

Detailed documentation is in Korean.

| Document | Contents |
|---|---|
| [Specification](kodstar-spec.md) | Product scope, commands, output, freshness, and stdio contracts |
| [Architecture](docs/architecture.md) | Layers, source layout, execution flow, and state and resource ownership |
| [Development guide](docs/development.md) | Setup, builds, configuration, verification, and dependency management |
| [Development principles](AGENTS.md) | Design review, code, comments, and change procedures |

The engine lives in [`kodstar-core`](crates/kodstar-core), and the CLI in [`kodstar-cli`](crates/kodstar-cli).
Read the development principles before making changes, and follow the [verification procedure](docs/development.md#변경-검증).

```bash
cargo build --workspace --locked
cargo test --workspace --locked
```

Pinned dependencies and grammar sources are recorded in the [dependency inventory](LICENSES/dependencies.json).
Third-party license texts and notices are preserved in [LICENSES](LICENSES) and bundled in the executable. Run `kodstar --licenses` to read them.

---

<p align="center">
  <sub>Built with Rust · Tree-sitter + Clang · Stored in SQLite</sub>
</p>
