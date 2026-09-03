// Экран 1g — карточка записи в режиме редактирования.
//
// Это единственное место, где пароль поднимается в интерфейс целиком:
// его нужно видеть и править. Открывается такая карточка только по явному
// действию пользователя.

import {
  call, listen, loadDraft, saveDraft, folders, tags, estimate, generatePassword, copyText,
} from './api.js';
import { h, icon, $, clear, mount } from './ui.js';
import { setupAuxWindow, toast, toastCopied, guard } from './chrome.js';

const KINDS = [
  { value: 'password', label: 'Пароль' },
  { value: 'note', label: 'Заметка' },
  { value: 'api_key', label: 'Ключ API' },
  { value: 'document', label: 'Документ' },
];

let draft = null;
let saved = null;      // снимок для сравнения «есть ли несохранённое»
let allFolders = [];
let allTags = [];
const refs = {};

const snapshot = () => JSON.stringify(draft);
const isDirty = () => snapshot() !== saved;

function markDirty() {
  const dirty = isDirty();
  refs.dirtyIcon.className = dirty ? 'ph ph-circle-dashed' : 'ph ph-check-circle';
  refs.dirtyIcon.style.color = dirty ? 'var(--color-accent)' : 'var(--color-neutral-600)';
  refs.dirtyText.textContent = dirty ? 'Есть несохранённые изменения' : 'Всё сохранено';
  refs.save.disabled = !dirty;
}

/** Поле ввода, связанное с полем черновика. */
function bound(key, props = {}) {
  const node = h(props.multiline ? 'textarea' : 'input', {
    class: 'input',
    value: draft[key] ?? '',
    ...props,
    onInput: (e) => {
      draft[key] = e.target.value;
      props.onAfter?.(e.target.value);
      markDirty();
    },
  });
  delete node.multiline;
  return node;
}

function field(label, control, { flex } = {}) {
  return h('div', { class: 'field', style: flex ? `flex:${flex}` : null },
    h('label', {}, label), control);
}

// ── левая колонка ──────────────────────────────────────────────────────────

function renderPasswordBlock() {
  refs.password = h('input', {
    class: 'input mono',
    type: 'password',
    value: draft.password,
    style: 'border-color:var(--color-accent)',
    onInput: (e) => { draft.password = e.target.value; refreshStrength(); markDirty(); },
  });

  refs.meterFill = h('span', { style: 'width:0%' });
  refs.meterText = h('span', { style: 'font-size:11px;color:var(--color-neutral-400)' });

  const eye = h('button', {
    class: 'btn btn-secondary btn-icon', type: 'button', title: 'Показать пароль',
    onClick: () => {
      const shown = refs.password.type === 'text';
      refs.password.type = shown ? 'password' : 'text';
      eye.replaceChildren(icon(shown ? 'eye' : 'eye-slash', { size: 16 }));
    },
  }, icon('eye', { size: 16 }));

  return h('div', { class: 'field' },
    h('label', { for: 'pw' }, 'Пароль'),
    h('div', { style: 'display:flex;gap:8px' },
      refs.password,
      eye,
      h('button', {
        class: 'btn btn-secondary btn-icon', type: 'button', title: 'Копировать',
        onClick: () => guard(async () => {
          if (!draft.password) return toast('Пароль пуст', { error: true });
          const secs = await copyText(draft.password);
          toastCopied('Пароль', secs);
        }),
      }, icon('copy', { size: 16 })),
      h('button', {
        class: 'btn btn-primary', type: 'button', onClick: generateHere,
      }, icon('arrows-clockwise', { size: 15 }), 'Сгенерировать')),
    h('div', { class: 'meter-row', style: 'margin-top:7px' },
      h('span', { class: 'meter' }, refs.meterFill),
      refs.meterText));
}

async function refreshStrength() {
  if (!draft.password) {
    refs.meterFill.style.width = '0%';
    refs.meterText.textContent = 'пароль не задан';
    return;
  }
  const s = await guard(() => estimate(draft.password), { silent: true });
  if (!s) return;
  refs.meterFill.style.width = `${Math.round(s.fill * 100)}%`;
  refs.meterText.textContent = `${Math.round(s.bits)} бит энтропии · ${s.label}`;
}

/** Кнопка «Сгенерировать» подставляет пароль прямо в поле — без
    промежуточного окна, как в макете. */
async function generateHere() {
  const g = await guard(() => generatePassword({
    mode: 'password', length: 16, uppercase: true, digits: true,
    symbols: true, exclude_lookalike: false, separator: '-',
  }));
  if (!g) return;
  draft.password = g.value;
  refs.password.value = g.value;
  refs.password.type = 'text'; // показать сразу: иначе непонятно, что он сменился
  refreshStrength();
  markDirty();
}

