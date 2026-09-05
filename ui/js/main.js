// Главное окно: экран разблокировки (1f) и два вида списка — три панели (1d)
// и плотная таблица (1e). Какой из двух показывать, выбирается в настройках.

import {
  call, listen, listEntries, counts, folders, getEntry, getSettings, auditReport,
  copyField, revealField, openEntryUrl, autofillEntry, openEditor, openWindow,
  toggleFavorite, deleteEntry, restoreEntry, purgeEntry, emptyTrash, lockCountdown, status,
} from './api.js';
import {
  h, icon, $, clear, entryIcon, meter, totpRing,
  fmtDate, fmtDateShort, fmtAgo, fmtDays, fmtCountdown, plural, mount } from './ui.js';
import { titlebar, toast, toastCopied, guard, trackActivity } from './chrome.js';
import { renderUnlock } from './unlock.js';

const KINDS = [
  { kind: 'password', label: 'Пароли',     glyph: 'key',                 key: 'passwords' },
  { kind: 'note',     label: 'Заметки',    glyph: 'note',                key: 'notes' },
  { kind: 'api_key',  label: 'Ключи API',  glyph: 'code',                key: 'api_keys' },
  { kind: 'document', label: 'Документы',  glyph: 'identification-card', key: 'documents' },
];

const state = {
  settings: null,
  scope: { type: 'all' },
  query: '',
  sort: 'modified',
  entries: [],
  counts: null,
  folders: [],
  selectedId: null,
  detail: null,
  revealed: false,   // показан ли пароль выбранной записи
  audit: null,       // непусто — открыт раздел «Аудит»
};

const refs = {};
let searchTimer = null;
let countdownTimer = null;

// ═══ каркас окна ═══════════════════════════════════════════════════════════

async function boot() {
  state.settings = await getSettings();
  const st = await status();

  const root = $('#app');
  mount(clear(root), titlebar('Сейф'));
  refs.stage = h('div', { class: 'col grow' });
  mount(root, refs.stage);

  if (!st.unlocked) {
    await renderUnlock(refs.stage, () => boot());
    return;
  }
  await loadAll();
  renderApp();
  startCountdown();
}

async function updateLockHint() {
  if (!refs.lockHint) return;
  const left = await guard(() => lockCountdown(), { silent: true });
  // Места в панели немного: «через 8 мин» помещается, а полная фраза
  // обрезалась бы многоточием. Целиком она остаётся во всплывающей подсказке.
  refs.lockHint.textContent = left === null || left === undefined
    ? 'без автоблокировки'
    : `через ${fmtCountdown(left)}`;
  refs.lockHint.title = left === null || left === undefined
    ? 'Автоблокировка отключена'
    : `Автоматическая блокировка через ${fmtCountdown(left)}`;
}

function startCountdown() {
  clearInterval(countdownTimer);
  // Раз в 15 секунд: подпись всё равно округляется до минут, чаще незачем.
  countdownTimer = setInterval(updateLockHint, 15_000);
}

async function loadAll() {
  const [list, c, f] = await Promise.all([
    guard(() => listEntries({ scope: state.scope, query: state.query, sort: state.sort }), { silent: true }),
    guard(() => counts(), { silent: true }),
    guard(() => folders(), { silent: true }),
  ]);
  state.entries = list || [];
  state.counts = c || null;
  state.folders = f || [];

  if (!state.entries.some((e) => e.id === state.selectedId)) {
    state.selectedId = state.entries[0]?.id ?? null;
  }
  await loadDetail();
}

async function loadDetail() {
  state.revealed = false;
  state.detail = state.selectedId
    ? await guard(() => getEntry(state.selectedId), { silent: true })
    : null;
}

async function refresh() {
  await loadAll();
  renderApp();
}

// ═══ выбор вида ════════════════════════════════════════════════════════════

function renderApp() {
  if (state.audit) return renderAudit();
  if (state.settings.main_view === 'table') return renderTable();
  return renderPanels();
}

// ═══ левая панель (общая для 1d и раздела «Аудит») ═════════════════════════

