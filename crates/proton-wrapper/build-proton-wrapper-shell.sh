#!/usr/bin/env bash
set -e
echo "building shell for windows"
cargo build --release --package proton-wrapper-shell --target x86_64-pc-windows-gnu

cp ../../target/x86_64-pc-windows-gnu/release/proton-wrapper-shell.exe ./proton-wrapper-shell.exe
