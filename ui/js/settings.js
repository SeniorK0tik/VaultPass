// Экран 1i — настройки. Разделы слева, содержимое справа.
//
// Настройки хранятся в открытом config.json рядом с сейфом: в них нет
// секретов, зато горячая клавиша и путь к хранилищу нужны ещё до
// разблокировки.

import {
  call, getSettings, setSettings, status, auditReport, openWindow,
  logInfo, logTail, openLogDir, seedStatus, seedPickFile, seedForget,
} from './api.js';
import { h, icon, $, clear, fmtBytes, fmtDate, mount } from './ui.js';
import { setupAuxWindow, toast, guard } from './chrome.js';
import { seedChangePasswordDialog } from './seed.js';

const SECTIONS = [
  { id: 'general',  label: 'Общие',            glyph: 'sliders-horizontal', render: renderGeneral },
  { id: 'security', label: 'Безопасность',     glyph: 'shield-check',       render: renderSecurity },
  { id: 'seed',     label: 'Сид-фразы',        glyph: 'seal',               render: renderSeed },
  { id: 'quick',    label: 'Быстрый доступ',   glyph: 'lightning',          render: renderQuick },
  { id: 'sync',     label: 'Синхронизация',    glyph: 'arrows-clockwise',   render: renderSync },
  { id: 'backups',  label: 'Резервные копии',  glyph: 'archive',            render: renderBackups },
  { id: 'diag',     label: 'Диагностика',      glyph: 'bug',                render: renderDiagnostics },
];

let settings = null;
let vaultStatus = null;
let seedInfo = null;
let current = 'security'; // раздел, открытый в макете
const refs = {};

// ── строительные блоки строки настройки ────────────────────────────────────

/** Строка «название + описание слева, орган управления справа». */
function setting(title, hint, control, { align = 'center' } = {}) {
  return h('div', {
    style: `display:flex;align-items:flex-${align === 'center' ? 'start' : align};` +
           'gap:20px;padding:14px 0;border-top:1px solid var(--color-divider)',
  },
    h('div', { style: 'width:230px;flex:none' },
      h('div', { style: 'font-size:13.5px' }, title),
      hint && h('div', { style: 'font-size:11.5px;color:var(--color-neutral-600)' }, hint)),
    h('div', { style: 'display:flex;align-items:center;gap:10px;flex-wrap:wrap' }, control));
}

/** Сегментированный переключатель. */
function seg(name, options, value, onPick) {
  return h('div', { class: 'seg' },
    options.map((o) => h('label', { class: 'seg-opt' },
      h('input', {
        type: 'radio', name, checked: o.value === value,
        onChange: () => onPick(o.value),
      }),
      o.label)));
}

/** Флажок-переключатель с подписью справа. */
function check(label, value, onChange) {
  return h('label', { class: 'radio check', style: 'gap:9px' },
    h('input', { type: 'checkbox', checked: value, onChange: (e) => onChange(e.target.checked) }),
    h('span', { class: 'dot' }),
    h('span', { style: 'font-size:13px' }, label));
}

async function patch(changes) {
  const next = { ...settings, ...changes };
  const saved = await guard(() => setSettings(next));
  if (!saved) {
    render(); // откатываем интерфейс к тому, что реально записано
    return;
  }
  settings = saved;
  render();
}

// ── разделы ────────────────────────────────────────────────────────────────

function renderGeneral() {
  return [
    h('h4', {}, 'Общие'),
    h('p', { style: 'font-size:12.5px;color:var(--color-neutral-500);margin-bottom:18px' },
      'Как выглядит главное окно и что происходит при запуске системы.'),

    setting('Вид главного окна', 'Оба варианта из макета',
      seg('mainview', [
        { value: 'panels', label: 'Три панели' },
        { value: 'table', label: 'Таблица' },
      ], settings.main_view, (v) => patch({ main_view: v }))),

    setting('Подсказки клавиш', 'Нижняя строка быстрого окна',
      check(settings.show_key_hints ? 'Показывать' : 'Скрыты',
        settings.show_key_hints, (v) => patch({ show_key_hints: v }))),

    setting('Запуск вместе с системой', 'Значок появится в трее',
      check(settings.launch_at_startup ? 'Включён' : 'Выключен',
        settings.launch_at_startup, (v) => patch({ launch_at_startup: v }))),

    setting('Файл хранилища', 'Путь к сейфу',
      h('div', { class: 'mono selectable', style: 'font-size:12px;color:var(--color-neutral-400);word-break:break-all;max-width:380px' },
        vaultStatus?.path || '—')),
  ];
}