function sidebar() {
  const c = state.counts;
  const item = (glyph, label, count, scope, extra) => h('button', {
    class: 'side-item', type: 'button',
    'aria-current': !state.audit && sameScope(scope, state.scope) ? 'true' : null,
    onClick: () => pick(scope),
  }, icon(glyph), h('span', {}, label),
     extra || (count !== null && count !== undefined
       ? h('span', { class: 'count' }, count) : null));

  refs.lockHint = h('div', {
    style: 'font-size:10.5px;color:var(--color-neutral-600);white-space:nowrap;' +
           'overflow:hidden;text-overflow:ellipsis',
  }, '');

  return h('div', {
    class: 'col',
    style: 'width:222px;flex:none;border-right:1px solid var(--color-divider);padding:14px 12px',
  },
    h('div', { style: 'display:flex;align-items:center;gap:8px;padding:0 4px 14px' },
      icon('vault', { fill: true, size: 19, color: 'var(--color-accent)' }),
      h('span', { style: 'font-size:16px;font-weight:500' }, 'Сейф')),

    searchButton(),

    h('div', { class: 'scroll', style: 'flex:1;display:flex;flex-direction:column;gap:1px' },
      item('squares-four', 'Все записи', c?.total, { type: 'all' }),
      KINDS.map((k) => item(k.glyph, k.label, c?.[k.key], { type: 'kind', kind: k.kind })),

      h('div', { class: 'rule', style: 'margin:14px 0' }),

      item('star', 'Избранное', c?.favorites || null, { type: 'favorites' }),
      h('button', {
        class: 'side-item', type: 'button', 'aria-current': state.audit ? 'true' : null,
        onClick: showAudit,
      }, icon('shield-warning'), h('span', {}, 'Аудит'), refs.auditCount = h('span', { class: 'count' })),
      item('trash', 'Корзина', c?.trash || null, { type: 'trash' }),

      state.folders.length ? h('div', { class: 'rule', style: 'margin:14px 0' }) : null,
      state.folders.length ? h('div', { class: 'kicker', style: 'padding:0 9px 6px' }, 'Папки') : null,
      state.folders.map((f) => item('folder', f.name, f.count, { type: 'folder', id: f.id })),
      h('button', {
        class: 'side-item', type: 'button', onClick: newFolder,
        style: 'color:var(--color-neutral-500)',
      }, icon('folder-plus'), h('span', {}, 'Новая папка'))),

    h('div', {
      class: 'spacer',
      style: 'display:flex;align-items:center;gap:8px;padding:9px;border-radius:8px;' +
             'border:1px solid var(--color-divider)',
    },
      icon('lock-simple-open', { fill: true, size: 15, color: 'var(--color-accent)' }),
      h('div', { style: 'flex:1;min-width:0' },
        h('div', { style: 'font-size:11.5px;white-space:nowrap' }, 'Локально · XChaCha20'),
        refs.lockHint),
      h('button', {
        class: 'btn btn-icon btn-ghost', style: 'width:26px;height:26px', type: 'button',
        title: 'Заблокировать', onClick: lockNow,
      }, icon('lock-key', { size: 14 }))));
}

function searchButton() {
  refs.search = h('input', {
    class: 'input input-bare',
    style: 'font-size:12.5px',
    placeholder: 'Поиск', value: state.query, autocomplete: 'off',
    onInput: (e) => {
      state.query = e.target.value;
      clearTimeout(searchTimer);
      searchTimer = setTimeout(refresh, 110);
    },
  });
  return h('div', {
    style: 'display:flex;align-items:center;gap:8px;padding:6px 9px;margin-bottom:14px;flex:none;' +
           'border:1px solid var(--color-divider);border-radius:8px;background:var(--color-surface)',
  },
    icon('magnifying-glass', { size: 14, color: 'var(--color-neutral-600)' }),
    h('div', { style: 'flex:1;min-width:0' }, refs.search),
    h('span', { class: 'key' }, 'Ctrl+K'));
}

const sameScope = (a, b) =>
  a.type === b.type && (a.kind ?? null) === (b.kind ?? null) && (a.id ?? null) === (b.id ?? null);

async function pick(scope) {
  state.scope = scope;
  state.audit = null;
  state.selectedId = null;
  await refresh();
}

// ═══ 1d — три панели ═══════════════════════════════════════════════════════

function renderPanels() {
  mount(clear(refs.stage), h('div', { class: 'screen-body' },
    sidebar(), listColumn(), detailColumn()));
  fillAuditCount();
  updateLockHint();
}

function scopeTitle() {
  switch (state.scope.type) {
    case 'all': return 'Все записи';
    case 'kind': return KINDS.find((k) => k.kind === state.scope.kind).label;
    case 'favorites': return 'Избранное';
    case 'trash': return 'Корзина';
    case 'folder': return state.folders.find((f) => f.id === state.scope.id)?.name || 'Папка';
    default: return 'Записи';
  }
}

