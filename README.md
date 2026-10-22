<p align="center">
  <img src="docs/assets/kodstar-boot-logo.png" width="680" alt="KodStar — a gold star and orbital wordmark inspired by vintage PC boot screens" />
</p>

<h1 align="center">KodStar</h1>

<p align="center">
  <strong>Follow the code. Keep the history.</strong><br />
  A local CLI for exploring code relationships and keeping source history.
</p>

<p align="center">
  <a href="rust-toolchain.toml"><img src="https://img.shields.io/badge/Rust-1.99.1-D6AA33?style=flat-square&amp;logo=rust&amp;logoColor=white&amp;labelColor=161B18" alt="Rust toolchain 1.99.1" /></a>
  <a href="#supported-languages"><img src="https://img.shields.io/badge/platform-macOS%20%2B%20Linux-D6AA33?style=flat-square&amp;labelColor=161B18" alt="macOS and Linux" /></a>
  <a href="#features"><img src="https://img.shields.io/badge/analysis-local-D6AA33?style=flat-square&amp;labelColor=161B18" alt="Local analysis" /></a>
  <a href="#supported-languages"><img src="https://img.shields.io/badge/status-early%20development-D6AA33?style=flat-square&amp;labelColor=161B18" alt="Early development" /></a>
</p>

<p align="center">
  <a href="#quick-start">Quick start</a> ·
  <a href="#examples">Examples</a> ·
  <a href="#supported-languages">Languages</a> ·
  <a href="#documentation-and-development">Documentation</a>
</p>

---

**KodStar** is a source-code relationship analysis engine and CLI written in Rust.
It extracts syntax facts with official **Tree-sitter** parsers and stores relationships and analysis state in **SQLite**.
Follow calls, references, and dependencies, while automatic checkpoints preserve changes observed as you use the CLI.

## Features

- **Explore code relationships** — Find symbol definitions, call targets, incoming references, and dependency paths.
- **Inspect change impact** — Trace the impact of a symbol or changes since a saved checkpoint.
- **Incremental synchronization** — After initialization, regular commands check for changes and update the analysis.
- **Automatic local history** — Name checkpoints, compare changes, restore files, and work with local branches.
- **Configurable resource budgets** — Set worker counts and memory budgets, backed by a disk-based index.
- **Tool integration** — Use text, JSON, JSONL, or TSV output, file watching, and stdio sessions.

## Quick start

### 1. Install

You need `rustup`, the [pinned Rust toolchain](rust-toolchain.toml), and native C build tools.
See [development setup](docs/development.md#개발-환경) for platform-specific requirements.

```bash
git clone https://github.com/keonho-kim/kodstar.git
cd kodstar
./install.sh
```

The installer places a dist build at `$HOME/.local/bin/kodstar` and configures your shell's PATH.
Open a new terminal, or run the PATH command printed by the installer, to use `kodstar` from any directory.
To update an existing installation, get the latest source and run `./install.sh` again.

<details>
<summary>Installation path and shell setup</summary>

PATH setup covers sh, Bash, and Zsh. Fish is configured when it is your shell or its configuration file exists.
Installation is separate from project initialization. To install an existing executable, use
`./install.sh --binary /absolute/path/to/kodstar`.

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
Inside a project, KodStar selects the registered ancestor root, so you can use relative paths.

```bash
# List symbols defined in a file
kodstar symbols src/api.rs

# Inspect caller's dependencies and how they were resolved
kodstar deps src/api.rs caller
kodstar explain src/api.rs caller

# Search definitions by name
kodstar search target
```

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

# Compare against the saved checkpoint and inspect change impact
kodstar history diff --from baseline
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
KodStar stops if it detects conflicting external edits. See the [local history contract](kodstar-spec.md#7-cli-명령어-전체) for details.

### Watch files and connect external tools

```bash
# Watch changes to saved files
kodstar watch

# Start a stdio session for external tools
kodstar serve --stdio
```

JSON, JSONL, TSV, and stdio contracts are defined in the [specification](kodstar-spec.md).

<details>
<summary>Command overview</summary>

| Purpose | Commands |
|---|---|
| Initialization and analysis updates | `init`, `init --full`, `init --verify` |
| Definitions and name search | `symbols`, `list`, `search` |
| Relationships, explanations, and impact | `related`, `deps`, `refs`, `explain`, `impact`, `path` |
| Local history | `history`, `history save/diff/restore/upgrade` |
| Local branches | `branch`, `branch create/switch/delete` |
| Status and diagnostics | `status`, `doctor`, `languages` |
| Cache management | `cache list/clean/gc` |
| Tool integration | `watch`, `serve --stdio` |

Run `kodstar <command> --help` to see the options for each command.

</details>

## Supported languages

KodStar provides selected static analysis rules for **19 languages** and **6 component and template formats**.

| Category | Languages and formats |
|---|---|
| Systems and native | Rust, Go, C, C++, Zig |
| Application languages | Java, Kotlin, Scala, C#, Swift, Pascal |
| Dynamic languages and web | Python, JavaScript, TypeScript, Ruby, PHP |
| BEAM·Lisp | Elixir, Erlang, Common Lisp |
| Components and templates | Vue, Svelte, Astro, EJS, Blade, Razor |

**Full implementation of the specification and porting of Graphify's language and framework rules are still in progress.**
Run `kodstar languages --format json` for capabilities and limitations by language.
See [porting verification](docs/development.md#포팅-검증) for evidence and remaining gaps.

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

`kodstar history upgrade` explicitly converts history formats 1, 2, and 3 to format 4.
For format 4, it repacks stored content and reclaims free pages while preserving checkpoints, source content, names, and branches.
If the analysis cache format is incompatible, explicitly clean it and initialize again.

```bash
kodstar history upgrade
kodstar cache clean --root .
kodstar init .
```

See [storage consistency](kodstar-spec.md#16-저장-일관성과-장애-시-동작) for storage formats and recovery behavior,
and the [development guide](docs/development.md) for measurements and performance tradeoffs.

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
cause uninstall to stop with a diagnostic. If a project has moved, use KodStar once at its new location to update its registration before uninstalling.

</details>

## Documentation and development

Detailed documentation is currently in Korean.

| Document | Contents |
|---|---|
| [Specification](kodstar-spec.md) | Product scope, commands, output, freshness, and stdio contracts |
| [Architecture](docs/architecture.md) | Layers, source layout, execution flow, and state and resource ownership |
| [Development guide](docs/development.md) | Setup, builds, configuration, verification, dependencies, and porting evidence |
| [Development principles](AGENTS.md) | Design review, code, comments, and change procedures |

The engine lives in [`kodstar-core`](crates/kodstar-core), and the CLI in [`kodstar-cli`](crates/kodstar-cli).
Read the development principles before making changes, and follow the [verification procedure](docs/development.md#변경-검증).

```bash
cargo build --workspace --locked
cargo test --workspace --locked
```

Pinned dependencies and grammar sources are recorded in the [dependency inventory](LICENSES/dependencies.json).
Third-party license texts and notices are preserved in [LICENSES](LICENSES).

---

<p align="center">
  <sub>Built with Rust · Parsed with Tree-sitter · Stored in SQLite</sub>
</p>
