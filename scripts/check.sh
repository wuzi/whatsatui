#!/bin/sh
set -eu

cd -- "$(dirname -- "$0")/.."

# rust-toolchain.toml selects the same toolchain locally and in CI.
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export RUST_TEST_THREADS="${RUST_TEST_THREADS:-2}"

mode=${1:-all}
case "$mode" in
    all|checks|release) ;;
    *) printf 'Usage: %s [all|checks|release]\n' "$0" >&2; exit 2 ;;
esac

if [ "$mode" != release ]; then
    cargo fmt --all -- --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked --all-targets
fi

if [ "$mode" != checks ]; then
    cargo build --locked --release
fi