function renderSecurity() {
  const lock = [
    { value: 60, label: '1 мин' },
    { value: 600, label: '10 мин' },
    { value: 3600, label: '1 час' },
    { value: null, label: 'Никогда' },
  ];
  const clip = [
    { value: 10, label: '10 с' },
    { value: 30, label: '30 с' },
    { value: 90, label: '90 с' },
    { value: null, label: 'Не чистить' },
  ];

  return [
    h('h4', {}, 'Безопасность'),
    h('p', { style: 'font-size:12.5px;color:var(--color-neutral-500);margin-bottom:18px' },
      'Хранилище шифруется на устройстве. Ключ выводится из мастер-пароля (Argon2id) и не хранится на диске.'),

    setting('Автоблокировка', 'При бездействии',
      seg('lock', lock, settings.autolock_secs ?? null, (v) => patch({ autolock_secs: v }))),

    setting('Очистка буфера обмена', 'После копирования секрета',
      seg('clip', clip, settings.clipboard_clear_secs ?? null, (v) => patch({ clipboard_clear_secs: v }))),

    setting('Горячая клавиша мини-окна', 'Работает поверх любых окон', [
      h('div', {
        class: 'mono',
        style: 'display:flex;align-items:center;gap:6px;padding:7px 12px;border:1px solid var(--color-accent);' +
               'border-radius:8px;font-size:13px;color:var(--color-accent)',
      }, prettyHotkey(settings.hotkey)),
      h('button', { class: 'btn btn-secondary', type: 'button', onClick: captureHotkey }, 'Изменить'),
    ]),

    setting('Вход по биометрии', 'Отпечаток или Windows Hello',
      h('div', { style: 'display:flex;align-items:center;gap:9px' },
        check('Включён', false, () => {
          toast('Биометрия появится в следующей версии — сейчас вход только по мастер-паролю.',
            { glyph: 'info', ms: 4200 });
        }),
        h('span', { class: 'tag tag-neutral' }, 'пока недоступно'))),

    setting('Скрывать секреты', 'Показывать только по запросу',
      check(settings.mask_secrets ? 'Всегда' : 'Показывать сразу',
        settings.mask_secrets, (v) => patch({ mask_secrets: v }))),

    h('div', {
      style: 'display:flex;align-items:center;gap:20px;padding:14px 0;' +
             'border-top:1px solid var(--color-divider);border-bottom:1px solid var(--color-divider)',
    },
      h('div', { style: 'width:230px;flex:none' },
        h('div', { style: 'font-size:13.5px' }, 'Мастер-пароль'),
        h('div', { style: 'font-size:11.5px;color:var(--color-neutral-600)' },
          vaultStatus?.unlocked ? 'Сейф разблокирован' : 'Сейф заблокирован')),
      h('div', { style: 'display:flex;gap:8px' },
        h('button', { class: 'btn btn-secondary', type: 'button', onClick: changeMaster }, 'Сменить'),
        h('button', { class: 'btn btn-ghost', type: 'button', onClick: recoveryKey },
          vaultStatus?.has_recovery_key ? 'Пересоздать ключ восстановления' : 'Ключ восстановления'))),

    refs.auditBanner = h('div', { style: 'margin-top:auto' }),
  ];
}

/**
 * Раздел сид-фраз. Здесь только то, что можно менять при закрытом хранилище:
 * какой файл подключён, как быстро раздел закрывается и что спрашивать перед
 * показом фразы. Сами записи живут в главном окне и только там.
 */
