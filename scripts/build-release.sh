#!/usr/bin/env bash
# Release builds for Linux and Windows from a Linux machine.
#
#   scripts/build-release.sh [linux|windows|all]      (default: all)
#
# Needs: rustup, zig, cargo-zigbuild
#   cargo install cargo-zigbuild --locked
#   zig:  pacman -S zig   or   pip install ziglang
#
# The Linux build links against glibc 2.28, so it runs on anything from Debian 10 / Ubuntu 18.04 on.
# Results: dist/transverse-<version>-linux-x86_64.tar.gz and dist/transverse-<version>-windows-x86_64.zip
set -euo pipefail
cd "$(dirname "$0")/.."

what="${1:-all}"
version=$(awk -F'"' '/^version *=/ {print $2; exit}' Cargo.toml)
dist=dist
mkdir -p "$dist"

need() {
    command -v "$1" >/dev/null 2>&1 || { echo "missing tool: $1  ($2)" >&2; exit 1; }
}
need rustup "https://rustup.rs"
need cargo-zigbuild "cargo install cargo-zigbuild --locked"
if ! command -v zig >/dev/null 2>&1 && ! python3 -m ziglang version >/dev/null 2>&1; then
    echo "missing tool: zig  (pacman -S zig   or   pip install ziglang)" >&2
    exit 1
fi

package_dir() { # <name> <target dir> <exe suffix>
    local name="$1" tdir="$2" suffix="$3"
    rm -rf "$dist/$name"
    mkdir -p "$dist/$name"
    cp "$tdir/transverse-gui$suffix" "$tdir/transverse$suffix" "$dist/$name/"
    cp packaging/README.txt LICENSE "$dist/$name/"
}

build_linux() {
    local target=x86_64-unknown-linux-gnu
    local name="transverse-$version-linux-x86_64"
    rustup target add "$target" >/dev/null
    cargo zigbuild --release -p transverse-gui -p transverse-cli --target "$target.2.28"
    package_dir "$name" "target/$target/release" ""
    tar -C "$dist" -czf "$dist/$name.tar.gz" "$name"
    echo "built $dist/$name.tar.gz"
}

build_windows() {
    local target=x86_64-pc-windows-gnu
    local name="transverse-$version-windows-x86_64"
    rustup target add "$target" >/dev/null
    cargo zigbuild --release -p transverse-gui -p transverse-cli --target "$target"
    package_dir "$name" "target/$target/release" ".exe"
    rm -f "$dist/$name.zip"
    (cd "$dist" && python3 -m zipfile -c "$name.zip" "$name")
    echo "built $dist/$name.zip"
}

case "$what" in
    linux) build_linux ;;
    windows) build_windows ;;
    all) build_linux; build_windows ;;
    *) echo "usage: $0 [linux|windows|all]" >&2; exit 2 ;;
esac
