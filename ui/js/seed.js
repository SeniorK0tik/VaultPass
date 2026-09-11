// Раздел сид-фраз — отдельное хранилище внутри главного окна.
//
// Правила этого экрана отличаются от остальных, и отличаются намеренно:
//
// * кнопки «копировать» здесь нет ни у одного поля — соответствующей команды
//   нет и в ядре, так что копировать нечем даже по ошибке;
// * фраза появляется только в отдельном окне показа, под размытием, пока
//   зажата кнопка, и исчезает сама через несколько секунд;
// * на окне показа перехвачены выделение, перетаскивание, контекстное меню и
//   Ctrl+C — DOM-узлы с фразой удаляются сразу после закрытия;
// * уход из раздела, скрытие главного окна и простая пауза в работе закрывают
//   хранилище.
//
// Чего разметка не может и не притворяется, что может: затереть строку в
// памяти вебвью (в JavaScript строки неизменяемы) и помешать снимку экрана
// или фотографии монитора. Об этом честно сказано в README.

import {
  listen,
  seedStatus, seedPickFile, seedForget, seedCreate, seedUnlock, seedLock,
  seedChangePassword, seedPing, seedCountdown, seedList, seedGet, seedAdd,
  seedUpdateDetails, seedReplacePhrase, seedDelete, seedReveal,
  seedRevealPassphrase, seedVerifyPhrase, seedClearClipboard,
  bip39Suggest, bip39Check,
} from './api.js';
import { h, icon, clear, mount, nextPaint, fmtDate, fmtAgo, fmtCountdown, plural } from './ui.js';
import { toast, guard } from './chrome.js';

/** Сколько слов бывает в фразе — то же, что знает ядро. */
const LENGTHS = [12, 15, 18, 21, 24];

const state = {
  status: null,
  entries: [],
  selectedId: null,
  detail: null,
  /// Путь только что выбран под новый файл — показываем создание, а не
  /// «файл не найден».
  pendingCreate: false,
};

let box = null;
let countdownTimer = null;

// ═══ точка входа ═══════════════════════════════════════════════════════════

/**
 * Собирает раздел. Возвращает узел сразу, а содержимое дорисовывает, когда
 * ответит ядро, — как это делают остальные экраны.
 */
export function seedSection() {
  box = h('div', { class: 'col grow', style: 'min-width:0' });
  trackSeedActivity(box);
  reload();
  return box;
}

/** Уход из раздела закрывает хранилище: открытым оно живёт только на экране. */
export function leaveSeedSection() {
  clearInterval(countdownTimer);
  countdownTimer = null;
  state.selectedId = null;
  state.detail = null;
  // Узел раздела сейчас будет выброшен главным окном: обнуляем ссылку, чтобы
  // ответ на событие блокировки не перерисовывал то, чего уже нет на экране.
  box = null;
  for (const node of document.querySelectorAll('.dialog-backdrop.is-seed')) node.remove();
  seedLock().catch(() => {});
}

async function reload() {
  state.status = await guard(() => seedStatus(), { silent: true });
  if (state.status?.unlocked) {
    state.entries = (await guard(() => seedList(), { silent: true })) || [];
    if (!state.entries.some((e) => e.id === state.selectedId)) {
      state.selectedId = state.entries[0]?.id ?? null;
    }
    state.detail = state.selectedId
      ? await guard(() => seedGet(state.selectedId), { silent: true })
      : null;
  } else {
    state.entries = [];
    state.detail = null;
  }
  paint();
}

function paint() {
  if (!box) return;
  const st = state.status;
  clear(box);

  if (!st) return;
  if (!st.configured) return mount(box, connectScreen());
  if (st.unlocked) {
    mount(box, vaultView());
    // Отсчёт заводится после того, как узлы встали на место: до этого
    // искать в `box` нечего — он ещё пуст.
    startCountdown();
    return box;
  }
  if (!st.exists) {
    return mount(box, state.pendingCreate ? createScreen() : missingScreen());
  }
  return mount(box, unlockScreen());
}

/**
 * Отметки активности раздела. Часы у него свои: возня с паролями в соседней
 * колонке не должна продлевать жизнь открытым сид-фразам, поэтому общий
 * `ping` сюда не годится.
 */
function trackSeedActivity(node) {
  let pending = false;
  const mark = () => {
    if (pending || !state.status?.unlocked) return;
    pending = true;
    setTimeout(() => {
      pending = false;
      seedPing().catch(() => {});
    }, 3000);
  };
  for (const ev of ['keydown', 'pointerdown', 'pointermove', 'wheel']) {
    node.addEventListener(ev, mark, { passive: true });
  }
}

// ═══ экраны до разблокировки ═══════════════════════════════════════════════

/** Общая рамка для всех экранов «раздел ещё не открыт». */
function gate(glyph, title, subtitle, ...rest) {
  return h('div', {
    class: 'col scroll grow',
    style: 'align-items:center;justify-content:center;padding:40px 32px;gap:0',
  },
    h('div', {
      style: 'width:54px;height:54px;border-radius:14px;border:1px solid var(--color-accent);' +
             'display:grid;place-items:center;color:var(--color-accent);' +
             'box-shadow:0 0 32px color-mix(in srgb,var(--color-accent) 20%,transparent)',
    }, icon(glyph, { size: 26 })),
    h('h3', { style: 'margin:20px 0 5px' }, title),
    h('div', {
      style: 'font-size:12.5px;color:var(--color-neutral-500);max-width:420px;' +
             'text-align:center;line-height:1.6',
    }, subtitle),
    rest);
}

