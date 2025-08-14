#!/usr/bin/env bash
set -e
echo "building shell for windows"
cargo build --release --package wine-wrapper-shell --target x86_64-pc-windows-gnu

cp ../../target/x86_64-pc-windows-gnu/release/wine-wrapper-shell.exe ./wine-wrapper-shell.exe