function listColumn() {
  const list = h('div', { class: 'scroll grow' });

  if (state.entries.length === 0) {
    mount(list, h('div', { class: 'empty' },
      icon(state.query ? 'magnifying-glass' : 'tray'),
      h('div', { class: 'empty-title' },
        state.query ? 'Ничего не нашлось' : 'Здесь пока пусто'),
      h('div', { class: 'empty-hint' },
        state.query ? 'Проверьте написание запроса.' : 'Нажмите + и создайте первую запись.')));
  } else {
    for (const group of groupEntries(state.entries)) {
      mount(list, h('div', { class: 'kicker', style: 'padding:8px 14px 5px' }, group.label));
      for (const entry of group.items) mount(list, listRow(entry));
    }
  }

  return h('div', {
    class: 'col',
    style: 'width:328px;flex:none;border-right:1px solid var(--color-divider)',
  },
    h('div', { style: 'display:flex;align-items:center;gap:8px;padding:13px 14px 10px;flex:none' },
      h('span', { style: 'font-size:13px;font-weight:500' }, scopeTitle()),
      h('span', { style: 'font-size:11.5px;color:var(--color-neutral-600)' }, state.entries.length),
      h('div', { style: 'margin-left:auto;display:flex;gap:2px' },
        h('button', {
          class: 'btn btn-icon btn-ghost', style: 'width:28px;height:28px', type: 'button',
          title: sortTitle(), onClick: cycleSort,
        }, icon('sort-ascending', { size: 15 })),
        state.scope.type === 'trash'
          ? h('button', {
              class: 'btn btn-icon btn-ghost', style: 'width:28px;height:28px', type: 'button',
              title: 'Очистить корзину',
              onClick: () => guard(async () => {
                if (!confirm('Удалить все записи из корзины без возможности вернуть?')) return;
                await emptyTrash();
                await refresh();
              }),
            }, icon('trash', { size: 15 }))
          : h('button', {
              class: 'btn btn-icon btn-primary', style: 'width:28px;height:28px', type: 'button',
              title: 'Новая запись', onClick: () => openEditor(null),
            }, icon('plus', { size: 15 })))),
    list);
}

/**
 * Группировка списка, как в макете: сначала «Часто используемые», затем по
 * первой букве названия. При активном поиске и в сортировках, отличных от
 * алфавитной, группы только мешали бы — список идёт как есть.
 */
function groupEntries(entries) {
  if (state.query || state.scope.type === 'trash') {
    return [{ label: state.query ? 'Совпадения' : 'В корзине', items: entries }];
  }

  const frequent = entries.filter((e) => e.usage_count > 0)
    .sort((a, b) => b.usage_count - a.usage_count).slice(0, 3);
  const frequentIds = new Set(frequent.map((e) => e.id));
  const rest = entries.filter((e) => !frequentIds.has(e.id));

  const groups = [];
  if (frequent.length) groups.push({ label: 'Часто используемые', items: frequent });

  if (state.sort === 'title') {
    const byLetter = new Map();
    for (const e of rest) {
      const letter = (e.title.trim()[0] || '#').toUpperCase();
      if (!byLetter.has(letter)) byLetter.set(letter, []);
      byLetter.get(letter).push(e);
    }
    for (const [letter, items] of byLetter) groups.push({ label: letter, items });
  } else if (rest.length) {
    groups.push({ label: sortTitle(), items: rest });
  }
  return groups;
}

const SORTS = ['modified', 'title', 'used', 'created'];
const sortTitle = () => ({
  modified: 'По изменению', title: 'По алфавиту',
  used: 'По частоте', created: 'По дате создания',
}[state.sort]);

async function cycleSort() {
  state.sort = SORTS[(SORTS.indexOf(state.sort) + 1) % SORTS.length];
  await refresh();
  toast(sortTitle(), { glyph: 'sort-ascending', ms: 1200 });
}

function listRow(entry) {
  const chosen = entry.id === state.selectedId;
  return h('button', {
    class: `row${entry.deleted ? ' is-dimmed' : ''}`,
    type: 'button', 'aria-selected': chosen ? 'true' : 'false',
    onClick: async () => { state.selectedId = entry.id; await loadDetail(); renderApp(); },
    onDblClick: () => openEditor(entry.id),
  },
    entryIcon(entry, { size: 30, accent: chosen }),
    h('div', { class: 'row-text' },
      h('div', { class: 'row-title' }, entry.title),
      h('div', { class: 'row-sub' }, entry.subtitle || entry.host || entry.kind_title)),
    entry.favorite ? icon('star', { fill: true, size: 12, color: 'var(--color-accent)' }) : null,
    entry.strength && entry.strength.bits < 60
      ? h('span', { class: 'tag tag-accent' }, 'слабый') : null);
}

// ── правая колонка: карточка записи ────────────────────────────────────────

