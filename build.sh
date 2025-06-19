#/bin/sh
set -e
cargo build
cargo build --target=aarch64-linux-android
cd examples/android
cargo android build