function renderSeed() {
  const lock = [
    { value: 60, label: '1 мин' },
    { value: 120, label: '2 мин' },
    { value: 300, label: '5 мин' },
  ];
  const hide = [
    { value: 15, label: '15 с' },
    { value: 30, label: '30 с' },
    { value: 60, label: '60 с' },
  ];

  const connected = seedInfo?.configured;

  return [
    h('h4', {}, 'Сид-фразы'),
    h('p', { style: 'font-size:12.5px;color:var(--color-neutral-500);margin-bottom:18px' },
      'Фразы восстановления кошельков хранятся в отдельном файле со своим ' +
      'мастер-паролем и своим ключом. Argon2id для него вчетверо дороже, чем ' +
      'у основного сейфа, а ключа восстановления нет намеренно.'),

    setting('Файл хранилища', connected ? 'Подключён' : 'Не подключён', [
      h('div', {
        class: 'mono selectable',
        style: 'font-size:12px;color:var(--color-neutral-400);word-break:break-all;max-width:340px',
      }, seedInfo?.path || '—'),
      h('button', {
        class: 'btn btn-secondary', type: 'button',
        onClick: () => pickSeedFile(connected ? 'open' : 'create'),
      }, connected ? 'Выбрать другой' : 'Создать…'),
      connected ? h('button', {
        class: 'btn btn-ghost', type: 'button', onClick: forgetSeedFile,
      }, 'Отключить') : h('button', {
        class: 'btn btn-ghost', type: 'button', onClick: () => pickSeedFile('open'),
      }, 'Подключить существующий…'),
    ]),

    setting('Автоблокировка раздела', 'Свои часы, короче общих',
      seg('seedlock', lock, settings.seed_autolock_secs,
        (v) => patch({ seed_autolock_secs: v }))),

    setting('Пароль перед показом', 'Подтверждать каждый показ фразы',
      check(settings.seed_require_password_on_reveal ? 'Спрашивать' : 'Не спрашивать',
        settings.seed_require_password_on_reveal,
        (v) => patch({ seed_require_password_on_reveal: v }))),

    setting('Фраза скрывается через', 'После показа на экране',
      seg('seedhide', hide, settings.seed_hide_after_secs,
        (v) => patch({ seed_hide_after_secs: v }))),

    connected ? setting('Мастер-пароль хранилища', 'Отдельный от пароля сейфа',
      h('button', {
        class: 'btn btn-secondary', type: 'button', onClick: seedChangePasswordDialog,
      }, 'Сменить')) : null,

    h('div', {
      style: 'margin-top:18px;padding:12px 14px;border-radius:10px;background:var(--color-surface);' +
             'display:flex;gap:12px;align-items:flex-start',
    },
      icon('info', { size: 18, color: 'var(--color-neutral-400)' }),
      h('div', { style: 'font-size:11.5px;line-height:1.6;color:var(--color-neutral-500)' },
        'Скопировать сид-фразу нельзя: команды копирования для этого раздела ' +
        'не существует. От снимка экрана и от фотографии монитора приложение ' +
        'защитить не может — переписывайте фразу на бумагу и держите её ' +
        'подальше от компьютера.')),
  ];
}

async function pickSeedFile(mode) {
  const picked = await guard(() => seedPickFile(mode));
  if (!picked) return;
  seedInfo = await guard(() => seedStatus(), { silent: true });
  render();
  toast(mode === 'create'
    ? 'Файл выбран — задайте мастер-пароль в разделе «Сид-фразы» главного окна'
    : 'Файл подключён', { glyph: 'seal', ms: 4200 });
}

async function forgetSeedFile() {
  if (!confirm('Отключить файл с сид-фразами?\n\nСам файл останется на диске.')) return;
  await guard(() => seedForget());
  seedInfo = await guard(() => seedStatus(), { silent: true });
  render();
}

