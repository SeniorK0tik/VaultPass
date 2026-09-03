// Экраны 1a и 1b — быстрое окно поверх любых приложений.
//
// Один и тот же экран в двух видах: «палитра» (1a) — только список, «палитра
// с полями» (1b) — плюс панель полей выбранной записи справа. Что показывать,
// решает настройка `quick_view`; 1b — надмножество 1a, поэтому логика общая.
//
// Главное действие — Enter: копирует пароль и закрывает окно. Всё остальное
// подчинено тому, чтобы до этого Enter дойти как можно быстрее.

import {
  call, listen, currentWindow, search, recent, getSettings, quickCopy,
  copyField, openEntryUrl, autofillEntry, openEditor, status,
} from './api.js';
import { h, icon, $, clear, entryIcon, totpRing, mount } from './ui.js';
import { toast, toastCopied, guard, trackActivity } from './chrome.js';

let settings = null;
let items = [];
let selected = 0;
let query = '';
let searchTimer = null;
const refs = {};

const withFields = () => settings?.quick_view === 'fields';

// ── разметка ───────────────────────────────────────────────────────────────

function build() {
  refs.input = h('input', {
    class: 'input input-bare',
    style: 'font-size:17px;letter-spacing:-.01em',
    placeholder: 'Поиск по записям',
    autocomplete: 'off', spellcheck: false,
    onInput: (e) => { query = e.target.value; scheduleSearch(); },
  });

  const header = h('div', {
    style: 'display:flex;align-items:center;gap:11px;padding:15px 17px;flex:none;' +
           'border-bottom:1px solid var(--color-divider)',
    'data-tauri-drag-region': '',
  },
    icon('magnifying-glass', { size: 19, color: 'var(--color-neutral-500)' }),
    h('div', { style: 'flex:1;min-width:0' }, refs.input),
    h('span', { class: 'key' }, 'esc'));

  refs.list = h('div', { class: 'scroll', style: 'flex:1;padding:8px 0 4px' });
  refs.fields = h('div', {
    class: 'col',
    style: 'flex:1;min-width:0;border-left:1px solid var(--color-divider)',
  });

  refs.hints = h('div', {
    style: 'display:flex;align-items:center;gap:16px;padding:10px 17px;flex:none;' +
           'border-top:1px solid var(--color-divider);font-size:11px;color:var(--color-neutral-500)',
  });

  const body = withFields()
    ? h('div', { class: 'screen-body' },
        h('div', {
          class: 'col',
          style: 'width:290px;flex:none;border-right:1px solid var(--color-divider)',
        }, refs.list),
        refs.fields)
    : h('div', { class: 'screen-body' }, h('div', { class: 'col grow' }, refs.list));

  mount(clear($('#app')), header, body, refs.hints);
  drawHints();
}

function drawHints() {
  clear(refs.hints);
  if (!settings?.show_key_hints) return;
  mount(refs.hints, 
    h('span', {}, '↑↓ выбор'),
    withFields() ? h('span', {}, '⇥ поля записи') : null,
    h('span', {}, 'Ctrl+⏎ открыть сайт'),
    h('span', {}, 'Ctrl+B автозаполнение'),
    withFields() ? null
      : h('span', { style: 'margin-left:auto;color:var(--color-neutral-600)' }, prettyHotkey()));
}

function prettyHotkey() {
  return (settings?.hotkey || '')
    .replace(/CmdOrCtrl/gi, 'Ctrl').split('+').map((s) => s.trim()).join(' + ');
}

// ── список ─────────────────────────────────────────────────────────────────

