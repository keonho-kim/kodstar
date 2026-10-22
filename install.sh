#!/bin/sh
# Purpose: Build the pinned local source and enter the owned user installation flow.
# Pattern: Installation Entrypoint.
# Usage: ./install.sh or ./install.sh --binary /path/to/kodstar.
# Related: rust-toolchain.toml, crates/kodstar-cli/src/installation/mod.rs.

set -eu

case "$(uname -s)" in
    Darwin|Linux) ;;
    *) printf '%s\n' 'KodStar supports macOS and Linux only.' >&2; exit 1 ;;
esac

case "${1-}" in
    --help|-h)
        printf '%s\n' 'Usage: ./install.sh [--binary /path/to/kodstar]' \
            'Build and install this checkout. Re-run to replace an existing installation.'
        exit 0
        ;;
    --binary)
        if [ "$#" -ne 2 ] || [ ! -f "$2" ] || [ ! -x "$2" ]; then
            printf '%s\n' '--binary requires an executable KodStar binary.' >&2
            exit 2
        fi
        exec "$2" __install
        ;;
    '') ;;
    *) printf '%s\n' 'Unknown argument. Use ./install.sh --help.' >&2; exit 2 ;;
esac

repo=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd -P)
cd "$repo"
toolchain=$(sed -n 's/^channel = "\([^"]*\)"$/\1/p' rust-toolchain.toml)
if [ -z "$toolchain" ]; then
    printf '%s\n' 'Unable to read the pinned Rust toolchain.' >&2
    exit 1
fi
rustup_bin=$(command -v rustup || true)
if [ -z "$rustup_bin" ] && [ -x "${HOME:?}/.cargo/bin/rustup" ]; then
    rustup_bin=$HOME/.cargo/bin/rustup
fi
if [ -z "$rustup_bin" ]; then
    printf '%s\n' 'Install Rust with rustup and a native C/C++ toolchain first.' >&2
    exit 1
fi
if ! "$rustup_bin" run "$toolchain" rustc --version >/dev/null 2>&1; then
    printf 'Install the pinned toolchain first: rustup toolchain install %s --profile minimal\n' "$toolchain" >&2
    exit 1
fi
"$rustup_bin" run "$toolchain" cargo build --profile dist --locked -p kodstar-cli --target-dir "$repo/target"
exec "$repo/target/dist/kodstar" __install