function detailColumn() {
  const entry = state.detail;
  if (!entry) {
    return h('div', { class: 'col grow' },
      h('div', { class: 'empty' },
        icon('vault'),
        h('div', { class: 'empty-title' }, 'Выберите запись'),
        h('div', { class: 'empty-hint' }, 'Слева — список, здесь появятся её поля.')));
  }

  const isTrash = entry.deleted;

  const actions = isTrash
    ? [
        h('button', {
          class: 'btn btn-primary', type: 'button',
          onClick: () => guard(async () => { await restoreEntry(entry.id); await refresh(); }),
        }, icon('arrow-counter-clockwise', { size: 15 }), 'Восстановить'),
        h('button', {
          class: 'btn btn-secondary', type: 'button',
          onClick: () => guard(async () => {
            if (!confirm(`Удалить «${entry.title}» окончательно?`)) return;
            await purgeEntry(entry.id);
            await refresh();
          }),
        }, icon('trash', { size: 15 }), 'Удалить навсегда'),
      ]
    : [
        entry.has_password ? h('button', {
          class: 'btn btn-primary', type: 'button',
          onClick: () => copy(entry.id, 'password', 'Пароль'),
        }, icon('copy', { size: 15 }), 'Копировать пароль') : null,
        entry.url ? h('button', {
          class: 'btn btn-secondary', type: 'button',
          onClick: () => guard(() => openEntryUrl(entry.id)),
        }, icon('arrow-square-out', { size: 15 }), 'Открыть сайт') : null,
        entry.has_password ? h('button', {
          class: 'btn btn-secondary', type: 'button',
          onClick: () => guard(() => autofillEntry(entry.id)),
        }, icon('keyboard', { size: 15 }), 'Автозаполнение') : null,
      ];

  const fields = h('div', {
    style: 'padding:14px 24px 0;display:flex;flex-direction:column;gap:8px',
  });

  if (entry.username) {
    mount(fields, fieldBox('Логин', entry.username, {
      onCopy: () => copy(entry.id, 'username', 'Логин'),
    }));
  }
  if (entry.email) {
    mount(fields, fieldBox('Почта', entry.email, {
      onCopy: () => copy(entry.id, 'email', 'Почта'),
    }));
  }
  if (entry.has_password) {
    mount(fields, fieldBox('Пароль', state.revealed ? state.revealedValue : entry.password_masked, {
      extra: entry.strength
        ? h('span', { style: 'font-size:11px;color:var(--color-neutral-500)' }, entry.strength.label)
        : null,
      onReveal: () => toggleReveal(entry.id),
      revealed: state.revealed,
      onCopy: () => copy(entry.id, 'password', 'Пароль'),
    }));
  }
  if (entry.has_totp) {
    mount(fields, h('div', { class: 'fieldbox' },
      h('div', { class: 'fb-text' },
        h('div', { class: 'fb-label' }, 'Код 2FA'),
        h('div', { class: 'fb-value dim', style: 'font-family:inherit;font-size:13px' },
          'секрет сохранён, вычисление кодов появится в следующей версии')),
      totpRing(24)));
  }
  for (const f of entry.custom) {
    mount(fields, fieldBox(f.label, f.value, {
      onCopy: () => guard(async () => {
        const secs = await call('copy_custom_field', { id: entry.id, fieldId: f.id });
        toastCopied(f.label, secs);
      }),
    }));
  }
  if (entry.note) {
    mount(fields, h('div', {
      style: 'padding:10px 12px;border:1px solid var(--color-divider);border-radius:8px',
    },
      h('div', { class: 'fb-label' }, 'Заметка'),
      h('div', {
        class: 'selectable',
        style: 'font-size:13px;white-space:pre-wrap;color:color-mix(in srgb,var(--color-text) 78%,transparent)',
      }, entry.note)));
  }
  if (entry.expires_at) {
    const left = entry.expires_in_days;
    mount(fields, fieldBox('Срок действия',
      left < 0 ? `истёк ${fmtDays(left)} назад` : `истекает через ${fmtDays(left)}`, {}));
  }

  return h('div', { class: 'col grow' },
    h('div', { class: 'scroll grow' },
      h('div', { style: 'padding:20px 24px 0' },
        h('div', { style: 'display:flex;align-items:flex-start;gap:14px' },
          entryIcon(entry, { size: 48, accent: true }),
          h('div', { style: 'flex:1;min-width:0' },
            h('h3', { style: 'margin:0 0 3px' }, entry.title),
            h('div', {
              style: 'display:flex;align-items:center;gap:7px;font-size:12px;color:var(--color-neutral-500)',
            },
              entry.host ? h('span', {}, entry.host) : null,
              entry.host ? h('span', {}, '·') : null,
              h('span', {}, entry.kind_title))),
          isTrash ? null : h('div', { style: 'display:flex;gap:6px' },
            h('button', {
              class: 'btn btn-secondary btn-icon', type: 'button',
              title: entry.favorite ? 'Убрать из избранного' : 'В избранное',
              onClick: () => guard(async () => { await toggleFavorite(entry.id); await refresh(); }),
            }, icon('star', { fill: entry.favorite, size: 15 })),
            h('button', {
              class: 'btn btn-secondary', type: 'button', onClick: () => openEditor(entry.id),
            }, icon('pencil-simple', { size: 15 }), 'Изменить'),
            h('button', {
              class: 'btn btn-secondary btn-icon', type: 'button', title: 'В корзину',
              onClick: () => guard(async () => { await deleteEntry(entry.id); await refresh(); }),
            }, icon('trash', { size: 16 })))),

        entry.tags.length ? h('div', { class: 'tagrow', style: 'margin-top:12px' },
          entry.tags.map((t) => h('span', { class: 'tag tag-neutral' }, t)),
          entry.has_totp ? h('span', { class: 'tag tag-outline' }, '2FA') : null) : null,

        h('div', { style: 'display:flex;gap:8px;margin:16px 0 4px;flex-wrap:wrap' }, actions)),
      fields,
      h('div', { style: 'height:16px' })),

    h('div', {
      style: 'display:flex;align-items:center;gap:14px;padding:12px 24px;flex:none;' +
             'border-top:1px solid var(--color-divider);font-size:11px;color:var(--color-neutral-600)',
    },
      h('span', {}, `Изменено ${fmtDate(entry.modified_at)}`),
      h('span', {}, '·'),
      h('span', {}, `Создано ${fmtDate(entry.created_at)}`),
      entry.password_age_days !== null && entry.password_age_days !== undefined
        ? [h('span', {}, '·'), h('span', {}, `Пароль обновлён ${fmtAgo(entry.password_modified_at)}`)]
        : null,
      entry.history.length ? h('button', {
        class: 'btn btn-ghost', style: 'margin-left:auto;font-size:11.5px', type: 'button',
        onClick: () => showHistory(entry),
      }, icon('clock-counter-clockwise', { size: 14 }), 'История') : null));
}