function renderCustomFields() {
  const list = h('div', { style: 'display:flex;flex-direction:column;gap:10px' });

  const draw = () => {
    clear(list);
    draft.custom.forEach((f, i) => {
      mount(list, h('div', { style: 'display:flex;gap:10px;align-items:flex-end' },
        field('Название', h('input', {
          class: 'input', value: f.label,
          onInput: (e) => { f.label = e.target.value; markDirty(); },
        }), { flex: '1' }),
        field('Значение', h('input', {
          class: 'input mono', value: f.value, type: f.secret ? 'password' : 'text',
          onInput: (e) => { f.value = e.target.value; markDirty(); },
        }), { flex: '1' }),
        h('button', {
          class: 'btn btn-secondary btn-icon', type: 'button',
          title: f.secret ? 'Скрытое поле' : 'Обычное поле',
          onClick: () => { f.secret = !f.secret; markDirty(); draw(); },
        }, icon(f.secret ? 'lock-simple' : 'lock-simple-open', { size: 16 })),
        h('button', {
          class: 'btn btn-secondary btn-icon', type: 'button', title: 'Удалить поле',
          onClick: () => { draft.custom.splice(i, 1); markDirty(); draw(); },
        }, icon('trash', { size: 16 }))));
    });
    mount(list, h('button', {
      class: 'btn btn-secondary', type: 'button', style: 'align-self:flex-start',
      onClick: () => {
        draft.custom.push({ label: '', value: '', secret: false });
        markDirty();
        draw();
      },
    }, icon('plus', { size: 15 }), 'Поле'));
  };
  draw();

  return h('div', {},
    h('div', { class: 'kicker', style: 'margin-bottom:7px' }, 'Свои поля'),
    list);
}

// ── правая колонка ─────────────────────────────────────────────────────────

function renderSidebar() {
  const folderSelect = h('select', {
    class: 'input',
    onChange: (e) => { draft.folder = e.target.value || null; markDirty(); },
  },
    h('option', { value: '' }, 'Без папки'),
    allFolders.map((f) => h('option', { value: f.id, selected: draft.folder === f.id }, f.name)));

  refs.tagRow = h('div', { class: 'tagrow' });
  drawTags();

  refs.history = h('div', { style: 'display:flex;flex-direction:column;gap:6px' });

  return h('div', {
    class: 'col scroll',
    style: 'width:280px;flex:none;border-left:1px solid var(--color-divider);padding:22px 18px;gap:16px',
  },
    field('Тип записи', h('select', {
      class: 'input',
      onChange: (e) => { draft.kind = e.target.value; markDirty(); },
    }, KINDS.map((k) => h('option', { value: k.value, selected: draft.kind === k.value }, k.label)))),

    field('Папка', folderSelect),

    h('div', {},
      h('div', { style: 'font-size:12px;color:color-mix(in srgb,var(--color-text) 70%,transparent);margin-bottom:6px' }, 'Теги'),
      refs.tagRow),

    h('div', {},
      h('div', { style: 'font-size:12px;color:color-mix(in srgb,var(--color-text) 70%,transparent);margin-bottom:6px' }, 'Быстрый доступ'),
      h('label', { class: 'radio check', style: 'display:flex;gap:8px' },
        h('input', {
          type: 'checkbox', checked: draft.quick_access,
          onChange: (e) => { draft.quick_access = e.target.checked; markDirty(); },
        }),
        h('span', { class: 'dot' }),
        'Показывать в мини-окне'),
      h('label', { class: 'radio check', style: 'display:flex;gap:8px;margin-top:8px' },
        h('input', {
          type: 'checkbox', checked: draft.favorite,
          onChange: (e) => { draft.favorite = e.target.checked; markDirty(); },
        }),
        h('span', { class: 'dot' }),
        'В избранном')),

    h('div', {},
      h('div', { style: 'font-size:12px;color:color-mix(in srgb,var(--color-text) 70%,transparent);margin-bottom:6px' }, 'История пароля'),
      refs.history),

    h('div', { class: 'spacer', style: 'font-size:11px;color:var(--color-neutral-600);line-height:1.5' },
      'Изменения шифруются локально и записываются в хранилище при сохранении.'));
}

function drawTags() {
  mount(clear(refs.tagRow), 
    draft.tags.map((t) => h('button', {
      class: 'tag tag-neutral is-button', type: 'button', title: 'Убрать тег',
      onClick: () => {
        draft.tags = draft.tags.filter((x) => x !== t);
        markDirty();
        drawTags();
      },
    }, t)),
    h('button', {
      class: 'tag tag-outline is-button', type: 'button', onClick: addTag,
    }, '+ добавить'));
}

function addTag() {
  const suggestions = allTags.map(([name]) => name).filter((n) => !draft.tags.includes(n));
  const name = prompt(
    suggestions.length
      ? `Тег\n\nУже используются: ${suggestions.slice(0, 12).join(', ')}`
      : 'Тег',
    '');
  const clean = (name || '').trim();
  if (!clean || draft.tags.includes(clean)) return;
  draft.tags.push(clean);
  markDirty();
  drawTags();
}

/** История паролей приходит из ядра только точками и датой — сами прежние
    значения наверх не поднимаются никогда. */