function connectScreen() {
  return gate('seal', 'Сид-фразы',
    'Фразы восстановления кошельков лежат в отдельном файле со своим ' +
    'мастер-паролем — не в основном сейфе. Файл можно держать где угодно, ' +
    'хоть на съёмном носителе: приложение запомнит только путь к нему.',

    h('div', { style: 'display:flex;gap:8px;margin-top:22px' },
      h('button', {
        class: 'btn btn-primary', type: 'button',
        onClick: () => pickFile('create'),
      }, icon('file-plus', { size: 15 }), 'Создать файл…'),
      h('button', {
        class: 'btn btn-secondary', type: 'button',
        onClick: () => pickFile('open'),
      }, icon('folder-open', { size: 15 }), 'Подключить существующий…')),

    note('Мастер-пароль этого файла не связан с паролем основного сейфа и ' +
         'нигде не хранится. Ключа восстановления у него нет намеренно: ' +
         'вторая дверь к сид-фразам — это вторая дверь для того, кто их ищет.'));
}

function missingScreen() {
  return gate('warning-circle', 'Файл не найден',
    `Раздел подключён к файлу, которого сейчас нет по этому пути. ` +
    `Возможно, носитель не подключён или файл переехал.`,
    h('div', {
      class: 'mono', style: 'font-size:11.5px;color:var(--color-neutral-600);' +
             'margin-top:14px;max-width:460px;text-align:center;word-break:break-all',
    }, state.status.path),
    h('div', { style: 'display:flex;gap:8px;margin-top:20px' },
      h('button', {
        class: 'btn btn-primary', type: 'button', onClick: () => pickFile('open'),
      }, icon('folder-open', { size: 15 }), 'Указать заново'),
      h('button', {
        class: 'btn btn-secondary', type: 'button', onClick: forget,
      }, 'Отключить раздел')));
}

function createScreen() {
  const refs = {};
  const err = errorLine();

  const submit = async () => {
    const value = refs.pw.value;
    const min = state.status.min_password_len;
    if (value.length < min) {
      err.textContent = `Мастер-пароль короче ${min} символов.`;
      return;
    }
    if (value !== refs.again.value) {
      err.textContent = 'Пароли не совпадают.';
      return;
    }
    refs.go.disabled = true;
    refs.go.textContent = 'Создаём…';
    try {
      // Argon2id на 256 МиБ занимает около секунды — экран должен успеть
      // перерисоваться до того, как поток встанет.
      await nextPaint();
      await seedCreate(value);
      refs.pw.value = '';
      refs.again.value = '';
      state.pendingCreate = false;
      await reload();
    } catch (e) {
      err.textContent = e.message;
      refs.go.disabled = false;
      refs.go.textContent = 'Создать хранилище';
    }
  };

  refs.pw = passwordInput({ onEnter: submit, onInput: () => { err.textContent = ''; } });
  refs.again = passwordInput({ onEnter: submit });
  refs.go = h('button', {
    class: 'btn btn-primary btn-block', style: 'margin-top:14px;height:40px', type: 'button',
    onClick: submit,
  }, 'Создать хранилище');

  const form = h('div', { style: 'width:100%;max-width:380px;margin-top:22px' },
    h('div', { class: 'field' },
      h('label', {}, `Придумайте мастер-пароль (от ${state.status.min_password_len} символов)`),
      refs.pw),
    h('div', { class: 'field', style: 'margin-top:10px' },
      h('label', {}, 'Повторите'), refs.again),
    err,
    refs.go);

  const out = gate('seal', 'Новое хранилище сид-фраз',
    'Пароль нигде не хранится: из него выводится ключ шифрования файла. ' +
    'Забыть его — значит потерять доступ к фразам, а вместе с ними к кошелькам. ' +
    'Ключа восстановления у этого хранилища нет.',
    form,
    h('div', {
      class: 'mono', style: 'font-size:11px;color:var(--color-neutral-600);margin-top:16px;' +
             'max-width:460px;text-align:center;word-break:break-all',
    }, state.status.path));

  setTimeout(() => refs.pw.focus(), 0);
  return out;
}

function unlockScreen() {
  const refs = {};
  const err = errorLine();

  const submit = async () => {
    const value = refs.pw.value;
    if (!value) { err.textContent = 'Введите мастер-пароль.'; return; }
    refs.go.disabled = true;
    refs.go.textContent = 'Открываем…';
    try {
      await nextPaint();
      await seedUnlock(value);
      refs.pw.value = '';
      await reload();
    } catch (e) {
      err.textContent = e.message;
      refs.go.disabled = false;
      refs.go.textContent = 'Открыть';
      refs.pw.select();
    }
  };

  refs.pw = passwordInput({ onEnter: submit, onInput: () => { err.textContent = ''; } });
  refs.go = h('button', {
    class: 'btn btn-primary btn-block', style: 'margin-top:14px;height:40px', type: 'button',
    onClick: submit,
  }, 'Открыть');

  const out = gate('lock-key', 'Сид-фразы',
    'Отдельное хранилище с отдельным мастер-паролем.',
    h('div', { style: 'width:100%;max-width:380px;margin-top:22px' },
      h('div', { class: 'field' },
        h('label', {}, 'Мастер-пароль хранилища сид-фраз'), refs.pw),
      err,
      refs.go,
      h('div', { style: 'display:flex;gap:8px;margin-top:8px' },
        h('button', {
          class: 'btn btn-ghost', style: 'font-size:11.5px;padding-inline:0', type: 'button',
          onClick: forget,
        }, 'Отключить файл'),
        h('button', {
          class: 'btn btn-ghost', style: 'font-size:11.5px;margin-left:auto', type: 'button',
          onClick: () => pickFile('open'),
        }, 'Выбрать другой файл'))),
    h('div', {
      class: 'mono', style: 'font-size:11px;color:var(--color-neutral-600);margin-top:18px;' +
             'max-width:460px;text-align:center;word-break:break-all',
    }, state.status.path));

  setTimeout(() => refs.pw.focus(), 0);
  return out;
}

async function pickFile(mode) {
  const picked = await guard(() => seedPickFile(mode));
  if (!picked) return; // диалог закрыли — молча
  state.pendingCreate = mode === 'create';
  await reload();
}