function fieldBox(label, value, { extra, onCopy, onReveal, revealed } = {}) {
  return h('div', { class: 'fieldbox' },
    h('div', { class: 'fb-text' },
      h('div', { class: 'fb-label' }, label),
      h('div', { class: `fb-value${revealed ? ' selectable' : ''}` }, value)),
    extra || null,
    onReveal ? h('button', {
      class: 'btn btn-icon btn-ghost', style: 'width:30px;height:30px', type: 'button',
      title: revealed ? 'Скрыть' : 'Показать', onClick: onReveal,
    }, icon(revealed ? 'eye-slash' : 'eye', { size: 16 })) : null,
    onCopy ? h('button', {
      class: 'btn btn-icon btn-ghost', style: 'width:30px;height:30px', type: 'button',
      title: 'Копировать', onClick: onCopy,
    }, icon('copy', { size: 16 })) : null);
}

/** Показ пароля — отдельный запрос к ядру: в списке его значения нет вовсе. */
async function toggleReveal(id) {
  if (state.revealed) {
    state.revealed = false;
    state.revealedValue = null;
  } else {
    const value = await guard(() => revealField(id, 'password'));
    if (value === undefined) return;
    state.revealed = true;
    state.revealedValue = value;
  }
  renderApp();
}

async function copy(id, field, label) {
  const secs = await guard(() => copyField(id, field));
  if (secs !== undefined) toastCopied(label, secs);
}

function showHistory(entry) {
  const box = h('div', {
    class: 'dialog-backdrop',
    onClick: (e) => { if (e.target === box) box.remove(); },
  },
    h('div', { class: 'dialog' },
      h('div', { class: 'dialog-title' }, 'История пароля'),
      h('div', { class: 'dialog-body dim', style: 'font-size:12px' },
        'Прежние значения хранятся в сейфе, но на экран не выводятся — видны только длина и дата замены.'),
      h('div', { style: 'display:flex;flex-direction:column;gap:6px' },
        entry.history.map((item) => h('div', {
          style: 'display:flex;align-items:center;gap:8px;padding:7px 9px;' +
                 'border:1px solid var(--color-divider);border-radius:8px',
        },
          h('span', { class: 'mono', style: 'flex:1;font-size:12px;color:var(--color-neutral-400)' }, item.masked),
          h('span', { style: 'font-size:10.5px;color:var(--color-neutral-600)' }, fmtDate(item.replaced_at))))),
      h('div', { class: 'dialog-actions' },
        h('button', { class: 'btn btn-secondary', type: 'button', onClick: () => box.remove() }, 'Закрыть'))));
  mount(document.body, box);
}

// ═══ 1e — плотная таблица ══════════════════════════════════════════════════

