# Единый вход в проект «Сейф» для Windows — то же, что ./x на Linux.
#
#   .\x.ps1              список команд
#   .\x.ps1 setup        проверить и доставить всё необходимое
#   .\x.ps1 test         тесты
#   .\x.ps1 dev          запустить с пересборкой на лету
#   .\x.ps1 build        собрать .msi и .exe
#   .\x.ps1 release 1.5.0
#
# Если PowerShell откажется запускать скрипт, разрешите это для текущего окна:
#   Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass

[CmdletBinding()]
param(
  [Parameter(Position = 0)][string]$Command = 'help',
  [Parameter(Position = 1, ValueFromRemainingArguments = $true)][string[]]$Rest
)

$ErrorActionPreference = 'Stop'
Set-Location -Path $PSScriptRoot

# Классический хост PowerShell 5.1 живёт в кодовой странице 866 и показал бы
# всю кириллицу мусором. Windows Terminal это делает сам, старая консоль — нет.
try { [Console]::OutputEncoding = [System.Text.Encoding]::UTF8 } catch { }

function Say  ($m) { Write-Host "> $m" -ForegroundColor White }
function Ok   ($m) { Write-Host "OK $m" -ForegroundColor Green }
function Warn ($m) { Write-Host "!  $m" -ForegroundColor Yellow }
function Die  ($m) { Write-Host "X  $m" -ForegroundColor Red; exit 1 }
function Have ($n) { $null -ne (Get-Command $n -ErrorAction SilentlyContinue) }

# PowerShell не считает ненулевой код возврата внешней программы ошибкой:
# без этой проверки `release` спокойно продолжил бы собирать установщик
# после провалившихся тестов.
function Run {
  param([Parameter(Mandatory)][string]$Exe, [Parameter(ValueFromRemainingArguments)][string[]]$Args)
  & $Exe @Args
  if ($LASTEXITCODE -ne 0) { Die "«$Exe $($Args -join ' ')» завершилась с кодом $LASTEXITCODE" }
}

# Версия компилятора закреплена в rust-toolchain.toml. На Windows та причина,
# по которой её закрепили, не проявляется (падал крейт из стека gtk-rs, а он
# собирается только на Linux) — но одинаковая версия на обеих платформах
# избавляет от «у меня собирается, а у тебя нет».
function Need-Toolchain {
  if (-not (Test-Path 'rust-toolchain.toml')) { return }
  $hit = Select-String -Path 'rust-toolchain.toml' -Pattern 'channel\s*=\s*"([^"]+)"' | Select-Object -First 1
  if (-not $hit) { return }
  $want = $hit.Matches[0].Groups[1].Value
  if (-not (rustup toolchain list | Select-String -SimpleMatch $want)) {
    Warn "Нужен Rust $want (закреплён в rust-toolchain.toml), его нет."
    Say  'Ставлю...'
    Run rustup toolchain install $want --profile minimal
  }
}

function Need-TauriCli {
  if (Have 'cargo-tauri') { return }
  Say 'Ставлю tauri-cli...'
  Run cargo install tauri-cli --version '^2' --locked
}

# Без компоновщика MSVC сборка падает глубоко внутри cargo с невнятным текстом.
# Дешевле проверить заранее и сказать прямо, чего не хватает.
function Need-Msvc {
  if (Have 'link.exe') { return }
  $vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
  if (Test-Path $vswhere) {
    $found = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($found) { Ok "Build Tools: $found"; return }
  }
  Die @"
Не найдены средства сборки C++ (компоновщик MSVC).
   Поставьте Visual Studio Build Tools и отметьте рабочую нагрузку
   «Разработка классических приложений на C++»:
   https://visualstudio.microsoft.com/visual-cpp-build-tools/
   После установки перезапустите PowerShell.
"@
}