function drawList() {
  clear(refs.list);

  if (items.length === 0) {
    mount(refs.list, h('div', { class: 'empty', style: 'padding:28px 20px' },
      icon('magnifying-glass'),
      h('div', { class: 'empty-title' }, query ? 'Ничего не нашлось' : 'Сейф пуст'),
      h('div', { class: 'empty-hint' },
        query ? 'Проверьте написание или создайте запись.' : 'Создайте первую запись в главном окне.')));
  } else {
    mount(refs.list, h('div', { class: 'kicker', style: 'padding:4px 17px 6px' },
      query ? 'Совпадения' : 'Недавние'));

    items.forEach((entry, i) => {
      const chosen = i === selected;
      mount(refs.list, h('button', {
        class: 'row row-lg', type: 'button', 'aria-selected': chosen ? 'true' : 'false',
        onClick: () => { selected = i; drawList(); drawFields(); },
        onDblClick: () => activate(),
      },
        entryIcon(entry, { size: 31, accent: chosen }),
        h('div', { class: 'row-text' },
          h('div', { class: 'row-title' }, entry.title),
          h('div', { class: 'row-sub' }, entry.subtitle || entry.host || entry.kind_title)),
        chosen && entry.has_password
          ? h('span', { style: 'font-size:11px;color:var(--color-accent);white-space:nowrap' }, '⏎ копировать пароль')
          : h('span', { class: 'tag tag-neutral' }, shortKind(entry.kind))));
    });
  }

  // Действие «создать запись» — как в макете, отдельным разделом внизу.
  if (query.trim()) {
    mount(refs.list, 
      h('div', { class: 'rule', style: 'margin:8px 0' }),
      h('div', { class: 'kicker', style: 'padding:2px 17px 6px' }, 'Действия'),
      h('button', {
        class: 'row row-lg', type: 'button',
        onClick: () => guard(async () => {
          await openEditor(null);
          await currentWindow().hide();
        }),
      },
        h('div', {
          class: 'row-icon',
          style: 'background:transparent;border:1px solid var(--color-divider);color:var(--color-neutral-400)',
        }, icon('plus', { size: 15 })),
        h('div', { class: 'row-text' },
          h('div', { class: 'row-title' }, `Создать запись «${query.trim()}»`)),
        h('span', { class: 'key' }, 'Ctrl+N')));
  }

  refs.list.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
}

function shortKind(kind) {
  return { password: 'пароль', note: 'заметка', api_key: 'ключ', document: 'документ' }[kind] || '';
}

// ── панель полей (только вид 1b) ───────────────────────────────────────────

function drawFields() {
  if (!withFields()) return;
  clear(refs.fields);

  const entry = items[selected];
  if (!entry) {
    mount(refs.fields, h('div', { class: 'empty' },
      icon('cursor-click'),
      h('div', { class: 'empty-title' }, 'Выберите запись')));
    return;
  }

  const head = h('div', { style: 'padding:14px 16px 10px;flex:none' },
    h('div', { style: 'display:flex;align-items:center;gap:10px' },
      entryIcon(entry, { size: 34, accent: true }),
      h('div', { style: 'min-width:0' },
        h('div', { style: 'font-size:16px;font-weight:500' }, entry.title),
        h('div', { style: 'font-size:11.5px;color:var(--color-neutral-500)' },
          `${entry.kind_title} · обновлён ${relative(entry.modified_at)}`))));

  const list = h('div', {
    class: 'scroll',
    style: 'flex:1;padding:0 16px;display:flex;flex-direction:column;gap:7px',
  });

  if (entry.username) {
    mount(list, fieldBox('Логин', entry.username, {
      hint: 'Shift+⏎',
      onCopy: () => copy(entry.id, 'username', 'Логин'),
    }));
  }
  if (entry.has_password) {
    mount(list, fieldBox('Пароль', '••••••••••••••••', {
      accent: true, hint: '⏎',
      onCopy: () => copy(entry.id, 'password', 'Пароль'),
    }));
  }
  if (entry.has_totp) {
    // Секрет TOTP хранится, но коды в этой версии не считаются —
    // показывать выдуманные шесть цифр было бы враньём.
    mount(list, h('div', { class: 'fieldbox' },
      h('div', { class: 'fb-text' },
        h('div', { class: 'fb-label' }, 'Код 2FA'),
        h('div', { class: 'fb-value dim', style: 'font-size:12.5px;font-family:inherit' },
          'секрет сохранён, коды появятся позже')),
      totpRing(22)));
  }
  if (entry.note) {
    mount(list, h('div', { class: 'fieldbox' },
      h('div', { class: 'fb-text' },
        h('div', { class: 'fb-label' }, 'Заметка'),
        h('div', { class: 'fb-value is-wrap selectable', style: 'font-family:inherit;font-size:13px' }, entry.note))));
  }

  const foot = h('div', {
    style: 'display:flex;align-items:center;gap:8px;padding:10px 16px;flex:none;' +
           'border-top:1px solid var(--color-divider);font-size:11px;color:var(--color-neutral-500)',
  },
    refs.status = h('span', { style: 'display:flex;align-items:center;gap:8px' },
      icon('keyboard', { size: 14, color: 'var(--color-neutral-600)' }),
      h('span', {}, 'Enter — копировать пароль')),
    h('span', { style: 'margin-left:auto;color:var(--color-neutral-600)' }, prettyHotkey()));

  mount(refs.fields, head, list, foot);
}