function renderQuick() {
  return [
    h('h4', {}, 'Быстрый доступ'),
    h('p', { style: 'font-size:12.5px;color:var(--color-neutral-500);margin-bottom:18px' },
      'Что показывать по горячей клавише поверх остальных окон.'),

    setting('Вид быстрого окна', 'Три варианта из макета',
      seg('quickview', [
        { value: 'palette', label: 'Палитра' },
        { value: 'fields', label: 'Палитра с полями' },
        { value: 'tray', label: 'Мини-окно у трея' },
      ], settings.quick_view, (v) => patch({ quick_view: v }))),

    setting('Горячая клавиша', 'Сочетание регистрируется в системе', [
      h('div', {
        class: 'mono',
        style: 'padding:7px 12px;border:1px solid var(--color-accent);border-radius:8px;' +
               'font-size:13px;color:var(--color-accent)',
      }, prettyHotkey(settings.hotkey)),
      h('button', { class: 'btn btn-secondary', type: 'button', onClick: captureHotkey }, 'Изменить'),
    ]),

    setting('Проверить', 'Откроет быстрое окно прямо сейчас',
      h('button', {
        class: 'btn btn-secondary', type: 'button',
        onClick: () => guard(() => openWindow(settings.quick_view === 'tray' ? 'tray' : 'quick')),
      }, icon('lightning', { size: 15 }), 'Показать')),

    h('div', {
      style: 'margin-top:18px;padding:12px 14px;border-radius:10px;background:var(--color-surface);' +
             'display:flex;gap:12px;align-items:flex-start',
    },
      icon('info', { size: 18, color: 'var(--color-neutral-400)' }),
      h('div', { style: 'font-size:11.5px;line-height:1.6;color:var(--color-neutral-500)' },
        'В сеансе Wayland глобальные сочетания проходят через портал рабочего стола ' +
        'и поддерживаются не всеми окружениями. Если клавиша не срабатывает, ' +
        'мини-окно всегда открывается щелчком по значку в трее.')),
  ];
}

function renderSync() {
  return [
    h('h4', {}, 'Синхронизация'),
    h('p', { style: 'font-size:12.5px;color:var(--color-neutral-500);margin-bottom:18px' },
      'Хранилище живёт только на этом устройстве.'),
    h('div', { class: 'empty', style: 'align-items:flex-start;text-align:left;padding:24px 0' },
      icon('cloud-slash', { size: 30 }),
      h('div', { class: 'empty-title' }, 'Облачной синхронизации нет'),
      h('div', { class: 'empty-hint', style: 'max-width:460px' },
        'Файл сейфа — обычный файл: его можно положить в любую папку, которую ' +
        'синхронизирует стороннее приложение. Содержимое всё равно зашифровано, ' +
        'ключ выводится из мастер-пароля и на диск не попадает.'),
      h('div', { class: 'mono selectable', style: 'margin-top:10px;font-size:12px;color:var(--color-neutral-500);word-break:break-all' },
        vaultStatus?.path || '')),
  ];
}

function renderBackups() {
  const list = h('div', { style: 'display:flex;flex-direction:column;gap:7px;margin-top:6px' });

  guard(() => call('backups')).then((items) => {
    clear(list);
    if (!items || items.length === 0) {
      mount(list, h('div', { class: 'dim', style: 'font-size:12.5px;padding:8px 0' },
        'Копий пока нет — первая появится при следующем сохранении.'));
      return;
    }
    for (const b of items) {
      mount(list, h('div', { class: 'fieldbox' },
        icon('archive', { size: 16, color: 'var(--color-neutral-400)' }),
        h('div', { class: 'fb-text' },
          h('div', { style: 'font-size:13px' }, b.name),
          h('div', { style: 'font-size:11px;color:var(--color-neutral-600)' },
            `${fmtBytes(b.bytes)} · ${fmtDate(b.modified)}`))));
    }
  });

  return [
    h('h4', {}, 'Резервные копии'),
    h('p', { style: 'font-size:12.5px;color:var(--color-neutral-500);margin-bottom:18px' },
      'Перед каждой записью прежний файл сейфа копируется в подпапку backups. ' +
      'Хранятся пять последних; копии зашифрованы тем же ключом.'),
    h('div', { style: 'display:flex;gap:8px;margin-bottom:6px' },
      h('button', {
        class: 'btn btn-secondary', type: 'button',
        onClick: () => guard(() => call('open_backup_dir')),
      }, icon('folder-open', { size: 15 }), 'Открыть папку')),
    list,
  ];
}