async function forget() {
  if (!confirm('Отключить файл с сид-фразами?\n\nСам файл останется на диске — приложение просто перестанет его знать.')) return;
  await guard(() => seedForget());
  state.pendingCreate = false;
  await reload();
}

// ═══ открытое хранилище ════════════════════════════════════════════════════

function vaultView() {
  return h('div', { style: 'display:flex;flex:1;min-height:0;min-width:0' },
    listColumn(), detailColumn());
}

function listColumn() {
  const list = h('div', { class: 'scroll grow' });

  if (state.entries.length === 0) {
    mount(list, h('div', { class: 'empty' },
      icon('seal'),
      h('div', { class: 'empty-title' }, 'Здесь пока пусто'),
      h('div', { class: 'empty-hint' }, 'Нажмите + и добавьте фразу кошелька.')));
  } else {
    for (const e of state.entries) mount(list, listRow(e));
  }

  return h('div', {
    class: 'col',
    style: 'width:300px;flex:none;border-right:1px solid var(--color-divider)',
  },
    h('div', { style: 'display:flex;align-items:center;gap:8px;padding:13px 14px 10px;flex:none' },
      h('span', { style: 'font-size:13px;font-weight:500' }, 'Сид-фразы'),
      h('span', { style: 'font-size:11.5px;color:var(--color-neutral-600)' }, state.entries.length),
      h('div', { style: 'margin-left:auto;display:flex;gap:2px' },
        h('button', {
          class: 'btn btn-icon btn-ghost', style: 'width:28px;height:28px', type: 'button',
          title: 'Закрыть раздел', onClick: lockNow,
        }, icon('lock-key', { size: 15 })),
        h('button', {
          class: 'btn btn-icon btn-primary', style: 'width:28px;height:28px', type: 'button',
          title: 'Новая фраза', onClick: addDialog,
        }, icon('plus', { size: 15 })))),
    list,
    footer());
}

function listRow(entry) {
  const chosen = entry.id === state.selectedId;
  return h('button', {
    class: 'row', type: 'button', 'aria-selected': chosen ? 'true' : 'false',
    onClick: async () => {
      state.selectedId = entry.id;
      state.detail = await guard(() => seedGet(entry.id), { silent: true });
      paint();
    },
  },
    h('div', { class: `row-icon${chosen ? ' is-accent' : ''}` }, icon('seal', { size: 15 })),
    h('div', { class: 'row-text' },
      h('div', { class: 'row-title' }, entry.title),
      h('div', { class: 'row-sub' },
        [entry.wallet, entry.network].filter(Boolean).join(' · ') ||
        `${entry.word_count} ${plural(entry.word_count, 'слово', 'слова', 'слов')}`)),
    entry.standard ? null : h('span', { class: 'tag tag-neutral' }, 'своя'));
}

/** Нижняя строка: чем этот раздел живёт и когда закроется. */
function footer() {
  const left = h('span', { id: 'seed-left' }, '');
  return h('div', {
    style: 'display:flex;align-items:center;gap:7px;padding:9px 14px;flex:none;' +
           'border-top:1px solid var(--color-divider);font-size:10.5px;' +
           'color:var(--color-neutral-600)',
  },
    icon('lock-simple-open', { size: 13, color: 'var(--color-accent)' }),
    h('span', {}, 'Отдельный файл'),
    h('span', { style: 'margin-left:auto' }, left));
}

function startCountdown() {
  clearInterval(countdownTimer);
  const tick = async () => {
    const node = box?.querySelector('#seed-left');
    if (!node) return;
    const left = await guard(() => seedCountdown(), { silent: true });
    node.textContent = left === null || left === undefined
      ? '' : `закроется через ${fmtCountdown(left)}`;
  };
  tick();
  countdownTimer = setInterval(tick, 5000);
}

async function lockNow() {
  await guard(() => seedLock());
  await reload();
}

// ── карточка ───────────────────────────────────────────────────────────────