function renderTable() {
  const nav = h('div', { class: 'nav', style: 'padding:12px 20px;gap:20px;flex:none' },
    h('span', { class: 'nav-brand', style: 'display:flex;align-items:center;gap:8px' },
      icon('vault', { fill: true, size: 19, color: 'var(--color-accent)' }), 'Сейф'),
    navLink('Записи', !state.audit, () => pick({ type: 'all' })),
    navLink('Аудит', !!state.audit, showAudit),
    navLink('Генератор', false, () => openWindow('generator')),
    navLink('Настройки', false, () => openWindow('settings')),
    h('div', {
      style: 'display:flex;align-items:center;gap:8px;padding:6px 10px;width:250px;' +
             'border:1px solid var(--color-divider);border-radius:8px;background:var(--color-surface)',
    },
      icon('magnifying-glass', { size: 14, color: 'var(--color-neutral-600)' }),
      h('div', { style: 'flex:1;min-width:0' }, searchInputForTable()),
      h('span', { class: 'key' }, 'Ctrl+K')),
    h('button', {
      class: 'btn btn-primary', type: 'button', onClick: () => openEditor(null),
    }, icon('plus', { size: 15 }), 'Запись'),
    h('button', {
      class: 'btn btn-secondary btn-icon', type: 'button', title: 'Заблокировать', onClick: lockNow,
    }, icon('lock-key', { size: 16 })));

  const filters = h('div', { style: 'display:flex;align-items:center;gap:7px;padding:6px 0 12px;flex:none' },
    chip(`Все · ${state.counts?.total ?? 0}`, sameScope(state.scope, { type: 'all' }), () => pick({ type: 'all' })),
    KINDS.map((k) => chip(k.label, sameScope(state.scope, { type: 'kind', kind: k.kind }),
      () => pick({ type: 'kind', kind: k.kind }))),
    chip('Избранное', sameScope(state.scope, { type: 'favorites' }), () => pick({ type: 'favorites' })),
    chip('Корзина', sameScope(state.scope, { type: 'trash' }), () => pick({ type: 'trash' })),
    h('button', {
      type: 'button',
      style: 'margin-left:auto;display:flex;align-items:center;gap:6px;border:0;background:transparent;' +
             'color:var(--color-neutral-600);font-size:11.5px;cursor:pointer',
      onClick: cycleSort,
    }, icon('sort-ascending', { size: 14 }), sortTitle()));

  const rows = state.entries.map((e) => h('tr', {
    style: e.id === state.selectedId
      ? 'background:color-mix(in srgb,var(--color-accent) 10%,transparent)' : null,
    onClick: async () => { state.selectedId = e.id; await loadDetail(); renderApp(); },
    onDblClick: () => openEditor(e.id),
  },
    h('td', {}, h('span', { style: 'display:flex;align-items:center;gap:9px' },
      icon(e.icon, { size: 16, color: e.id === state.selectedId ? 'var(--color-accent)' : 'var(--color-neutral-400)' }),
      e.title)),
    h('td', { class: 'mono', style: 'font-size:13px' }, e.username || '—'),
    h('td', {}, h('span', { style: 'font-size:12.5px;color:var(--color-neutral-400)' }, e.kind_title)),
    h('td', {}, e.strength
      ? meter(e.strength, { small: true })
      : h('span', { style: 'font-size:11.5px;color:var(--color-neutral-500)' },
          e.expires_in_days !== null && e.expires_in_days !== undefined
            ? (e.expires_in_days < 0 ? 'истёк' : `истекает ${fmtDays(e.expires_in_days)}`)
            : '—')),
    h('td', { style: 'font-size:12.5px;color:var(--color-neutral-500)' }, fmtDateShort(e.modified_at))));

  const table = state.entries.length
    ? h('table', { class: 'table' },
        h('thead', {}, h('tr', {},
          h('th', { style: 'width:38%' }, 'Название'),
          h('th', {}, 'Логин'),
          h('th', { style: 'width:110px' }, 'Тип'),
          h('th', { style: 'width:130px' }, 'Надёжность'),
          h('th', { style: 'width:110px' }, 'Изменено'))),
        h('tbody', {}, rows))
    : h('div', { class: 'empty' },
        icon('tray'),
        h('div', { class: 'empty-title' }, state.query ? 'Ничего не нашлось' : 'Здесь пока пусто'));

  mount(clear(refs.stage), nav, h('div', { class: 'screen-body' },
    h('div', { class: 'col grow', style: 'padding:6px 20px 0' },
      filters,
      h('div', { class: 'scroll grow' }, table),
      h('div', {
        style: 'display:flex;align-items:center;gap:14px;padding:12px 0;flex:none;' +
               'font-size:11.5px;color:var(--color-neutral-600)',
      },
        h('span', {}, state.selectedId ? 'Выбрана 1 запись' : 'Запись не выбрана'),
        h('span', {}, '·'),
        h('span', {}, '⏎ копировать пароль'),
        h('span', {}, '·'),
        h('span', {}, 'Двойной щелчок — правка'),
        h('span', { style: 'margin-left:auto' },
          `${state.counts?.total ?? 0} ${plural(state.counts?.total ?? 0, 'запись', 'записи', 'записей')} · синхронизация не требуется`))),
    tableAside()));
  fillAuditCount();
}

function searchInputForTable() {
  refs.search = h('input', {
    class: 'input input-bare',
    style: 'font-size:12.5px',
    placeholder: 'Поиск', value: state.query, autocomplete: 'off',
    onInput: (e) => {
      state.query = e.target.value;
      clearTimeout(searchTimer);
      searchTimer = setTimeout(refresh, 110);
    },
  });
  return refs.search;
}

