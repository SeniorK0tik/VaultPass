#!/usr/bin/env bash
# Снимает все девять экранов интерфейса в обычном браузере.
# Нужно, чтобы сверять вёрстку с макетом, не собирая Tauri целиком.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="$(mktemp -d)"
PORT="${PORT:-8731}"
trap 'rm -rf "$WORK"; kill %1 2>/dev/null || true' EXIT

CHROME="$(command -v google-chrome || command -v chromium || command -v chromium-browser || true)"
if [[ -z "$CHROME" ]]; then
  echo "Не найден Chrome или Chromium." >&2
  exit 1
fi

cp -r "$ROOT/ui/." "$WORK/"
cp "$ROOT/tools/preview/mock-tauri.js" "$WORK/"
# Подставной мост подключается до модулей экрана — иначе они не найдут __TAURI__.
for f in "$WORK"/*.html; do
  python3 - "$f" <<'PY'
import pathlib, sys
p = pathlib.Path(sys.argv[1]); s = p.read_text(encoding='utf-8')
p.write_text(s.replace('<div id="app"',
    '<script src="./mock-tauri.js"></script>\n<div id="app"'), encoding='utf-8')
PY
done

(cd "$WORK" && python3 -m http.server "$PORT" >/dev/null 2>&1) &
sleep 1

if [[ "${1:-}" == "--serve" ]]; then
  echo "http://127.0.0.1:$PORT/index.html — Ctrl+C чтобы остановить"
  wait
fi

OUT="$ROOT/docs/screens"
mkdir -p "$OUT"
shoot() {
  "$CHROME" --headless --disable-gpu --no-sandbox --hide-scrollbars \
    --window-size="$3,$4" --virtual-time-budget=6000 --force-device-scale-factor=1 \
    --screenshot="$OUT/$1.png" "http://127.0.0.1:$PORT/$2" >/dev/null 2>&1
  echo "  $1.png"
}

shoot 1f-unlock    "index.html?mock=locked"                460  560
shoot 1a-palette   "quick.html?label=quick&quick=palette"  620  420
shoot 1b-quick     "quick.html?label=quick"                700  392
shoot 1c-tray      "tray.html?label=tray"                  330  372
shoot 1d-panels    "index.html"                            1180 740
shoot 1e-table     "index.html?view=table"                 1180 740
shoot 1g-editor    "editor.html?label=editor"              900  634
shoot 1h-generator "generator.html?label=generator"        470  500
shoot 1i-settings  "settings.html?label=settings"          900  630

echo "Готово: $OUT"