function detailColumn() {
  const entry = state.detail;
  if (!entry) {
    return h('div', { class: 'col grow' },
      h('div', { class: 'empty' },
        icon('seal'),
        h('div', { class: 'empty-title' }, 'Выберите запись'),
        h('div', { class: 'empty-hint' }, 'Слева — кошельки, здесь появятся их сведения.')));
  }

  const meta = h('div', { style: 'display:flex;flex-direction:column;gap:7px;margin-top:18px' },
    metaRow('Кошелёк', entry.wallet || '—'),
    metaRow('Сеть', entry.network || '—'),
    metaRow('Путь выведения', entry.derivation || '—'),
    metaRow('Слов в фразе', `${entry.word_count}${entry.standard ? '' : ' · свой словарь'}`),
    metaRow('Дополнительное слово', entry.has_passphrase ? 'есть' : 'нет'),
    metaRow('Добавлено', fmtDate(entry.created_at)),
    metaRow('Последний показ',
      entry.last_viewed_at
        ? `${fmtAgo(entry.last_viewed_at)} · ${entry.view_count} ${plural(entry.view_count, 'раз', 'раза', 'раз')}`
        : 'ни разу'));

  return h('div', { class: 'col grow' },
    h('div', { class: 'scroll grow', style: 'padding:20px 24px' },
      h('div', { style: 'display:flex;align-items:flex-start;gap:14px' },
        h('div', {
          class: 'row-icon is-accent',
          style: 'width:46px;height:46px;border-radius:12px;font-size:21px;flex:none',
        }, icon('seal', { size: 22 })),
        h('div', { style: 'flex:1;min-width:0' },
          h('h3', { style: 'margin:0 0 3px' }, entry.title),
          h('div', { style: 'font-size:12px;color:var(--color-neutral-500)' },
            `${entry.word_count} ${plural(entry.word_count, 'слово', 'слова', 'слов')}` +
            (entry.has_passphrase ? ' · с дополнительным словом' : ''))),
        h('button', {
          class: 'btn btn-secondary btn-icon', type: 'button', title: 'Удалить запись',
          onClick: () => deleteDialog(entry),
        }, icon('trash', { size: 16 }))),

      h('div', { style: 'display:flex;gap:8px;margin:18px 0 0;flex-wrap:wrap' },
        h('button', {
          class: 'btn btn-primary', type: 'button', onClick: () => revealDialog(entry),
        }, icon('eye', { size: 15 }), 'Показать фразу'),
        h('button', {
          class: 'btn btn-secondary', type: 'button', onClick: () => verifyDialog(entry),
        }, icon('check-square', { size: 15 }), 'Проверить запись'),
        entry.has_passphrase ? h('button', {
          class: 'btn btn-secondary', type: 'button', onClick: () => revealPassphraseDialog(entry),
        }, icon('key', { size: 15 }), 'Дополнительное слово') : null),

      note('Фраза показывается под размытием, пока удерживается кнопка, и ' +
           'исчезает сама. Скопировать её нельзя: такой команды в приложении нет.'),

      meta,

      entry.note ? h('div', {
        style: 'margin-top:14px;padding:10px 12px;border:1px solid var(--color-divider);border-radius:8px',
      },
        h('div', { class: 'fb-label' }, 'Заметка'),
        h('div', {
          style: 'font-size:13px;white-space:pre-wrap;color:color-mix(in srgb,var(--color-text) 78%,transparent)',
        }, entry.note)) : null,

      h('div', { style: 'display:flex;gap:8px;margin-top:18px;flex-wrap:wrap' },
        h('button', {
          class: 'btn btn-secondary', type: 'button', onClick: () => detailsDialog(entry),
        }, icon('pencil-simple', { size: 15 }), 'Изменить сведения'),
        h('button', {
          class: 'btn btn-secondary', type: 'button', onClick: () => replaceDialog(entry),
        }, icon('arrows-clockwise', { size: 15 }), 'Заменить фразу'))));
}

function metaRow(label, value) {
  return h('div', {
    style: 'display:flex;justify-content:space-between;gap:12px;font-size:12px;' +
           'color:var(--color-neutral-500)',
  },
    h('span', {}, label),
    h('span', { style: 'color:var(--color-text);text-align:right' }, value));
}

// ═══ общие мелочи ══════════════════════════════════════════════════════════

function errorLine() {
  return h('div', {
    style: 'font-size:11.5px;color:#e0808f;min-height:17px;margin-top:8px',
  });
}

function note(text) {
  return h('div', {
    style: 'font-size:11.5px;color:var(--color-neutral-600);line-height:1.6;' +
           'max-width:460px;margin-top:14px',
  }, text);
}

function passwordInput({ onEnter, onInput } = {}) {
  return h('input', {
    class: 'input mono', type: 'password',
    style: 'letter-spacing:.18em', autocomplete: 'off',
    onKeyDown: (e) => { if (e.key === 'Enter' && onEnter) onEnter(); },
    onInput: onInput || null,
  });
}

/**
 * Запрещает вынести содержимое узла наружу мышью или клавиатурой.
 *
 * Выделение в приложении и так выключено (`user-select` в app.css), но здесь
 * это не оформление, а требование, поэтому сказано явно и рядом с фразой.
 */
function forbidCopying(node) {
  for (const ev of ['copy', 'cut', 'contextmenu', 'dragstart', 'selectstart']) {
    node.addEventListener(ev, (e) => e.preventDefault());
  }
  node.style.userSelect = 'none';
  node.style.webkitUserSelect = 'none';
}

/** Окно поверх экрана. Возвращает узел и функцию закрытия. */
function openDialog({ title, width = 460 }) {
  let onClose = null;
  const body = h('div', { class: 'col', style: 'gap:0' });
  const back = h('div', {
    // Своя метка: по ней окна раздела закрываются при блокировке хранилища,
    // а чужие (например, смена мастер-пароля сейфа в настройках) — нет.
    class: 'dialog-backdrop is-seed',
    onClick: (e) => { if (e.target === back) close(); },
  },
    h('div', { class: 'dialog', style: `width:${width}px;max-width:calc(100vw - 48px)` },
      h('div', { class: 'dialog-title' }, title),
      body));

  function close() {
    back.remove();
    document.removeEventListener('keydown', onKey, true);
    if (onClose) onClose();
  }
  function onKey(e) {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(); }
  }

  document.addEventListener('keydown', onKey, true);
  mount(document.body, back);
  return { node: back, body, close, whenClosed: (fn) => { onClose = fn; } };
}

function actions(...children) {
  return h('div', { class: 'dialog-actions' }, children);
}

// ═══ показ фразы ═══════════════════════════════════════════════════════════

/**
 * Просит мастер-пароль, если так настроено, и вызывает `action(password)`.
 * Отдельная ступень: открытый раздел сам по себе права видеть фразу не даёт.
 */
function askThen(dlg, { hint, buttonLabel, action }) {
  const err = errorLine();
  // Обработчик обёрнут в стрелку намеренно: `go` объявлен ниже, и передать
  // его сюда значением нельзя — на этой строке он ещё не существует.
  const pw = passwordInput({ onEnter: () => go() });
  const go = async () => {
    const need = state.status.require_password_on_reveal;
    if (need && !pw.value) { err.textContent = 'Введите мастер-пароль.'; return; }
    btn.disabled = true;
    btn.textContent = 'Проверяем…';
    try {
      await nextPaint();
      await action(need ? pw.value : null);
      pw.value = '';
    } catch (e) {
      err.textContent = e.message;
      btn.disabled = false;
      btn.textContent = buttonLabel;
      if (e.code === 'seed_locked_after_failures' || e.code === 'seed_locked') {
        dlg.close();
        reload();
      }
      return;
    }
  };
  const btn = h('button', { class: 'btn btn-primary', type: 'button', onClick: go }, buttonLabel);

  mount(dlg.body,
    h('div', { class: 'dialog-body', style: 'font-size:12.5px' }, hint),
    state.status.require_password_on_reveal
      ? h('div', { class: 'field', style: 'margin-top:14px' },
          h('label', {}, 'Мастер-пароль хранилища сид-фраз'), pw)
      : null,
    err,
    actions(
      h('button', { class: 'btn btn-secondary', type: 'button', onClick: dlg.close }, 'Отмена'),
      btn));

  setTimeout(() => pw.focus(), 0);
}

