#!/usr/bin/env bash
# Системные зависимости для сборки «Сейфа» на Debian 12/13 и Ubuntu 22.04+.
#
# Tauri рисует интерфейс в системном вебвью, поэтому для сборки нужны
# заголовки WebKitGTK. Готовому .deb эти пакеты не нужны — там достаточно
# рантаймовых, они перечислены в tauri.conf.json.
set -euo pipefail

echo "Устанавливаю зависимости сборки…"
sudo apt update
sudo apt install -y \
  build-essential curl wget file pkg-config \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libxdo-dev \
  libssl-dev

if ! command -v cargo >/dev/null; then
  echo
  echo "Rust не найден. Установите его:"
  echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  exit 1
fi

echo
echo "Готово. Дальше:"
echo "  cargo test                    # проверить ядро"
echo "  cargo tauri dev               # запустить"
echo "  cargo tauri build             # собрать .deb и AppImage"
