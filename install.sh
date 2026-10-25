#!/bin/sh
# Purpose: Install the CLI from a temporary build or explicit binary and clean build output.
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
            'Install or update the CLI at ~/.local/bin/kodstar and prepare ~/.kodstar.' \
            'Builds this checkout in a temporary directory and removes build output on exit.' \
            'Use --binary to install an existing executable without building.'
        exit 0
        ;;
    --binary)
        if [ "$#" -ne 2 ] || [ ! -f "$2" ] || [ ! -x "$2" ]; then
            printf '%s\n' '--binary requires an executable KodStar binary.' >&2
            exit 2
        fi
        selected_binary=$2
        case "$selected_binary" in
            /*) ;;
            *) selected_binary="$(pwd -P)/$selected_binary" ;;
        esac
        exec "$selected_binary" __install
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
build_dir=$(mktemp -d "${TMPDIR:-/tmp}/kodstar-install.XXXXXXXX")
cleanup() {
    install_status=$?
    trap - 0 HUP INT TERM
    if ! rm -rf -- "$build_dir"; then
        printf 'Unable to remove temporary build directory: %s\n' "$build_dir" >&2
        if [ "$install_status" -eq 0 ]; then install_status=1; fi
    fi
    exit "$install_status"
}
trap cleanup 0
trap 'exit 129' HUP
trap 'exit 130' INT
trap 'exit 143' TERM

printf '%s\n' 'Building kodstar CLI...' >&2
"$rustup_bin" run "$toolchain" cargo build --quiet --profile dist --locked \
    -p kodstar-cli --target-dir "$build_dir/target"
"$build_dir/target/dist/kodstar" __install