function revealDialog(entry) {
  const dlg = openDialog({ title: entry.title, width: 560 });
  askThen(dlg, {
    hint: 'Фраза появится под размытием. Чтобы прочитать её, удерживайте кнопку — ' +
          'и перепишите на бумагу: скопировать её нельзя.',
    buttonLabel: 'Показать',
    action: async (password) => {
      const revealed = await seedReveal(entry.id, password);
      showPhrase(dlg, revealed.words, revealed.hide_after_secs);
      reload();
    },
  });
}

/** Сама сетка со словами: размытая, пока не зажата кнопка, и с отсчётом. */
function showPhrase(dlg, words, hideAfter, { numbered = true } = {}) {
  clear(dlg.body);

  const cells = words.map((w, i) => h('div', { class: 'seed-word' },
    numbered ? h('span', { class: 'seed-num' }, i + 1) : null,
    h('span', { class: 'seed-text' }, w)));

  const grid = h('div', { class: `seed-grid is-veiled${numbered ? '' : ' is-single'}` }, cells);
  forbidCopying(grid);

  const left = h('span', {}, '');
  let seconds = hideAfter;

  const hold = h('button', {
    class: 'btn btn-primary btn-block', style: 'margin-top:12px', type: 'button',
  }, icon('eye', { size: 15 }), 'Удерживайте, чтобы прочитать');

  const show = () => grid.classList.remove('is-veiled');
  const veil = () => grid.classList.add('is-veiled');
  for (const ev of ['pointerdown']) hold.addEventListener(ev, show);
  for (const ev of ['pointerup', 'pointerleave', 'pointercancel', 'blur']) {
    hold.addEventListener(ev, veil);
  }
  // Клавиатура: пробел и Enter на кнопке работают так же, как удержание мыши.
  hold.addEventListener('keydown', (e) => { if (e.key === ' ' || e.key === 'Enter') show(); });
  hold.addEventListener('keyup', veil);

  const timer = setInterval(() => {
    seconds -= 1;
    left.textContent = `скроется через ${fmtCountdown(Math.max(seconds, 0))}`;
    if (seconds <= 0) dlg.close();
  }, 1000);
  left.textContent = `скроется через ${fmtCountdown(seconds)}`;

  // Ушли из окна — фраза не должна остаться на экране за спиной.
  const onBlur = () => dlg.close();
  window.addEventListener('blur', onBlur);

  // Пока фраза на экране, сочетания копирования не работают вовсе: без этого
  // Ctrl+C сработал бы на случайном выделении в другом месте окна.
  const onKey = (e) => {
    const combo = (e.ctrlKey || e.metaKey) && ['c', 'x', 'insert'].includes(e.key.toLowerCase());
    if (combo) { e.preventDefault(); e.stopPropagation(); toast('Сид-фразу нельзя скопировать', { error: true }); }
  };
  document.addEventListener('keydown', onKey, true);

  dlg.whenClosed(() => {
    clearInterval(timer);
    window.removeEventListener('blur', onBlur);
    document.removeEventListener('keydown', onKey, true);
    // Узлы со словами уходят вместе с окном; ссылки на строки отпускаем.
    for (const c of cells) clear(c);
    words.length = 0;
  });

  mount(dlg.body,
    h('div', { class: 'dialog-body', style: 'font-size:12px' },
      'Перепишите фразу на бумагу и держите её подальше от этого компьютера.'),
    grid,
    hold,
    h('div', {
      style: 'display:flex;align-items:center;gap:8px;margin-top:10px;' +
             'font-size:11px;color:var(--color-neutral-600)',
    },
      icon('clock-countdown', { size: 14 }), left,
      h('button', {
        class: 'btn btn-ghost', style: 'margin-left:auto;font-size:11.5px', type: 'button',
        onClick: dlg.close,
      }, 'Скрыть сейчас')));
}

function revealPassphraseDialog(entry) {
  const dlg = openDialog({ title: 'Дополнительное слово' });
  askThen(dlg, {
    hint: 'Слово-пароль показывается отдельно от фразы: одно действие — один секрет на экране.',
    buttonLabel: 'Показать',
    action: async (password) => {
      const value = await seedRevealPassphrase(entry.id, password);
      showPhrase(dlg, [value], state.status.hide_after_secs, { numbered: false });
    },
  });
}

// ═══ ввод фразы ════════════════════════════════════════════════════════════

/**
 * Сетка полей для набора фразы.
 *
 * Поля скрытые: подглядеть из-за плеча нечего, а опечатку ловит не глаз, а
 * проверка по списку и контрольной сумме. Глазом посмотреть тоже можно —
 * кнопка рядом.
 */