function navLink(label, active, onClick) {
  return h('a', {
    href: '#', 'aria-current': active ? 'page' : null,
    onClick: (e) => { e.preventDefault(); onClick(); },
  }, label);
}

function chip(label, active, onClick) {
  return h('button', {
    class: `tag is-button ${active ? 'tag-outline' : 'tag-neutral'}`,
    type: 'button', onClick,
  }, label);
}

/** Правая колонка вида 1e — сжатая карточка выбранной записи. */
function tableAside() {
  const entry = state.detail;
  const box = h('div', {
    class: 'col scroll',
    style: 'width:296px;flex:none;border-left:1px solid var(--color-divider);padding:18px;gap:14px',
  });

  if (!entry) {
    mount(box, h('div', { class: 'empty' },
      icon('cursor-click'),
      h('div', { class: 'empty-title' }, 'Выберите строку')));
    return box;
  }

  mount(box, 
    h('div', { style: 'display:flex;align-items:center;gap:11px' },
      entryIcon(entry, { size: 38, accent: true }),
      h('div', { style: 'min-width:0' },
        h('div', { style: 'font-size:15px;font-weight:500' }, entry.title),
        h('div', { style: 'font-size:11.5px;color:var(--color-neutral-500)' },
          entry.host || entry.kind_title))),

    entry.has_password ? h('button', {
      class: 'btn btn-primary btn-block', style: 'margin:0', type: 'button',
      onClick: () => copy(entry.id, 'password', 'Пароль'),
    }, icon('copy', { size: 15 }), 'Копировать пароль') : null,

    h('div', { style: 'display:flex;flex-direction:column;gap:7px' },
      entry.username ? compactField('Логин', entry.username, () => copy(entry.id, 'username', 'Логин')) : null,
      entry.email ? compactField('Почта', entry.email, () => copy(entry.id, 'email', 'Почта')) : null,
      entry.has_password ? compactField('Пароль', entry.password_masked, () => copy(entry.id, 'password', 'Пароль')) : null,
      entry.has_totp ? compactField('Код 2FA', 'появится позже', null) : null),

    h('div', { class: 'rule' }),

    h('div', { style: 'display:flex;flex-direction:column;gap:6px;font-size:11.5px;color:var(--color-neutral-500)' },
      metaRow('Папка', state.folders.find((f) => f.id === entry.folder)?.name || '—'),
      metaRow('Изменено', fmtDate(entry.modified_at)),
      metaRow('Пароль обновлён', entry.password_modified_at ? fmtAgo(entry.password_modified_at) : '—')),

    h('div', { class: 'spacer', style: 'display:flex;gap:6px' },
      h('button', {
        class: 'btn btn-secondary', style: 'flex:1', type: 'button',
        onClick: () => openEditor(entry.id),
      }, icon('pencil-simple', { size: 15 }), 'Изменить'),
      h('button', {
        class: 'btn btn-secondary btn-icon', type: 'button', title: 'В корзину',
        onClick: () => guard(async () => { await deleteEntry(entry.id); await refresh(); }),
      }, icon('trash', { size: 16 }))));

  return box;
}

function compactField(label, value, onCopy) {
  return h('div', {
    style: 'padding:8px 10px;border:1px solid var(--color-divider);border-radius:8px;' +
           'display:flex;align-items:center;gap:8px',
    onClick: onCopy || null,
    title: onCopy ? 'Щелчок копирует' : null,
  },
    h('div', { style: 'flex:1;min-width:0' },
      h('div', { class: 'fb-label' }, label),
      h('div', { class: 'fb-value', style: 'font-size:13.5px' }, value)),
    onCopy ? icon('copy', { size: 14, color: 'var(--color-neutral-600)' }) : null);
}

function metaRow(label, value) {
  return h('div', { style: 'display:flex;justify-content:space-between;gap:12px' },
    h('span', {}, label),
    h('span', { style: 'color:var(--color-text);text-align:right' }, value));
}

// ═══ аудит ═════════════════════════════════════════════════════════════════

const ISSUE_LABEL = {
  weak: 'слабый пароль', reused: 'повтор',
  stale: 'давно не менялся', expiring: 'скоро истекает', expired: 'истёк',
};

async function showAudit() {
  state.audit = await guard(() => auditReport());
  if (!state.audit) return;
  renderAudit();
}

async function fillAuditCount() {
  const report = await guard(() => auditReport(), { silent: true });
  const n = report?.findings.length ?? 0;
  if (refs.auditCount) {
    refs.auditCount.replaceChildren();
    if (n) {
      refs.auditCount.className = 'tag tag-accent';
      refs.auditCount.style.marginLeft = 'auto';
      refs.auditCount.textContent = n;
    } else {
      refs.auditCount.className = 'count';
      refs.auditCount.textContent = '';
    }
  }
}

