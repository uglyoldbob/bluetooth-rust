#/bin/sh

set -e
cargo build
cargo build --target=aarch64-linux-android