function wordGrid(count = 24, { isStandard = () => true } = {}) {
  const inputs = [];
  const chips = h('div', { class: 'tagrow', style: 'min-height:26px;margin-top:8px' });
  let masked = true;
  let suggestTimer = null;

  const cell = (i) => {
    const input = h('input', {
      class: 'input mono seed-input', type: 'password', autocomplete: 'off',
      spellcheck: false,
      onFocus: () => suggest(i),
      onInput: () => {
        input.classList.remove('is-bad');
        clearTimeout(suggestTimer);
        suggestTimer = setTimeout(() => suggest(i), 90);
      },
      onBlur: () => check(i),
      onKeyDown: (e) => {
        // Пробел — это «слово набрано», а не символ: между полями фразы
        // пробелу взяться неоткуда.
        if (e.key === ' ') { e.preventDefault(); focusAt(i + 1); }
        if (e.key === 'Backspace' && !input.value && i > 0) { e.preventDefault(); focusAt(i - 1); }
      },
      onPaste: (e) => onPaste(e, i),
    });
    inputs.push(input);
    return h('div', { class: 'seed-cell' },
      h('span', { class: 'seed-num' }, i + 1), input);
  };

  const grid = h('div', { class: 'seed-grid is-input' });
  for (let i = 0; i < count; i += 1) mount(grid, cell(i));

  function focusAt(i) {
    inputs[Math.max(0, Math.min(i, inputs.length - 1))]?.focus();
  }

  async function suggest(i) {
    const prefix = inputs[i].value.trim();
    clear(chips);
    if (!prefix) return;
    const list = await bip39Suggest(prefix, 6).catch(() => []);
    if (list.length === 1 && list[0] === prefix.toLowerCase()) return;
    for (const w of list) {
      mount(chips, h('button', {
        class: 'tag tag-neutral is-button', type: 'button',
        // mousedown, а не click: click приходит после blur, а поле к тому
        // времени уже потеряло бы фокус и подсказка исчезла.
        onMouseDown: (e) => {
          e.preventDefault();
          inputs[i].value = w;
          inputs[i].classList.remove('is-bad');
          clear(chips);
          focusAt(i + 1);
        },
      }, w));
    }
  }

  async function check(i) {
    const value = inputs[i].value.trim().toLowerCase();
    // У кошельков со своим словарём (Electrum, Monero) слов из списка BIP39
    // не будет вовсе — красить их красным незачем.
    if (!value || !isStandard()) { inputs[i].classList.remove('is-bad'); return; }
    const list = await bip39Suggest(value, 1).catch(() => []);
    inputs[i].classList.toggle('is-bad', list[0] !== value);
  }

  /**
   * Вставка фразы целиком. Набирать двадцать четыре слова руками — верный
   * способ ошибиться, поэтому вставка разрешена; сразу после неё предлагается
   * очистить буфер обмена, чтобы фраза не осталась там лежать.
   */
  function onPaste(e, i) {
    const text = (e.clipboardData || window.clipboardData)?.getData('text') || '';
    const parts = text.trim().split(/\s+/).filter(Boolean);
    if (parts.length < 2) return;
    e.preventDefault();
    parts.forEach((w, k) => {
      const slot = inputs[i + k];
      if (slot) { slot.value = w.toLowerCase(); slot.classList.remove('is-bad'); }
    });
    focusAt(i + parts.length);
    inputs.forEach((_, k) => check(k));
    seedClearClipboard()
      .then(() => toast('Фраза вставлена, буфер обмена очищен', { glyph: 'eraser' }))
      .catch(() => toast('Фраза вставлена. Очистите буфер обмена вручную.', { error: true }));
  }

  const eye = h('button', {
    class: 'btn btn-ghost', style: 'font-size:11.5px', type: 'button',
    onClick: () => {
      masked = !masked;
      for (const input of inputs) input.type = masked ? 'password' : 'text';
      eye.replaceChildren(icon(masked ? 'eye' : 'eye-slash', { size: 14 }),
        document.createTextNode(masked ? ' Показать ввод' : ' Скрыть ввод'));
    },
  }, icon('eye', { size: 14 }), ' Показать ввод');

  return {
    node: h('div', {}, grid, chips,
      h('div', { style: 'display:flex;align-items:center;margin-top:4px' }, eye)),
    values: () => inputs.map((input) => input.value.trim().toLowerCase()),
    focus: () => focusAt(0),
    wipe: () => { for (const input of inputs) input.value = ''; },
    recheck: () => inputs.forEach((_, k) => check(k)),
  };
}

/** Переключатель числа слов. */
function lengthPicker(current, onPick) {
  const row = h('div', { class: 'tagrow' });
  const paintRow = (value) => {
    clear(row);
    for (const n of LENGTHS) {
      mount(row, h('button', {
        class: `tag is-button ${n === value ? 'tag-outline' : 'tag-neutral'}`,
        type: 'button',
        onClick: () => { paintRow(n); onPick(n); },
      }, `${n} слов`));
    }
  };
  paintRow(current);
  return row;
}

/** Общая часть форм «добавить» и «заменить»: сетка, длина, свой словарь. */
function phraseForm(initialCount = 24) {
  let count = initialCount;
  let standard = true;
  let grid = wordGrid(count, { isStandard: () => standard });

  const holder = h('div', { style: 'margin-top:12px' }, grid.node);
  const passphrase = h('input', {
    class: 'input mono', type: 'password', autocomplete: 'off',
    placeholder: 'если у кошелька его нет — оставьте пустым',
  });

  const standardToggle = h('label', {
    class: 'radio check', style: 'display:flex;align-items:center;gap:8px;font-size:12px',
  },
    h('input', {
      type: 'checkbox', checked: true,
      onChange: (e) => { standard = e.target.checked; grid.recheck(); },
    }),
    h('span', { class: 'dot' }),
    h('span', {}, 'Фраза BIP39 — проверять по списку и контрольной сумме'));

  const picker = lengthPicker(count, (n) => {
    count = n;
    const fresh = wordGrid(count, { isStandard: () => standard });
    clear(holder);
    mount(holder, fresh.node);
    grid = fresh;
    fresh.focus();
  });

  return {
    node: h('div', {},
      h('div', { class: 'field' }, h('label', {}, 'Длина фразы'), picker),
      holder,
      h('div', { class: 'field', style: 'margin-top:12px' },
        h('label', {}, 'Дополнительное слово (25-е)'), passphrase),
      h('div', { style: 'margin-top:12px' }, standardToggle)),
    read: () => ({
      words: grid.values(),
      passphrase: passphrase.value,
      standard,
    }),
    wipe: () => { grid.wipe(); passphrase.value = ''; },
    focus: () => grid.focus(),
    /** Проверка до отправки: та же, что в ядре, но без лишнего похода туда. */
    validate: async () => {
      const words = grid.values();
      if (words.some((w) => !w)) return 'Заполните все слова фразы.';
      if (!standard) return null;
      try {
        await bip39Check(words);
        return null;
      } catch (e) {
        return e.message;
      }
    },
  };
}

