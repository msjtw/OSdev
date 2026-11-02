#!/usr/bin/env sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$ROOT"
export RUSTFLAGS="-C link-arg=-T$ROOT/../serverlib/linker/server.ld -C link-arg=--defsym=SERVER_HEAP_SIZE=${UART_HEAP_SIZE:-65536} -C link-arg=--no-relax"
cargo +nightly build --release --features server --target riscv32ima-unknown-none-elf \
    -Z build-std=core,alloc --target-dir "$ROOT/target" >&2
echo "$ROOT/target/riscv32ima-unknown-none-elf/release/uart_driver"