function renderDiagnostics() {
  const path = h('div', {
    class: 'mono selectable',
    style: 'font-size:12px;color:var(--color-neutral-400);word-break:break-all',
  }, '…');
  const size = h('span', { class: 'dim', style: 'font-size:11.5px' }, '');
  const tail = h('pre', {
    class: 'mono selectable scroll',
    style: 'margin:0;padding:12px;border:1px solid var(--color-divider);border-radius:8px;' +
           'background:var(--color-surface);font-size:11px;line-height:1.6;max-height:260px;' +
           'white-space:pre-wrap;word-break:break-word;color:var(--color-neutral-300)',
  }, 'Загружаю…');

  const refresh = async () => {
    const info = await guard(() => logInfo(), { silent: true });
    if (info) {
      path.textContent = info.file || '—';
      size.textContent = info.exists ? `${fmtBytes(info.bytes)}` : 'файл ещё не создан';
    }
    const text = await guard(() => logTail(120), { silent: true });
    tail.textContent = (text || '').trim() || 'Журнал пуст.';
    tail.scrollTop = tail.scrollHeight;
  };
  refresh();

  return [
    h('h4', {}, 'Диагностика'),
    h('p', { style: 'font-size:12.5px;color:var(--color-neutral-500);margin-bottom:18px' },
      'Приложение ведёт журнал: действия, отказы и необработанные ошибки интерфейса. ' +
      'Паролей, логинов и названий записей в нём нет — только имена команд, коды ошибок ' +
      'и их тексты. Если что-то сломалось, приложите последние строки отсюда.'),

    setting('Файл журнала', null, h('div', { style: 'max-width:420px' }, path, size)),

    h('div', { style: 'display:flex;gap:8px;padding:14px 0;border-top:1px solid var(--color-divider)' },
      h('button', {
        class: 'btn btn-primary', type: 'button',
        onClick: () => guard(async () => {
          const text = await logTail(200);
          await call('copy_text', { text: text || '(журнал пуст)' });
          toast('Последние 200 строк скопированы');
        }),
      }, icon('copy', { size: 15 }), 'Скопировать журнал'),
      h('button', {
        class: 'btn btn-secondary', type: 'button', onClick: () => guard(() => openLogDir()),
      }, icon('folder-open', { size: 15 }), 'Открыть папку'),
      h('button', {
        class: 'btn btn-secondary', type: 'button', onClick: refresh,
      }, icon('arrows-clockwise', { size: 15 }), 'Обновить')),

    h('div', { class: 'kicker', style: 'margin-bottom:7px' }, 'Последние строки'),
    tail,
  ];
}

// ── действия ───────────────────────────────────────────────────────────────

/** `CmdOrCtrl+Shift+Space` → `Ctrl + Shift + Space`. */
function prettyHotkey(spec) {
  return (spec || '')
    .replace(/CmdOrCtrl/gi, 'Ctrl')
    .split('+')
    .map((p) => p.trim())
    .join(' + ');
}

/**
 * Ловит следующее сочетание клавиш. Одиночная клавиша не принимается:
 * глобальная горячая клавиша без модификатора отняла бы её у всех остальных
 * программ в системе.
 */
