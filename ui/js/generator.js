// Экран 1h — генератор паролей.
//
// Считает всё ядро: и случайность, и энтропию. Здесь только органы
// управления и показ результата — иначе пароль пришлось бы получать из
// Math.random(), который для этого непригоден.

import { generatePassword, copyText, listen } from './api.js';
import { h, icon, $, clear, mount } from './ui.js';
import { setupAuxWindow, toast, toastCopied, guard } from './chrome.js';

const MODES = [
  { id: 'password',   label: 'Пароль', min: 8, max: 64, step: 1, def: 16, unit: 'Длина' },
  { id: 'passphrase', label: 'Фраза',  min: 3, max: 12, step: 1, def: 5,  unit: 'Слов' },
  { id: 'pin',        label: 'PIN',    min: 4, max: 12, step: 1, def: 6,  unit: 'Цифр' },
];

const state = {
  mode: 'password',
  length: 16,
  uppercase: true,
  digits: true,
  symbols: true,
  exclude_lookalike: false,
  separator: '-',
  result: null,
  /** Последние выданные пароли — кнопка «История (12)» в макете.
      Живут только в памяти окна и исчезают вместе с ним. */
  history: [],
};

const refs = {};

function modeSpec() {
  return MODES.find((m) => m.id === state.mode);
}

function build() {
  const root = setupAuxWindow('Генератор', { buttons: ['min', 'close'] });

  // ── плашка результата ────────────────────────────────────────────────────
  refs.value = h('div', {
    class: 'mono selectable',
    style: 'font-size:20px;line-height:1.4;word-break:break-all;letter-spacing:.02em;min-height:28px',
  });
  refs.meterFill = h('span', { style: 'width:0%' });
  refs.bits = h('span', { style: 'font-size:11px;color:var(--color-neutral-400);white-space:nowrap' });

  const output = h('div', {
    style: 'padding:16px 16px 14px;border:1px solid var(--color-accent);border-radius:10px;' +
           'background:color-mix(in srgb,var(--color-accent) 7%,transparent)',
  },
    refs.value,
    h('div', { style: 'display:flex;align-items:center;gap:9px;margin-top:12px' },
      h('span', { class: 'meter' }, refs.meterFill),
      refs.bits,
      h('button', {
        class: 'btn btn-icon btn-ghost', style: 'width:28px;height:28px',
        title: 'Сгенерировать заново', type: 'button', onClick: regenerate,
      }, icon('arrows-clockwise', { size: 16 })),
      h('button', {
        class: 'btn btn-icon btn-primary', style: 'width:28px;height:28px',
        title: 'Копировать', type: 'button', onClick: copy,
      }, icon('copy', { size: 15 }))));

  // ── переключатель режима ─────────────────────────────────────────────────
  const seg = h('div', { class: 'seg', style: 'width:100%' },
    MODES.map((m) => h('label', { class: 'seg-opt', style: 'flex:1;justify-content:center' },
      h('input', {
        type: 'radio', name: 'gen', checked: m.id === state.mode,
        onChange: () => {
          state.mode = m.id;
          state.length = m.def;
          syncLength();
          regenerate();
        },
      }),
      m.label)));

  // ── длина ────────────────────────────────────────────────────────────────
  refs.lengthLabel = h('span', { style: 'font-size:12px;color:color-mix(in srgb,var(--color-text) 70%,transparent)' });
  refs.lengthValue = h('span', { class: 'mono', style: 'margin-left:auto;font-size:13px;color:var(--color-accent)' });
  refs.slider = h('input', {
    type: 'range', class: 'slider',
    onInput: (e) => {
      state.length = Number(e.target.value);
      syncLength();
      regenerate();
    },
  });
  refs.min = h('span', {});
  refs.max = h('span', {});

  const lengthBlock = h('div', {},
    h('div', { style: 'display:flex;align-items:baseline;margin-bottom:8px' }, refs.lengthLabel, refs.lengthValue),
    refs.slider,
    h('div', { style: 'display:flex;justify-content:space-between;font-size:10.5px;color:var(--color-neutral-600);margin-top:4px' },
      refs.min, refs.max));

  // ── наборы символов ──────────────────────────────────────────────────────
  const toggle = (key, label) => h('label', { class: 'radio check', style: 'display:flex;gap:9px' },
    h('input', {
      type: 'checkbox', checked: state[key],
      onChange: (e) => { state[key] = e.target.checked; regenerate(); },
    }),
    h('span', { class: 'dot' }),
    label);

  refs.options = h('div', { style: 'display:flex;flex-direction:column;gap:9px' });

  // ── низ ──────────────────────────────────────────────────────────────────
  refs.historyBtn = h('button', {
    class: 'btn btn-ghost', style: 'font-size:12.5px', type: 'button', onClick: showHistory,
  }, icon('clock-counter-clockwise', { size: 15 }), 'История');

  const footer = h('div', { style: 'display:flex;align-items:center;gap:8px' },
    refs.historyBtn,
    h('button', {
      class: 'btn btn-secondary', style: 'margin-left:auto', type: 'button', onClick: fillField,
    }, 'Заполнить поле'));

  mount(root, h('div', {
    class: 'scroll',
    style: 'padding:20px 20px 22px;display:flex;flex-direction:column;gap:16px',
  }, output, seg, lengthBlock, refs.options,
     h('div', { class: 'rule' }), footer));

  refs.toggle = toggle;
  syncLength();
  renderOptions();
  regenerate();
}