function Cmd-Setup {
  Say 'Проверяю окружение'
  if (-not (Have 'cargo')) {
    Die "Rust не установлен. Поставьте rustup: https://rustup.rs, затем перезапустите PowerShell."
  }
  Ok "cargo $((cargo --version) -split ' ' | Select-Object -Index 1)"

  Need-Msvc
  Need-Toolchain
  Need-TauriCli
  Ok ((cargo tauri --version) | Select-Object -Last 1)

  $webview = @(
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
    'HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
  ) | Where-Object { Test-Path $_ }
  if ($webview) { Ok 'WebView2 установлен' }
  else { Warn 'WebView2 не найден. Он нужен для запуска; установщик доставит его сам, но для .\x.ps1 dev поставьте вручную: https://developer.microsoft.com/microsoft-edge/webview2/' }

  Write-Host ''
  Ok 'Готово. Дальше: .\x.ps1 test и .\x.ps1 dev'
}

function Cmd-Test {
  Say 'Тесты'
  if ($Rest) { Run cargo test --workspace @Rest } else { Run cargo test --workspace }
}
function Cmd-Check { Say 'Проверка типов'; Run cargo check --workspace }
function Cmd-Fmt   { Run cargo fmt --all; Ok 'отформатировано' }
function Cmd-Lint {
  Say 'Форматирование'; Run cargo fmt --all --check
  Say 'Clippy';         Run cargo clippy --workspace --all-targets -- -D warnings
}

function Cmd-Dev {
  Need-Msvc; Need-Toolchain; Need-TauriCli
  Say 'Запускаю (правки в ui\ подхватываются перезагрузкой окна: Ctrl+R)'
  if ($Rest) { Run cargo tauri dev @Rest } else { Run cargo tauri dev }
}

function Cmd-Run {
  Need-Msvc; Need-Toolchain
  Say 'Собираю релизный двоичный файл'
  Run cargo build --release -p seif
  Ok 'запускаю target\release\seif.exe'
  & '.\target\release\seif.exe'
}

function Cmd-Build {
  Need-Msvc; Need-Toolchain; Need-TauriCli

  # NSIS и MSI собираются раздельно намеренно. WiX заметно капризнее: он
  # запускает проверки установщика (ICE), которые падают по причинам, никак
  # не связанным с приложением. Одной командой провал WiX обесценил бы уже
  # готовый .exe — а он и есть установщик, который Tauri предлагает по
  # умолчанию.
  Say 'Собираю .exe (NSIS)'
  Run cargo tauri build --target x86_64-pc-windows-msvc --bundles nsis

  Say 'Собираю .msi (WiX)'
  & cargo tauri build --target x86_64-pc-windows-msvc --bundles msi
  if ($LASTEXITCODE -ne 0) {
    Write-Host ''
    Warn 'MSI собрать не удалось — .exe выше готов и полностью работоспособен.'
    Warn 'Tauri не показывает вывод light.exe. Чтобы увидеть настоящую причину:'
    Write-Host '   cargo tauri build --target x86_64-pc-windows-msvc --bundles msi --verbose'
  }

  Write-Host ''
  Ok 'Готово:'
  Get-ChildItem -Path 'target\x86_64-pc-windows-msvc\release\bundle' -Recurse -File -ErrorAction SilentlyContinue |
    Where-Object { $_.Extension -in '.msi', '.exe' } |
    ForEach-Object { Write-Host ("   {0}  ({1:N0} байт)" -f $_.FullName, $_.Length) }
  Write-Host ''
  Warn 'Установщики не подписаны: SmartScreen покажет предупреждение при первом запуске.'
}