function captureHotkey() {
  const hint = h('div', { class: 'dialog-body dim' }, 'Нажмите сочетание…');
  const box = h('div', { class: 'dialog-backdrop' },
    h('div', { class: 'dialog' },
      h('div', { class: 'dialog-title' }, 'Новое сочетание'),
      hint,
      h('div', { class: 'dialog-actions' },
        h('button', { class: 'btn btn-secondary', type: 'button', onClick: close }, 'Отмена'))));

  function close() {
    window.removeEventListener('keydown', onKey, true);
    box.remove();
  }

  function onKey(e) {
    e.preventDefault();
    e.stopPropagation();
    if (e.key === 'Escape') return close();

    const mods = [];
    if (e.ctrlKey || e.metaKey) mods.push('CmdOrCtrl');
    if (e.shiftKey) mods.push('Shift');
    if (e.altKey) mods.push('Alt');

    const key = normaliseKey(e);
    if (!key) return; // нажат только модификатор — ждём дальше
    if (mods.length === 0) {
      hint.textContent = 'Нужен хотя бы один модификатор — Ctrl, Alt или Shift.';
      return;
    }
    close();
    patch({ hotkey: [...mods, key].join('+') });
  }

  window.addEventListener('keydown', onKey, true);
  mount(document.body, box);
}

function normaliseKey(e) {
  if (['Control', 'Shift', 'Alt', 'Meta'].includes(e.key)) return null;
  if (e.code.startsWith('Key')) return e.code.slice(3);
  if (e.code.startsWith('Digit')) return e.code.slice(5);
  if (e.code.startsWith('F') && /^F\d+$/.test(e.code)) return e.code;
  const named = { Space: 'Space', Enter: 'Enter', Backslash: 'Backslash', Period: 'Period', Comma: 'Comma' };
  return named[e.code] || null;
}

function changeMaster() {
  if (!vaultStatus?.unlocked) {
    toast('Сначала разблокируйте сейф.', { error: true });
    return;
  }
  const cur = h('input', { class: 'input mono', type: 'password', placeholder: 'Текущий мастер-пароль' });
  const a = h('input', { class: 'input mono', type: 'password', placeholder: 'Новый мастер-пароль' });
  const b = h('input', { class: 'input mono', type: 'password', placeholder: 'Ещё раз' });
  const err = h('div', { style: 'font-size:11.5px;color:#e0808f;min-height:16px' });

  const box = h('div', { class: 'dialog-backdrop' },
    h('div', { class: 'dialog' },
      h('div', { class: 'dialog-title' }, 'Смена мастер-пароля'),
      h('div', { class: 'dialog-body dim', style: 'font-size:12px' },
        'Записи не перешифровываются — меняется только обёртка ключа, поэтому операция мгновенна.'),
      cur, a, b, err,
      h('div', { class: 'dialog-actions' },
        h('button', { class: 'btn btn-secondary', type: 'button', onClick: () => box.remove() }, 'Отмена'),
        h('button', { class: 'btn btn-primary', type: 'button', onClick: submit }, 'Сменить'))));

  async function submit() {
    if (a.value !== b.value) { err.textContent = 'Новые пароли не совпадают.'; return; }
    if (a.value.length < 8) { err.textContent = 'Нужно не меньше 8 символов.'; return; }
    try {
      await call('change_master_password', { current: cur.value, new: a.value });
      box.remove();
      toast('Мастер-пароль изменён');
    } catch (e) {
      err.textContent = e.message;
    }
  }

  mount(document.body, box);
  cur.focus();
}

