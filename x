#!/usr/bin/env bash
# Единый вход в проект «Сейф». Все команды — здесь; помнить cargo-заклинания
# и пути к пакетам не нужно.
#
#   ./x                 список команд
#   ./x setup           поставить всё необходимое
#   ./x test            тесты
#   ./x dev             запустить с пересборкой на лету
#   ./x run             собрать релиз и запустить
#   ./x build           собрать пакеты для этой ОС
#   ./x version 1.5.0   поднять номер версии везде
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"

BOLD=$'\033[1m'; DIM=$'\033[2m'; GREEN=$'\033[32m'; RED=$'\033[31m'; YEL=$'\033[33m'; OFF=$'\033[0m'
say()  { printf '%s▸ %s%s\n' "$BOLD" "$*" "$OFF"; }
ok()   { printf '%s✓ %s%s\n' "$GREEN" "$*" "$OFF"; }
warn() { printf '%s! %s%s\n' "$YEL" "$*" "$OFF"; }
die()  { printf '%s✗ %s%s\n' "$RED" "$*" "$OFF" >&2; exit 1; }

# На этой машине rustc 1.96 падает с SIGSEGV на glib-macros 0.18.5 — крейте из
# стека gtk-rs, который Tauri тянет на Linux и версию которого поднять нельзя.
# Версия компилятора закреплена в rust-toolchain.toml; здесь только проверка,
# что она установлена, с понятным сообщением вместо стены вывода cargo.
need_toolchain() {
  [[ -f rust-toolchain.toml ]] || return 0
  local want
  want="$(grep -oP 'channel\s*=\s*"\K[^"]+' rust-toolchain.toml || true)"
  [[ -n "$want" ]] || return 0
  if ! rustup toolchain list 2>/dev/null | grep -q "^$want"; then
    warn "Нужен Rust $want (закреплён в rust-toolchain.toml), его нет."
    say "Ставлю…"
    rustup toolchain install "$want" --profile minimal
  fi
}

have() { command -v "$1" >/dev/null 2>&1; }

need_tauri_cli() {
  have cargo-tauri && return 0
  say "Ставлю tauri-cli…"
  cargo install tauri-cli --version '^2' --locked
}

os_name() {
  case "$(uname -s)" in
    Linux)  echo linux ;;
    Darwin) echo macos ;;
    *)      echo windows ;;
  esac
}

# ─────────────────────────────────────────────────────────────────────────────