# Номер версии живёт в трёх местах, и они обязаны совпадать: рассинхрон
# всплывает уже в установщике, где его труднее всего заметить.
function Cmd-Version {
  $v = $Rest | Select-Object -First 1
  if (-not $v) {
    $a = (Select-String -Path 'Cargo.toml' -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
    $b = (Select-String -Path 'src-tauri\tauri.conf.json' -Pattern '"version":\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
    Write-Host "Cargo.toml       $a"
    Write-Host "tauri.conf.json  $b"
    return
  }
  if ($v -notmatch '^\d+\.\d+\.\d+$') { Die "Версия должна быть вида 1.5.0, а не «$v»." }

  $short = ($v -split '\.')[0..1] -join '.'

  # Через .NET, а не Get-Content/Set-Content: в PowerShell 5.1 «-Encoding UTF8»
  # дописывает BOM, а он ломает разбор и Cargo.toml, и tauri.conf.json.
  # Заодно [regex]::Replace умеет ограничить число замен — у оператора
  # -replace такой возможности нет вовсе.
  $utf8 = New-Object System.Text.UTF8Encoding($false)
  function Replace-First([string]$path, [string]$pattern, [string]$value) {
    $full = Join-Path $PSScriptRoot $path
    $text = [System.IO.File]::ReadAllText($full, $utf8)
    $text = [regex]::Replace($text, $pattern, $value, 1)
    [System.IO.File]::WriteAllText($full, $text, $utf8)
  }
  function Replace-All([string]$path, [string]$pattern, [string]$value) {
    $full = Join-Path $PSScriptRoot $path
    $text = [System.IO.File]::ReadAllText($full, $utf8)
    [System.IO.File]::WriteAllText($full, [regex]::Replace($text, $pattern, $value), $utf8)
  }

  Replace-First 'Cargo.toml'              '(?m)^version = "[^"]+"'  "version = `"$v`""
  Replace-First 'src-tauri/tauri.conf.json' '"version": "[^"]+"'    "`"version`": `"$v`""
  Replace-All   'ui/js/settings.js'       'Сейф \d+\.\d+ ·'       "Сейф $short ·"

  cargo update -w --quiet 2>$null
  Ok "Версия поднята до $v"
  $script:Rest = @(); Cmd-Version
}

function Cmd-Release {
  $v = $Rest | Select-Object -First 1
  if (-not $v) { Die 'Укажите версию: .\x.ps1 release 1.5.0' }
  Cmd-Version
  $script:Rest = @()
  Cmd-Lint
  Cmd-Test
  Cmd-Build
  Write-Host ''
  Ok 'Локальные установщики собраны.'
  Say 'Чтобы собрать обе платформы, поставьте тег — CI сделает остальное:'
  Write-Host "   git commit -am `"Версия $v`"; git tag v$v; git push --tags"
}

function Cmd-Logs {
  # Именно LOCALAPPDATA: Tauri кладёт журнал в app_log_dir, а это
  # {FOLDERID_LocalAppData}\{identifier}\logs, а не роуминговый APPDATA.
  $f = Join-Path $env:LOCALAPPDATA 'app.seif.vault\logs\seif.log' 
  if (-not (Test-Path $f)) {
    Warn "Журнала ещё нет: $f"
    Say  'Он появится при первом запуске — .\x.ps1 dev'
    return
  }
  Say $f
  $arg = $Rest | Select-Object -First 1
  if ($arg -eq '-f') { Get-Content $f -Tail 40 -Wait }
  elseif ($arg) { Get-Content $f -Tail ([int]$arg) }
  else { Get-Content $f -Tail 60 }
}

function Cmd-Clean { Run cargo clean; Ok 'target\ очищен' }

function Usage {
  @'
Сейф — единый вход в проект (Windows).

  .\x.ps1 setup              проверить и доставить всё необходимое
  .\x.ps1 test               прогнать тесты
  .\x.ps1 check              проверить типы всего проекта
  .\x.ps1 lint               формат + clippy как в CI
  .\x.ps1 fmt                отформатировать код

  .\x.ps1 dev                запустить с пересборкой на лету
  .\x.ps1 run                собрать релиз и запустить

  .\x.ps1 build              собрать .msi и .exe
  .\x.ps1 version [1.5.0]    показать или поднять номер версии
  .\x.ps1 release 1.5.0      версия + проверки + сборка

  .\x.ps1 logs [N|-f]        последние N строк журнала (-f — следить)
  .\x.ps1 clean              очистить target\
'@ | Write-Host
}

switch ($Command) {
  'setup'   { Cmd-Setup }
  'test'    { Cmd-Test }
  'check'   { Cmd-Check }
  'lint'    { Cmd-Lint }
  'fmt'     { Cmd-Fmt }
  'dev'     { Cmd-Dev }
  'run'     { Cmd-Run }
  'build'   { Cmd-Build }
  'version' { Cmd-Version }
  'release' { Cmd-Release }
  'logs'    { Cmd-Logs }
  'clean'   { Cmd-Clean }
  'help'    { Usage }
  default   { Write-Host "Неизвестная команда «$Command»" -ForegroundColor Red; Write-Host ''; Usage; exit 1 }
}
