#!/bin/bash
pkill -f "target/debug/rust-web-app" 2>/dev/null
sleep 1
cargo run