cmd_setup() {
  say "Проверяю окружение"
  have cargo || die "Rust не установлен: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  ok "cargo $(cargo --version | cut -d' ' -f2)"

  if [[ "$(os_name)" == linux ]]; then
    local missing=()
    for m in webkit2gtk-4.1 javascriptcoregtk-4.1 libsoup-3.0 gtk+-3.0 \
             ayatana-appindicator3-0.1 librsvg-2.0 openssl; do
      pkg-config --exists "$m" 2>/dev/null || missing+=("$m")
    done
    if ((${#missing[@]})); then
      warn "Не хватает библиотек: ${missing[*]}"
      say "Ставлю (нужен пароль sudo)…"
      sudo apt-get update
      sudo apt-get install -y \
        build-essential curl wget file pkg-config \
        libwebkit2gtk-4.1-dev libayatana-appindicator3-dev \
        librsvg2-dev libxdo-dev libssl-dev
    fi
    ok "системные библиотеки на месте"
  fi

  need_toolchain
  need_tauri_cli
  ok "$(cargo tauri --version 2>/dev/null | tail -1)"
  echo
  ok "Готово. Дальше: ./x test и ./x dev"
}

cmd_test() {
  say "Тесты ядра"
  cargo test -p vault-core "$@"
}

cmd_lint() {
  say "Форматирование"
  cargo fmt --all --check
  say "Clippy"
  cargo clippy --workspace --all-targets -- -D warnings
}

cmd_fmt() {
  cargo fmt --all
  ok "отформатировано"
}

cmd_check() {
  say "Проверка типов всего проекта"
  cargo check --workspace
}

cmd_dev() {
  need_toolchain; need_tauri_cli
  say "Запускаю (правки в ui/ подхватываются перезагрузкой окна: Ctrl+R)"
  cargo tauri dev "$@"
}

cmd_run() {
  need_toolchain
  say "Собираю релизный двоичный файл"
  cargo build --release -p seif
  local bin="target/release/seif"
  [[ "$(os_name)" == windows ]] && bin="target/release/seif.exe"
  ok "запускаю $bin"
  "./$bin"
}

cmd_build() {
  need_toolchain; need_tauri_cli
  local target="${1:-$(os_name)}"
  case "$target" in
    linux)
      [[ "$(os_name)" == linux ]] || die "Пакеты для Linux собираются на Linux."
      say "Собираю .deb и AppImage"
      cargo tauri build
      echo
      ok "Готово:"
      find target/release/bundle -maxdepth 2 -type f \( -name '*.deb' -o -name '*.AppImage' \) \
        -printf '   %p  (%s байт)\n' 2>/dev/null
      ;;
    windows)
      if [[ "$(os_name)" == windows ]]; then
        say "Собираю .msi и .exe"
        cargo tauri build --target x86_64-pc-windows-msvc
        return
      fi
      # С Linux собирается только NSIS: .msi делает WiX, а он работает
      # исключительно под Windows. Сам Tauri называет такую сборку крайним
      # средством и тестирует её заметно меньше обычной, поэтому выпускать
      # версии всё равно лучше через CI — здесь это способ быстро получить
      # .exe для проверки, не поднимая виртуальную машину.
      command -v cargo-xwin >/dev/null || {
        warn "Нужен cargo-xwin."
        say "Ставлю…"
        cargo install --locked cargo-xwin
      }
      rustup target list --installed | grep -q x86_64-pc-windows-msvc || {
        say "Добавляю цель x86_64-pc-windows-msvc…"
        rustup target add x86_64-pc-windows-msvc
      }
      warn "С Linux собирается только NSIS (.exe). Для .msi нужна Windows или CI."
      cargo tauri build --runner cargo-xwin --target x86_64-pc-windows-msvc --bundles nsis
      echo
      ok "Готово:"
      find target/x86_64-pc-windows-msvc/release/bundle -maxdepth 2 -type f -name '*.exe' \
        -printf '   %p  (%s байт)\n' 2>/dev/null
      ;;
    all)
      cmd_build "$(os_name)"
      if [[ "$(os_name)" == linux ]]; then
        cmd_build windows
        warn ".msi здесь не собрать — его делает CI по тегу."
      fi
      ;;
    *) die "Не знаю цель «$target». Бывают: linux, windows, all." ;;
  esac
}