function fieldBox(label, value, { accent = false, hint, onCopy } = {}) {
  return h('div', { class: `fieldbox${accent ? ' is-accent' : ''}`, style: 'padding:8px 10px' },
    h('div', { class: 'fb-text' },
      h('div', { class: 'fb-label' }, label),
      h('div', { class: 'fb-value', style: 'font-size:13.5px' }, value)),
    hint && h('span', {
      style: `font-size:11px;color:var(--color-${accent ? 'accent' : 'neutral-600'})`,
    }, hint),
    h('button', {
      class: 'btn btn-icon btn-ghost', style: 'width:28px;height:28px', type: 'button',
      title: 'Копировать', onClick: onCopy,
    }, icon('copy', { size: 15 })));
}

function relative(iso) {
  const days = Math.floor((Date.now() - new Date(iso)) / 86_400_000);
  if (days <= 0) return 'сегодня';
  if (days === 1) return 'вчера';
  return `${days} дн. назад`;
}

// ── действия ───────────────────────────────────────────────────────────────

async function copy(id, field, label) {
  const secs = await guard(() => copyField(id, field));
  if (secs === undefined) return;
  if (refs.status) {
    mount(clear(refs.status), 
      icon('clock-countdown', { size: 14, color: 'var(--color-accent)' }),
      h('span', {}, secs ? `Скопировано · буфер очистится через 0:${String(secs).padStart(2, '0')}`
                         : 'Скопировано'));
  } else {
    toastCopied(label, secs);
  }
}

/** Enter: скопировать пароль и закрыть окно — то, ради чего окно и открывают. */
async function activate() {
  const entry = items[selected];
  if (!entry) return;
  if (!entry.has_password) {
    // У заметки и документа пароля нет — открываем карточку целиком.
    await guard(async () => { await openEditor(entry.id); await currentWindow().hide(); });
    return;
  }
  const secs = await guard(() => quickCopy(entry.id));
  if (secs !== undefined) toastCopied('Пароль', secs);
}

// ── поиск ──────────────────────────────────────────────────────────────────

/** Небольшая задержка гасит дребезг при быстром наборе, но остаётся
    незаметной: 90 мс короче паузы между нажатиями. */
function scheduleSearch() {
  clearTimeout(searchTimer);
  searchTimer = setTimeout(load, 90);
}

async function load() {
  const q = query.trim();
  const list = await guard(
    () => (q ? search(q, 8) : recent(6)),
    { silent: true });

  items = list || [];
  selected = 0;
  drawList();
  drawFields();
}

// ── клавиатура ─────────────────────────────────────────────────────────────

window.addEventListener('keydown', async (e) => {
  const entry = items[selected];

  switch (e.key) {
    case 'Escape':
      e.preventDefault();
      await currentWindow().hide();
      return;

    case 'ArrowDown':
      e.preventDefault();
      selected = Math.min(selected + 1, Math.max(items.length - 1, 0));
      drawList(); drawFields();
      return;

    case 'ArrowUp':
      e.preventDefault();
      selected = Math.max(selected - 1, 0);
      drawList(); drawFields();
      return;

    case 'Enter':
      e.preventDefault();
      if (!entry) return;
      if (e.ctrlKey || e.metaKey) {
        await guard(() => openEntryUrl(entry.id));
      } else if (e.shiftKey) {
        await copy(entry.id, 'username', 'Логин');
      } else {
        await activate();
      }
      return;

    case 'Tab':
      // ⇥ переводит фокус на панель полей — в виде 1a панели нет,
      // поэтому клавиша просто ничего не делает.
      if (withFields() && entry) {
        e.preventDefault();
        refs.fields.querySelector('button')?.focus();
      }
      return;
  }

  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'n') {
    e.preventDefault();
    await guard(async () => { await openEditor(null); await currentWindow().hide(); });
  }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'b' && entry) {
    e.preventDefault();
    await guard(() => autofillEntry(entry.id));
  }
});

// ── запуск ─────────────────────────────────────────────────────────────────

async function boot() {
  settings = await getSettings();
  build();

  const st = await status();
  if (!st.unlocked) {
    // Сейф закрыт: показывать пустую палитру бессмысленно.
    await currentWindow().hide();
    await call('open_window', { label: 'main' });
    return;
  }
  await load();
  refs.input.focus();
}

// Окно не создаётся заново на каждый вызов, поэтому состояние сбрасывается
// по событию открытия.
listen('quick-opened', async () => {
  settings = await getSettings();
  build();
  query = '';
  refs.input.value = '';
  await load();
  refs.input.focus();
});

listen('settings-changed', async (e) => {
  settings = e.payload;
  build();
  drawList();
  drawFields();
});

listen('vault-locked', () => currentWindow().hide());

trackActivity();
boot();