async function recoveryKey() {
  if (!vaultStatus?.unlocked) {
    toast('Сначала разблокируйте сейф.', { error: true });
    return;
  }
  const again = vaultStatus.has_recovery_key;
  const ok = confirm(again
    ? 'Создать новый ключ восстановления? Прежний перестанет работать.'
    : 'Создать ключ восстановления? Он показывается один раз — сохраните его отдельно от компьютера.');
  if (!ok) return;

  const key = await guard(() => call('create_recovery_key'));
  if (!key) return;

  const box = h('div', { class: 'dialog-backdrop' },
    h('div', { class: 'dialog', style: 'width:min(520px,100%)' },
      h('div', { class: 'dialog-title' }, 'Ключ восстановления'),
      h('div', { class: 'dialog-body' },
        'Этим ключом сейф открывается без мастер-пароля. Больше он не будет показан: ' +
        'в хранилище лежит только обёртка, самого ключа там нет.'),
      h('div', {
        class: 'mono selectable',
        style: 'padding:14px;border:1px solid var(--color-accent);border-radius:10px;' +
               'background:color-mix(in srgb,var(--color-accent) 7%,transparent);' +
               'font-size:14px;line-height:1.8;word-break:break-all;letter-spacing:.04em',
      }, key),
      h('div', { class: 'dialog-actions' },
        h('button', {
          class: 'btn btn-secondary', type: 'button',
          onClick: () => guard(async () => { await call('copy_text', { text: key }); toast('Ключ скопирован'); }),
        }, icon('copy', { size: 15 }), 'Копировать'),
        h('button', {
          class: 'btn btn-primary', type: 'button',
          onClick: async () => { box.remove(); vaultStatus = await status(); render(); },
        }, 'Я сохранил ключ'))));
  mount(document.body, box);
}

/** Плашка аудита внизу раздела «Безопасность». */
async function fillAuditBanner() {
  if (!refs.auditBanner || !vaultStatus?.unlocked) return;
  const report = await guard(() => auditReport(), { silent: true });
  if (!report) return;

  const total = report.findings.length;
  const parts = [
    report.weak && `${report.weak} слабых`,
    report.reused && `${report.reused} повторов`,
    report.stale && `${report.stale} давних`,
    report.expiring && `${report.expiring} по сроку`,
  ].filter(Boolean).join(' · ');

  mount(clear(refs.auditBanner), h('div', {
    style: 'display:flex;align-items:center;gap:12px;padding:12px 14px;border-radius:10px;' +
           'background:var(--color-surface);box-shadow:var(--shadow-sm)',
  },
    icon(total ? 'shield-warning' : 'shield-check', { size: 20, color: 'var(--color-accent)' }),
    h('div', { style: 'flex:1' },
      h('div', { style: 'font-size:13px' },
        total ? `Аудит: ${total} ${total === 1 ? 'запись требует' : 'записей требуют'} внимания`
              : 'Аудит: замечаний нет'),
      h('div', { style: 'font-size:11.5px;color:var(--color-neutral-500)' },
        parts || 'Все пароли стойкие и не повторяются')),
    total ? h('button', {
      class: 'btn btn-primary', type: 'button',
      onClick: () => guard(async () => {
        await openWindow('main');
        await window.__TAURI__.event.emit('show-audit');
      }),
    }, 'Проверить') : null));
}

// ── сборка ─────────────────────────────────────────────────────────────────

function render() {
  mount(clear(refs.nav), SECTIONS.map((s) => h('button', {
    class: 'side-item', type: 'button',
    'aria-current': s.id === current ? 'true' : null,
    onClick: () => { current = s.id; render(); },
  }, icon(s.glyph), h('span', {}, s.label))));

  mount(clear(refs.body), SECTIONS.find((s) => s.id === current).render());
  if (current === 'security') fillAuditBanner();
}

async function build() {
  const root = setupAuxWindow('Настройки', { buttons: ['min', 'close'] });
  settings = await getSettings();
  vaultStatus = await status();
  seedInfo = await guard(() => seedStatus(), { silent: true });

  refs.nav = h('div', { style: 'display:flex;flex-direction:column;gap:1px' });
  refs.body = h('div', {
    class: 'scroll',
    style: 'flex:1;min-width:0;padding:24px 28px;display:flex;flex-direction:column',
  });

  mount(root, h('div', { class: 'screen-body' },
    h('div', {
      class: 'col',
      style: 'width:196px;flex:none;border-right:1px solid var(--color-divider);padding:16px 10px',
    },
      refs.nav,
      h('div', { class: 'spacer', style: 'padding:9px;font-size:10.5px;color:var(--color-neutral-700);line-height:1.5' },
        `Сейф 1.4 · формат хранилища v${vaultStatus.format_version}`)),
    refs.body));

  render();
}

build();