# Номер версии живёт в трёх местах, и они обязаны совпадать: рассинхрон
# всплывает уже в установщике, где его труднее всего заметить.
cmd_version() {
  local v="${1:-}"
  if [[ -z "$v" ]]; then
    printf 'Cargo.toml       %s\n' "$(grep -m1 -oP '^version\s*=\s*"\K[^"]+' Cargo.toml)"
    printf 'tauri.conf.json  %s\n' "$(grep -m1 -oP '"version":\s*"\K[^"]+' src-tauri/tauri.conf.json)"
    return
  fi
  [[ "$v" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "Версия должна быть вида 1.5.0, а не «$v»."

  sed -i -E "0,/^version = \"[^\"]+\"/s//version = \"$v\"/" Cargo.toml
  sed -i -E "0,/\"version\": \"[^\"]+\"/s//\"version\": \"$v\"/" src-tauri/tauri.conf.json
  sed -i -E "s/Сейф [0-9]+\.[0-9]+( ·|,)/Сейф ${v%.*}\1/" ui/js/settings.js || true
  cargo update -w --quiet 2>/dev/null || true
  ok "Версия поднята до $v"
  cmd_version
}

cmd_release() {
  local v="${1:-}"
  [[ -n "$v" ]] || die "Укажите версию: ./x release 1.5.0"
  cmd_version "$v"
  cmd_lint
  cmd_test
  cmd_build "$(os_name)"
  echo
  ok "Локальные пакеты собраны."
  grep -q "^## $v " CHANGELOG.md 2>/dev/null || warn "В CHANGELOG.md нет записи про $v."
  say "Чтобы собрать обе платформы, поставьте тег — CI сделает остальное:"
  echo "   git commit -am \"Версия $v\" && git tag v$v && git push --tags"
}

cmd_screens() {
  say "Пересобираю снимки экранов"
  ./tools/preview/run.sh
}

cmd_preview() {
  ./tools/preview/run.sh --serve
}

cmd_icons() {
  python3 tools/make_icons.py
  ok "иконки перерисованы"
}

# Каталог журнала задаёт Tauri по идентификатору приложения, а не по имени
# двоичного файла, — угадывать его руками неудобно.
log_path() {
  local id="app.seif.vault"
  case "$(os_name)" in
    linux) echo "${XDG_DATA_HOME:-$HOME/.local/share}/$id/logs/seif.log" ;;
    macos) echo "$HOME/Library/Logs/$id/seif.log" ;;
    *)     echo "$APPDATA/$id/logs/seif.log" ;;
  esac
}

cmd_logs() {
  local f; f="$(log_path)"
  if [[ ! -f "$f" ]]; then
    warn "Журнала ещё нет: $f"
    say "Он появится при первом запуске — ./x dev"
    return
  fi
  say "$f"
  if [[ "${1:-}" == "-f" ]]; then
    tail -f "$f"
  else
    tail -n "${1:-60}" "$f"
  fi
}

cmd_clean() {
  cargo clean
  ok "target/ очищен"
}

usage() {
  cat <<'USAGE'
Сейф — единый вход в проект.

  ./x setup              поставить всё необходимое (библиотеки, tauri-cli)
  ./x test               прогнать тесты ядра
  ./x check              проверить типы всего проекта
  ./x lint               формат + clippy как в CI
  ./x fmt                отформатировать код

  ./x dev                запустить приложение с пересборкой на лету
  ./x run                собрать релиз и запустить

  ./x build [linux|windows|all]   собрать установщики
  ./x version [1.5.0]    показать или поднять номер версии
  ./x release 1.5.0      версия + проверки + сборка + подсказка про тег

  ./x logs [N|-f]        последние N строк журнала (-f — следить)
  ./x screens            пересобрать снимки экранов в docs/screens/
  ./x preview            открыть экраны в браузере без сборки Tauri
  ./x icons              перерисовать иконки приложения
  ./x clean              очистить target/
USAGE
}

case "${1:-help}" in
  setup)   shift; cmd_setup "$@" ;;
  test)    shift; cmd_test "$@" ;;
  check)   shift; cmd_check "$@" ;;
  lint)    shift; cmd_lint "$@" ;;
  fmt)     shift; cmd_fmt "$@" ;;
  dev)     shift; cmd_dev "$@" ;;
  run)     shift; cmd_run "$@" ;;
  build)   shift; cmd_build "$@" ;;
  version) shift; cmd_version "$@" ;;
  release) shift; cmd_release "$@" ;;
  logs)    shift; cmd_logs "$@" ;;
  screens) shift; cmd_screens "$@" ;;
  preview) shift; cmd_preview "$@" ;;
  icons)   shift; cmd_icons "$@" ;;
  clean)   shift; cmd_clean "$@" ;;
  help|-h|--help) usage ;;
  *) printf '%sНеизвестная команда «%s»%s\n\n' "$RED" "$1" "$OFF"; usage; exit 1 ;;
esac
