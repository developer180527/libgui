#!/usr/bin/env bash
# Build libgui_demo for the iOS Simulator and assemble target/ios-sim/libgui.app.
# Install/launch:  xcrun simctl install booted target/ios-sim/libgui.app
#                  xcrun simctl launch booted com.libgui.demo
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p libgui_demo --target aarch64-apple-ios-sim
APP=target/ios-sim/libgui.app
rm -rf "$APP" && mkdir -p "$APP"
cp target/aarch64-apple-ios-sim/release/libgui_demo "$APP/"
cp crates/libgui_demo/ios/Info.plist "$APP/"
codesign --force --sign - "$APP" >/dev/null 2>&1
echo "$APP"
