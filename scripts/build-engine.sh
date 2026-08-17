#!/bin/sh
set -eu

TRANSLATECODE_WASI_SYSROOT="${TRANSLATECODE_WASI_SYSROOT:-/opt/homebrew/opt/wasi-libc/share/wasi-sysroot}"
WASI_SYSROOT="$TRANSLATECODE_WASI_SYSROOT" \
CC_wasm32_wasip1="/opt/homebrew/opt/llvm/bin/clang" \
AR_wasm32_wasip1="/opt/homebrew/opt/llvm/bin/llvm-ar" \
CFLAGS_wasm32_wasip1="--sysroot=$TRANSLATECODE_WASI_SYSROOT" \
cargo build --manifest-path engine/Cargo.toml --target wasm32-wasip1 --release
mkdir -p public
cp engine/target/wasm32-wasip1/release/translatecode_engine.wasm public/engine.wasm