async function fillHistory() {
  if (!draft.id) {
    mount(refs.history, h('div', { class: 'dim', style: 'font-size:11.5px' }, 'Записи ещё нет'));
    return;
  }
  const entry = await guard(() => call('get_entry', { id: draft.id }), { silent: true });
  clear(refs.history);
  if (!entry || entry.history.length === 0) {
    mount(refs.history, h('div', { class: 'dim', style: 'font-size:11.5px' }, 'Пароль ещё не менялся'));
    return;
  }
  for (const item of entry.history) {
    mount(refs.history, h('div', {
      style: 'display:flex;align-items:center;gap:8px;padding:7px 9px;' +
             'border:1px solid var(--color-divider);border-radius:8px',
    },
      h('span', { class: 'mono', style: 'flex:1;font-size:12px;color:var(--color-neutral-400)' }, item.masked),
      h('span', { style: 'font-size:10.5px;color:var(--color-neutral-600)' },
        new Date(item.replaced_at).toLocaleDateString('ru-RU', { month: 'short', year: 'numeric' }))));
  }
}

// ── сохранение и выход ─────────────────────────────────────────────────────

async function save() {
  if (!draft.title.trim()) {
    toast('У записи должно быть название', { error: true });
    refs.title.focus();
    return;
  }
  const id = await guard(() => saveDraft(draft));
  if (!id) return;
  draft.id = id;
  saved = snapshot();
  markDirty();
  toast('Запись сохранена');
}

/** Esc при несохранённых правках сначала спрашивает — закрывать окно
    поверх потерянной работы нельзя. */
function confirmDiscard() {
  if (!isDirty()) return true;
  return confirm('Есть несохранённые изменения. Закрыть карточку и потерять их?');
}

// ── сборка ─────────────────────────────────────────────────────────────────

async function open(id) {
  draft = await loadDraft(id ?? null);
  draft.custom ||= [];
  draft.tags ||= [];
  if (id === null || id === undefined) draft.quick_access = true;
  saved = snapshot();

  [allFolders, allTags] = await Promise.all([
    guard(() => folders(), { silent: true }).then((v) => v || []),
    guard(() => tags(), { silent: true }).then((v) => v || []),
  ]);

  render();
  refreshStrength();
  fillHistory();
}

function render() {
  const root = $('#app');
  clear(root);
  mount(root, refs.titlebar);

  refs.title = bound('title', { placeholder: 'Например, GitHub' });

  const left = h('div', {
    class: 'col scroll',
    style: 'flex:1;min-width:0;padding:22px 24px;gap:12px',
  },
    h('div', { style: 'display:flex;align-items:center;gap:12px' },
      h('div', {
        class: 'row-icon is-accent',
        style: 'width:44px;height:44px;border-radius:11px;font-size:22px',
      }, icon(draft.id ? 'vault' : 'plus')),
      field('Название', refs.title, { flex: '1' })),

    h('div', { style: 'display:flex;gap:10px' },
      field('Логин', bound('username', { autocomplete: 'off' }), { flex: '1' }),
      field('Адрес', bound('url', { placeholder: 'https://', autocomplete: 'off' }), { flex: '1' })),

    renderPasswordBlock(),

    field('Одноразовые коды (TOTP)', h('input', {
      class: 'input mono', style: 'font-size:12.5px',
      value: draft.totp || '',
      placeholder: 'otpauth://totp/…',
      onInput: (e) => { draft.totp = e.target.value || null; markDirty(); },
    })),

    field('Заметка', bound('note', { multiline: true, style: 'min-height:88px' })),

    renderCustomFields());

  refs.dirtyIcon = icon('circle-dashed', { size: 15 });
  refs.dirtyText = h('span', { style: 'font-size:12px;color:var(--color-neutral-400)' });
  refs.save = h('button', { class: 'btn btn-primary', type: 'button', onClick: save }, 'Сохранить · Ctrl+S');

  const footer = h('div', {
    style: 'display:flex;align-items:center;gap:10px;padding:12px 20px;flex:none;' +
           'border-top:1px solid var(--color-divider);' +
           'background:color-mix(in srgb,var(--color-surface) 60%,transparent)',
  },
    refs.dirtyIcon, refs.dirtyText,
    h('div', { style: 'margin-left:auto;display:flex;gap:8px' },
      h('button', {
        class: 'btn btn-secondary', type: 'button',
        onClick: () => { if (confirmDiscard()) open(draft.id); },
      }, 'Отменить'),
      refs.save));

  mount(root, h('div', { class: 'screen-body' }, left, renderSidebar()), footer);
  markDirty();
}

// ── запуск ─────────────────────────────────────────────────────────────────

const root = setupAuxWindow('Редактирование', {
  buttons: ['min', 'close'],
  onEscape: confirmDiscard,
});
refs.titlebar = root.firstElementChild;

// Окно переиспользуется: главное окно шлёт сюда, какую запись открыть.
listen('open-entry', (e) => {
  if (!confirmDiscard()) return;
  open(e.payload ?? null);
});

// Генератор может отдать пароль в открытую карточку.
listen('generated-password', (e) => {
  if (!draft) return;
  draft.password = e.payload;
  refs.password.value = e.payload;
  refs.password.type = 'text';
  refreshStrength();
  markDirty();
  toast('Пароль подставлен из генератора');
});

window.addEventListener('keydown', (e) => {
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
    e.preventDefault();
    save();
  }
});

open(null);