function addDialog() {
  const dlg = openDialog({ title: 'Новая сид-фраза', width: 620 });
  const title = h('input', { class: 'input', placeholder: 'Ledger основной', autocomplete: 'off' });
  const wallet = h('input', { class: 'input', placeholder: 'Ledger Nano S', autocomplete: 'off' });
  const network = h('input', { class: 'input', placeholder: 'BTC', autocomplete: 'off' });
  const derivation = h('input', { class: 'input mono', placeholder: "m/44'/0'/0'", autocomplete: 'off' });
  const note = h('textarea', { class: 'input', rows: 2, placeholder: 'где лежит бумажная копия' });
  const form = phraseForm(24);
  const err = errorLine();

  const save = async () => {
    if (!title.value.trim()) { err.textContent = 'Придумайте название.'; return; }
    const problem = await form.validate();
    if (problem) { err.textContent = problem; return; }

    btn.disabled = true;
    try {
      const { words, passphrase, standard } = form.read();
      await seedAdd({
        title: title.value.trim(),
        wallet: wallet.value.trim(),
        network: network.value.trim(),
        derivation: derivation.value.trim(),
        note: note.value,
        words, passphrase, standard,
      });
      form.wipe();
      dlg.close();
      await reload();
      toast('Фраза сохранена', { glyph: 'seal-check' });
    } catch (e) {
      err.textContent = e.message;
      btn.disabled = false;
    }
  };
  const btn = h('button', { class: 'btn btn-primary', type: 'button', onClick: save }, 'Сохранить');

  dlg.whenClosed(() => form.wipe());

  mount(dlg.body,
    h('div', { class: 'field' }, h('label', {}, 'Название'), title),
    h('div', { style: 'display:flex;gap:10px;margin-top:10px' },
      h('div', { class: 'field', style: 'flex:1' }, h('label', {}, 'Кошелёк'), wallet),
      h('div', { class: 'field', style: 'width:110px' }, h('label', {}, 'Сеть'), network)),
    h('div', { class: 'field', style: 'margin-top:10px' },
      h('label', {}, 'Путь выведения'), derivation),
    form.node,
    h('div', { class: 'field', style: 'margin-top:12px' }, h('label', {}, 'Заметка'), note),
    err,
    actions(
      h('button', { class: 'btn btn-secondary', type: 'button', onClick: dlg.close }, 'Отмена'),
      btn));

  setTimeout(() => title.focus(), 0);
}

function replaceDialog(entry) {
  const dlg = openDialog({ title: `Заменить фразу — ${entry.title}`, width: 620 });
  const form = phraseForm(entry.word_count || 24);
  const err = errorLine();

  const save = async () => {
    const problem = await form.validate();
    if (problem) { err.textContent = problem; return; }
    if (!confirm('Заменить сохранённую фразу?\n\nПрежняя нигде не останется: истории у сид-фраз нет.')) return;

    btn.disabled = true;
    try {
      const { words, passphrase, standard } = form.read();
      await seedReplacePhrase(entry.id, words, passphrase, standard);
      form.wipe();
      dlg.close();
      await reload();
      toast('Фраза заменена', { glyph: 'seal-check' });
    } catch (e) {
      err.textContent = e.message;
      btn.disabled = false;
    }
  };
  const btn = h('button', { class: 'btn btn-primary', type: 'button', onClick: save }, 'Заменить');

  dlg.whenClosed(() => form.wipe());

  mount(dlg.body,
    h('div', { class: 'dialog-body', style: 'font-size:12.5px' },
      'Наберите новую фразу целиком. Прежняя будет затёрта — прошлых значений ' +
      'у сид-фраз не хранится намеренно.'),
    form.node,
    err,
    actions(
      h('button', { class: 'btn btn-secondary', type: 'button', onClick: dlg.close }, 'Отмена'),
      btn));
  setTimeout(() => form.focus(), 0);
}

function detailsDialog(entry) {
  const dlg = openDialog({ title: 'Сведения о записи' });
  const title = h('input', { class: 'input', value: entry.title, autocomplete: 'off' });
  const wallet = h('input', { class: 'input', value: entry.wallet, autocomplete: 'off' });
  const network = h('input', { class: 'input', value: entry.network, autocomplete: 'off' });
  const derivation = h('input', { class: 'input mono', value: entry.derivation, autocomplete: 'off' });
  const note = h('textarea', { class: 'input', rows: 3, value: entry.note });
  const err = errorLine();

  const save = async () => {
    if (!title.value.trim()) { err.textContent = 'Название не может быть пустым.'; return; }
    try {
      await seedUpdateDetails(entry.id, {
        title: title.value.trim(),
        wallet: wallet.value.trim(),
        network: network.value.trim(),
        derivation: derivation.value.trim(),
        note: note.value,
      });
      dlg.close();
      await reload();
    } catch (e) {
      err.textContent = e.message;
    }
  };

  mount(dlg.body,
    h('div', { class: 'dialog-body', style: 'font-size:12px' },
      'Сама фраза здесь не участвует — для неё есть «Заменить фразу».'),
    h('div', { class: 'field', style: 'margin-top:12px' }, h('label', {}, 'Название'), title),
    h('div', { style: 'display:flex;gap:10px;margin-top:10px' },
      h('div', { class: 'field', style: 'flex:1' }, h('label', {}, 'Кошелёк'), wallet),
      h('div', { class: 'field', style: 'width:110px' }, h('label', {}, 'Сеть'), network)),
    h('div', { class: 'field', style: 'margin-top:10px' }, h('label', {}, 'Путь выведения'), derivation),
    h('div', { class: 'field', style: 'margin-top:10px' }, h('label', {}, 'Заметка'), note),
    err,
    actions(
      h('button', { class: 'btn btn-secondary', type: 'button', onClick: dlg.close }, 'Отмена'),
      h('button', { class: 'btn btn-primary', type: 'button', onClick: save }, 'Сохранить')));
}