function renderAudit() {
  const report = state.audit;
  const body = h('div', { class: 'col scroll grow', style: 'padding:24px 28px' },
    h('h4', {}, 'Аудит'),
    h('p', { style: 'font-size:12.5px;color:var(--color-neutral-500);margin-bottom:6px' },
      'Проверка идёт на этом устройстве и никуда не отправляет данные. ' +
      'Оценка стойкости приблизительная — по составу символов; сверки с базами утечек здесь нет.'),

    h('div', { style: 'display:flex;gap:8px;margin:12px 0 18px;flex-wrap:wrap' },
      statTile('Слабые', report.weak),
      statTile('Повторы', report.reused),
      statTile('Давние', report.stale),
      statTile('По сроку', report.expiring)),

    report.findings.length === 0
      ? h('div', { class: 'empty' },
          icon('shield-check', { size: 30 }),
          h('div', { class: 'empty-title' }, 'Замечаний нет'),
          h('div', { class: 'empty-hint' }, 'Все пароли стойкие, не повторяются и не устарели.'))
      : h('div', { style: 'display:flex;flex-direction:column;gap:7px' },
          report.findings.map((f) => h('button', {
            class: 'fieldbox', type: 'button', style: 'cursor:pointer;text-align:left',
            onClick: () => openEditor(f.entry_id),
          },
            icon(f.issue === 'weak' ? 'shield-warning'
               : f.issue === 'reused' ? 'copy-simple'
               : f.issue === 'stale' ? 'clock-counter-clockwise' : 'hourglass',
              { size: 17, color: 'var(--color-accent)' }),
            h('div', { class: 'fb-text' },
              h('div', { style: 'font-size:13.5px' }, f.title),
              h('div', { style: 'font-size:11.5px;color:var(--color-neutral-500)' },
                `${ISSUE_LABEL[f.issue]} · ${f.detail}`)),
            icon('caret-right', { size: 14, color: 'var(--color-neutral-600)' })))));

  mount(clear(refs.stage), h('div', { class: 'screen-body' }, sidebar(), body));
  fillAuditCount();
  updateLockHint();
}

function statTile(label, value) {
  return h('div', {
    style: 'flex:1;min-width:120px;padding:12px 14px;border-radius:10px;' +
           'border:1px solid var(--color-divider)',
  },
    h('div', { style: `font-size:24px;line-height:1.1;color:var(--color-${value ? 'accent' : 'neutral-600'})` }, value),
    h('div', { class: 'kicker', style: 'margin-top:4px' }, label));
}

// ═══ прочие действия ═══════════════════════════════════════════════════════

async function newFolder() {
  const name = prompt('Название папки');
  if (!name?.trim()) return;
  await guard(async () => { await call('add_folder', { name: name.trim() }); await refresh(); });
}

async function lockNow() {
  await guard(() => call('lock_vault'));
}

// ═══ события и клавиатура ══════════════════════════════════════════════════

listen('vault-locked', (e) => {
  clearInterval(countdownTimer);
  state.selectedId = null;
  state.detail = null;
  state.audit = null;
  boot();
  if (e.payload === 'autolock') {
    setTimeout(() => toast('Сейф заблокирован из-за бездействия', { glyph: 'lock-key' }), 200);
  }
});

listen('vault-unlocked', () => boot());
listen('entries-changed', () => { if (state.settings) refresh(); });
listen('need-unlock', () => boot());
listen('show-audit', () => showAudit());
listen('settings-changed', async (e) => {
  state.settings = e.payload;
  await refresh();
});
listen('hotkey-failed', (e) => {
  toast(`Горячая клавиша недоступна: ${e.payload}`, { error: true, ms: 6000 });
});

window.addEventListener('keydown', async (e) => {
  const typing = e.target.matches('input,textarea,select');

  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
    e.preventDefault();
    refs.search?.focus();
    refs.search?.select();
    return;
  }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'n') {
    e.preventDefault();
    openEditor(null);
    return;
  }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'l') {
    e.preventDefault();
    lockNow();
    return;
  }
  if (e.key === 'Escape' && typing && refs.search === e.target) {
    e.preventDefault();
    state.query = '';
    refs.search.value = '';
    await refresh();
    return;
  }
  if (typing) return;

  if (e.key === 'Enter' && state.detail?.has_password) {
    e.preventDefault();
    copy(state.detail.id, 'password', 'Пароль');
    return;
  }
  if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
    e.preventDefault();
    const i = state.entries.findIndex((x) => x.id === state.selectedId);
    const next = Math.min(Math.max(i + (e.key === 'ArrowDown' ? 1 : -1), 0), state.entries.length - 1);
    if (state.entries[next]) {
      state.selectedId = state.entries[next].id;
      await loadDetail();
      renderApp();
    }
  }
});

trackActivity();
boot();
