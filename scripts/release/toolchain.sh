#!/usr/bin/env bash
# Purpose: Prepare verified LLVM and Darwin header inputs for the release builders.
# Pattern: Build Adapter.
# Usage: release.yml passes the native Rust target and a job-owned directory.
# Related: .github/workflows/release.yml, crates/kodstar-clang/build/resources.rs, crates/kodstar-clang/native/SOURCE.json.

set -euo pipefail

target="$1"
directory="$2"
llvm_version=23.1.4
case "$target" in
    aarch64-apple-darwin)
        llvm_platform=macOS-ARM64
        llvm_digest=0f8b0db925ab54a63028d788e85b14abaa25492e9608819728c17f6786948da4
        ;;
    x86_64-unknown-linux-gnu)
        llvm_platform=Linux-X64
        llvm_digest=efdd1dbd089b99bfcfcde7b8eb1236163f2e88df5a981cca4f783ec9cb5e0378
        ;;
    *)
        printf 'Unsupported release target %s.\n' "$target" >&2
        exit 2
        ;;
esac

mkdir -p "$directory"
cd "$directory"

fetch_verified() {
    local url="$1" file="$2" checksum="$3"
    if [[ ! -f "$file" ]]; then
        curl --fail --location --silent --show-error --retry 3 --output "$file" "$url"
    fi
    if command -v sha256sum > /dev/null; then
        printf '%s  %s\n' "$checksum" "$file" | sha256sum -c -
    else
        printf '%s  %s\n' "$checksum" "$file" | shasum -a 256 -c -
    fi
}

llvm_name="LLVM-$llvm_version-$llvm_platform"
llvm_archive="$llvm_name.tar.zst"
fetch_verified "https://github.com/llvm/llvm-project/releases/download/llvmorg-$llvm_version/$llvm_archive" \
    "$llvm_archive" "$llvm_digest"
mkdir llvm
# LLVM's release frames use a 1 GiB window. Only extract the build inputs.
zstd --memory=1024MB -dc "$llvm_archive" |
    tar -xf - -C llvm --strip-components 1 "$llvm_name/bin/llvm-config" "$llvm_name/include" "$llvm_name/lib"
rm "$llvm_archive"

if [[ "$target" == x86_64-unknown-linux-gnu ]]; then
    # The upstream Linux archive stores the generated libc++ ABI header in a target-specific include directory.
    cp "llvm/include/$target/c++/v1/__config_site" "llvm/include/c++/v1/__config_site"
fi

test "$(llvm/bin/llvm-config --version)" = "$llvm_version"
test -f llvm/lib/libclangAST.a
test -f llvm/include/c++/v1/__config_site

if [[ "$target" == aarch64-apple-darwin ]]; then
    zig_archive=zig-aarch64-macos-0.17.0.tar.xz
    fetch_verified "https://ziglang.org/download/0.17.0/$zig_archive" "$zig_archive" \
        b607e9b9234790a008116ae5bdb71c6243b84b9fb42a53a9e70fde41c06c536a
    mkdir zig
    tar -xJf "$zig_archive" -C zig --strip-components 1
    rm "$zig_archive"
fi
