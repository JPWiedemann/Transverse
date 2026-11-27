#!/usr/bin/env bash
# Builds the WebAssembly module of the web version into web/pkg.
#
#   scripts/build-web.sh
#
# Needs: the wasm32 target (rustup target add wasm32-unknown-unknown) and wasm-bindgen-cli in the
# version of the wasm-bindgen crate (cargo install wasm-bindgen-cli --locked --version <that>).
# wasm-opt (binaryen) is used when it is installed.
set -euo pipefail
cd "$(dirname "$0")/.."

want=$(awk '/^name = "wasm-bindgen"$/ {getline; gsub(/[^0-9.]/, "", $0); print; exit}' Cargo.lock)
have=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}') || true
if [ "$have" != "$want" ]; then
    echo "wasm-bindgen-cli $want needed (found: ${have:-none}): cargo install wasm-bindgen-cli --locked --version $want" >&2
    exit 1
fi

rustup target add wasm32-unknown-unknown >/dev/null
cargo build --release --target wasm32-unknown-unknown -p transverse-wasm
rm -rf web/pkg
wasm-bindgen --target web --out-dir web/pkg target/wasm32-unknown-unknown/release/transverse_wasm.wasm
if command -v wasm-opt >/dev/null 2>&1; then
    wasm-opt -O3 -o web/pkg/transverse_wasm_bg.wasm web/pkg/transverse_wasm_bg.wasm
fi
ls -l web/pkg/transverse_wasm_bg.wasm