/** Приводит подпись, ползунок и границы к текущему режиму. */
function syncLength() {
  const spec = modeSpec();
  state.length = Math.min(Math.max(state.length, spec.min), spec.max);
  refs.lengthLabel.textContent = spec.unit;
  refs.lengthValue.textContent = state.length;
  refs.min.textContent = spec.min;
  refs.max.textContent = spec.max;
  Object.assign(refs.slider, { min: spec.min, max: spec.max, step: spec.step, value: state.length });
  const pct = ((state.length - spec.min) / (spec.max - spec.min)) * 100;
  refs.slider.style.setProperty('--fill', `${pct}%`);
}

/** У фразы и PIN свой набор флажков: «Символы !@#$» к цифровому коду
    отношения не имеют и только сбивали бы с толку. */
function renderOptions() {
  clear(refs.options);
  const t = refs.toggle;
  if (state.mode === 'password') {
    mount(refs.options, 
      t('uppercase', 'Прописные A–Z'),
      t('digits', 'Цифры 0–9'),
      t('symbols', 'Символы !@#$'),
      t('exclude_lookalike', 'Исключить похожие (0/O, 1/l)'));
  } else if (state.mode === 'passphrase') {
    mount(refs.options, 
      t('uppercase', 'Часть слов с прописной'),
      t('digits', 'Добавить цифру в конец'));
  }
}

async function regenerate() {
  renderOptions();
  const g = await guard(() => generatePassword({
    mode: state.mode,
    length: state.length,
    uppercase: state.uppercase,
    digits: state.digits,
    symbols: state.symbols,
    exclude_lookalike: state.exclude_lookalike,
    separator: state.separator,
  }));
  if (!g) return;

  state.result = g;
  refs.value.textContent = g.value;
  refs.meterFill.style.width = `${Math.round(g.fill * 100)}%`;
  refs.bits.textContent = `${Math.round(g.entropy_bits)} бит`;

  state.history.unshift(g.value);
  state.history = state.history.slice(0, 12);
  refs.historyBtn.replaceChildren(
    icon('clock-counter-clockwise', { size: 15 }),
    `История (${state.history.length})`);
}

async function copy() {
  if (!state.result) return;
  const secs = await guard(() => copyText(state.result.value));
  if (secs !== undefined) toastCopied('Пароль', secs);
}

/**
 * «Заполнить поле» — отдаёт значение окну редактора, если оно открыто.
 * Событие уходит через ядро, потому что окна между собой не общаются.
 */
async function fillField() {
  if (!state.result) return;
  await guard(async () => {
    const { emit } = window.__TAURI__.event;
    await emit('generated-password', state.result.value);
    toast('Пароль передан в открытую карточку записи');
  });
}

function showHistory() {
  if (state.history.length <= 1) {
    toast('Пока сгенерирован только текущий пароль');
    return;
  }
  // История нужна на случай «сгенерировал, ушёл, а он был нужен».
  // Клик по строке возвращает пароль в плашку результата.
  const list = state.history.slice(1);
  const box = h('div', { class: 'dialog-backdrop', onClick: (e) => { if (e.target === box) box.remove(); } },
    h('div', { class: 'dialog', style: 'width:min(400px,100%)' },
      h('div', { class: 'dialog-title' }, 'Недавно сгенерированные'),
      h('div', { class: 'dialog-body dim', style: 'font-size:12px' },
        'Список живёт только пока открыто это окно и никуда не записывается.'),
      h('div', { class: 'scroll', style: 'display:flex;flex-direction:column;gap:6px;max-height:260px' },
        list.map((value) => h('button', {
          class: 'fieldbox', type: 'button', style: 'cursor:pointer;text-align:left',
          onClick: () => {
            refs.value.textContent = value;
            state.result = { ...state.result, value };
            box.remove();
          },
        }, h('div', { class: 'fb-text' }, h('div', { class: 'fb-value mono' }, value))))),
      h('div', { class: 'dialog-actions' },
        h('button', { class: 'btn btn-secondary', type: 'button', onClick: () => box.remove() }, 'Закрыть'))));
  mount(document.body, box);
}

// Пробел и Enter перегенерируют — так быстрее перебирать варианты.
window.addEventListener('keydown', (e) => {
  if (e.target.matches('input,textarea')) return;
  if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); regenerate(); }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'c') { e.preventDefault(); copy(); }
});

listen('settings-changed', () => {});
build();
