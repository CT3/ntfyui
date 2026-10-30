#!/usr/bin/env bash
# Renders the UI with sample data in headless Chrome and writes docs/screenshots/*.png.
# Usage: scripts/screenshots.sh   (needs Google Chrome and python3)
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
chrome="${CHROME:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}"
out="$root/docs/screenshots"
port=8765
work="$(mktemp -d)"
trap 'kill "${server_pid:-}" 2>/dev/null || true; rm -rf "$work"' EXIT

# Copy the real UI and load the mock bridge before main.js.
cp "$root"/src/{styles.css,main.js,emoji.json} "$work/"
cp "$root/scripts/screenshots/mock.js" "$work/"
sed 's#<script src="main.js"></script>#<script src="mock.js"></script><script src="main.js"></script>#' \
  "$root/src/index.html" > "$work/index.html"

python3 -m http.server "$port" --bind 127.0.0.1 --directory "$work" >/dev/null 2>&1 &
server_pid=$!
disown
sleep 1

shot() { # tab, color scheme (0 = dark, 1 = light), output name
  "$chrome" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=2 \
    --window-size=940,690 --blink-settings=preferredColorScheme="$2" --virtual-time-budget=3000 \
    --screenshot="$out/$3.png" "http://127.0.0.1:$port/index.html#$1" 2>/dev/null
}

mkdir -p "$out"
for tab in messages send subscriptions servers settings; do shot "$tab" 1 "$tab"; done
shot messages 0 messages-dark
echo "Screenshots written to $out"
