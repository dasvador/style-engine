#!/bin/bash
pkill -f "target/debug/style-engine" 2>/dev/null
sleep 1
cargo run