/**
 * Самопроверка: пользователь набирает фразу с бумажки, приложение отвечает
 * «совпадает» или «нет». Ничего не показывается — это единственный способ
 * сверить бумажную копию, не выводя фразу на экран.
 */
function verifyDialog(entry) {
  const dlg = openDialog({ title: `Проверить запись — ${entry.title}`, width: 620 });
  const grid = wordGrid(entry.word_count || 24);
  const verdict = h('div', { style: 'font-size:12.5px;min-height:20px;margin-top:8px' });

  const run = async () => {
    const words = grid.values();
    if (words.some((w) => !w)) {
      verdict.textContent = 'Заполните все слова.';
      verdict.style.color = '#e0808f';
      return;
    }
    const same = await guard(() => seedVerifyPhrase(entry.id, words));
    if (same === undefined) return;
    verdict.textContent = same
      ? 'Совпадает — бумажная копия верна.'
      : 'Не совпадает. Проверьте порядок слов и написание.';
    verdict.style.color = same ? 'var(--color-accent)' : '#e0808f';
  };

  dlg.whenClosed(() => grid.wipe());

  mount(dlg.body,
    h('div', { class: 'dialog-body', style: 'font-size:12.5px' },
      'Наберите фразу с бумажной копии. Приложение сравнит её с сохранённой и ' +
      'ответит «да» или «нет», не показывая ни одного слова.'),
    h('div', { style: 'margin-top:12px' }, grid.node),
    verdict,
    actions(
      h('button', { class: 'btn btn-secondary', type: 'button', onClick: dlg.close }, 'Закрыть'),
      h('button', { class: 'btn btn-primary', type: 'button', onClick: run }, 'Сверить')));
  setTimeout(() => grid.focus(), 0);
}

/** Удаление — сразу и навсегда, поэтому подтверждается набранным названием. */
function deleteDialog(entry) {
  const dlg = openDialog({ title: 'Удалить запись' });
  const field = h('input', { class: 'input', autocomplete: 'off', placeholder: entry.title });
  const err = errorLine();

  const go = async () => {
    try {
      await seedDelete(entry.id, field.value);
      dlg.close();
      state.selectedId = null;
      await reload();
      toast('Запись удалена', { glyph: 'trash' });
    } catch (e) {
      err.textContent = e.message;
    }
  };

  mount(dlg.body,
    h('div', { class: 'dialog-body', style: 'font-size:12.5px' },
      'Запись исчезнет сразу и навсегда: корзины у сид-фраз нет. ' +
      'Если бумажной копии не осталось, кошелёк будет потерян. ' +
      'Прежняя версия файла останется в резервных копиях рядом с ним.'),
    h('div', { class: 'field', style: 'margin-top:14px' },
      h('label', {}, `Наберите название записи — «${entry.title}»`), field),
    err,
    actions(
      h('button', { class: 'btn btn-secondary', type: 'button', onClick: dlg.close }, 'Отмена'),
      h('button', { class: 'btn btn-primary', type: 'button', onClick: go }, 'Удалить')));
  setTimeout(() => field.focus(), 0);
}

/** Смена мастер-пароля хранилища — вызывается с экрана настроек. */
export function seedChangePasswordDialog() {
  const dlg = openDialog({ title: 'Мастер-пароль хранилища сид-фраз' });
  const current = passwordInput({});
  const next = passwordInput({});
  const again = passwordInput({});
  const err = errorLine();

  const go = async () => {
    if (next.value !== again.value) { err.textContent = 'Новые пароли не совпадают.'; return; }
    try {
      await seedChangePassword(current.value, next.value);
      current.value = ''; next.value = ''; again.value = '';
      dlg.close();
      toast('Мастер-пароль изменён', { glyph: 'key' });
    } catch (e) {
      err.textContent = e.message;
    }
  };

  mount(dlg.body,
    h('div', { class: 'dialog-body', style: 'font-size:12.5px' },
      'Файл при этом не перешифровывается целиком: меняется только обёртка ключа.'),
    h('div', { class: 'field', style: 'margin-top:12px' }, h('label', {}, 'Текущий'), current),
    h('div', { class: 'field', style: 'margin-top:10px' }, h('label', {}, 'Новый'), next),
    h('div', { class: 'field', style: 'margin-top:10px' }, h('label', {}, 'Повторите'), again),
    err,
    actions(
      h('button', { class: 'btn btn-secondary', type: 'button', onClick: dlg.close }, 'Отмена'),
      h('button', { class: 'btn btn-primary', type: 'button', onClick: go }, 'Изменить')));
  setTimeout(() => current.focus(), 0);
}

// ═══ события ═══════════════════════════════════════════════════════════════

// Файл раздела можно выбрать и из окна настроек — тогда главное окно узнаёт
// об этом отсюда.
listen('seed-changed', () => { if (box) reload(); });

listen('seed-locked', (e) => {
  // Все открытые окна показа закрываются: на экране не должно остаться
  // ни одного слова от закрытого хранилища.
  for (const node of document.querySelectorAll('.dialog-backdrop.is-seed')) node.remove();
  if (!box) return;
  reload();
  if (e.payload === 'autolock') {
    setTimeout(() => toast('Раздел сид-фраз закрыт из-за бездействия', { glyph: 'lock-key' }), 200);
  }
});
