// Экран 1c — мини-окно у значка в трее.
//
// Самый короткий путь к паролю: список недавних записей, копирование одним
// щелчком. Окно без рамки, поверх остальных, исчезает при потере фокуса —
// пока не закреплено булавкой.
//
// Где окну появиться, решает Rust (`windows::place_tray`): двигать окна по
// экрану — не дело вебвью, и разрешения на это у интерфейса нет. Отсюда окно
// можно только перетащить за полосу заголовка, и это единственный способ,
// который работает под Wayland: там положение окна назначает композитор.

import {
  call, listen, currentWindow, recent, getSettings, setSettings, status, copyField,
  lockCountdown,
} from './api.js';
import { h, icon, $, clear, entryIcon, fmtCountdown, mount } from './ui.js';
import { toast, toastCopied, guard, trackActivity, wipeOnLock } from './chrome.js';

let settings = null;
let items = [];
let countdownTimer = null;
const refs = {};

/**
 * Делает полосу заголовка ручкой: за неё окно таскают по экрану.
 *
 * Штатный `data-tauri-drag-region` срабатывает только на самом элементе с
 * атрибутом — щелчок по значку или подписи внутри полосы окно уже не двигает,
 * а полоса из них почти целиком и состоит. Поэтому перетаскивание запускается
 * вручную, с любого её места, кроме кнопок.
 */
function draggable(node) {
  node.addEventListener('pointerdown', (e) => {
    if (e.button !== 0 || e.target.closest('button')) return;
    currentWindow().startDragging().catch(() => {});
  });
  return node;
}

/** Закрепляет окно на экране или отпускает его. */
async function togglePin() {
  const saved = await guard(() => setSettings({ ...settings, tray_pinned: !settings.tray_pinned }));
  if (!saved) return;
  settings = saved;
  drawPin();
  toast(settings.tray_pinned
    ? 'Окно закреплено — останется на экране'
    : 'Окно откреплено — спрячется при щелчке мимо', { glyph: 'push-pin' });
}

/** Вид булавки: залитая и подсвеченная — закреплено. */
function drawPin() {
  const on = Boolean(settings?.tray_pinned);
  refs.pin.title = on
    ? 'Открепить: окно будет прятаться, как раньше'
    : 'Закрепить: окно останется поверх остальных';
  refs.pin.classList.toggle('btn-primary', on);
  refs.pin.classList.toggle('btn-ghost', !on);
  mount(clear(refs.pin), icon('push-pin', { size: 14, fill: on }));
}

// ── разметка ───────────────────────────────────────────────────────────────

function build() {
  refs.lockLabel = h('span', { style: 'font-size:11px;color:var(--color-neutral-500)' });

  refs.pin = h('button', {
    class: 'btn btn-icon btn-ghost', style: 'width:24px;height:24px;margin-left:auto',
    type: 'button', onClick: togglePin,
  });
  drawPin();

  const header = draggable(h('div', {
    style: 'display:flex;align-items:center;gap:8px;padding:11px 13px;flex:none;cursor:grab;' +
           'border-bottom:1px solid var(--color-divider)',
  },
    icon('lock-simple-open', { fill: true, size: 15, color: 'var(--color-accent)' }),
    h('span', { style: 'font-size:13px;font-weight:500' }, 'Сейф'),
    refs.lockLabel,
    refs.pin,
    h('button', {
      class: 'btn btn-icon btn-ghost', style: 'width:24px;height:24px',
      type: 'button', title: 'Заблокировать',
      onClick: () => guard(async () => { await call('lock_vault'); await currentWindow().hide(); }),
    }, icon('lock-key', { size: 14 }))));

  refs.searchLabel = h('span', { style: 'font-size:12.5px;color:var(--color-neutral-600)' }, 'Поиск');

  const searchBox = h('div', { style: 'padding:9px 11px 4px;flex:none' },
    h('button', {
      type: 'button',
      style: 'display:flex;align-items:center;gap:8px;width:100%;padding:6px 9px;text-align:left;' +
             'border:1px solid var(--color-divider);border-radius:8px;background:var(--color-surface);' +
             'color:inherit;cursor:pointer',
      // Полноценный поиск — это палитра; дублировать её в 330 пикселях незачем.
      onClick: () => guard(async () => {
        await currentWindow().hide();
        await call('open_window', { label: 'quick' });
        await window.__TAURI__.event.emit('quick-opened');
      }),
    },
      icon('magnifying-glass', { size: 14, color: 'var(--color-neutral-600)' }),
      refs.searchLabel));

  refs.list = h('div', { class: 'scroll', style: 'flex:1;padding-bottom:4px' });

  const footer = h('div', {
    style: 'display:flex;align-items:center;gap:8px;padding:9px 13px;flex:none;' +
           'border-top:1px solid var(--color-divider)',
  },
    h('button', {
      class: 'btn btn-ghost', style: 'font-size:12px;padding-inline:4px', type: 'button',
      onClick: () => guard(async () => {
        await call('open_window', { label: 'main' });
        await currentWindow().hide();
      }),
    }, 'Открыть приложение'),
    refs.hotkey = h('span', {
      style: 'margin-left:auto;font-size:10.5px;color:var(--color-neutral-600)',
    }));

  mount(clear($('#app')), header, searchBox,
    h('div', { class: 'kicker', style: 'padding:8px 13px 4px' }, 'Недавние'),
    refs.list, footer);
}

function drawList() {
  clear(refs.list);

  if (items.length === 0) {
    mount(refs.list, h('div', { class: 'empty', style: 'padding:26px 16px' },
      icon('clock-counter-clockwise'),
      h('div', { class: 'empty-title' }, 'Пока пусто'),
      h('div', { class: 'empty-hint' },
        'Здесь появятся записи, к которым вы обращаетесь чаще всего.')));
    return;
  }

  items.forEach((entry, i) => {
    const first = i === 0;
    mount(refs.list, h('div', {
      class: 'row row-sm',
      style: first ? 'background:color-mix(in srgb,var(--color-text) 4%,transparent)' : null,
    },
      entryIcon(entry, { size: 26, accent: first }),
      h('div', { class: 'row-text' },
        h('div', { class: 'row-title' }, entry.title),
        h('div', { class: 'row-sub' }, entry.subtitle || entry.host || entry.kind_title)),
      h('button', {
        class: `btn btn-icon ${first ? 'btn-primary' : 'btn-ghost'}`,
        style: 'width:26px;height:26px;border-radius:7px', type: 'button',
        title: entry.has_password ? 'Копировать пароль' : 'Копировать логин',
        onClick: () => copy(entry),
      }, icon('copy', { size: 13 }))));
  });
}

async function copy(entry) {
  const field = entry.has_password ? 'password' : 'username';
  const secs = await guard(() => copyField(entry.id, field));
  if (secs === undefined) return;
  toastCopied(entry.has_password ? 'Пароль' : 'Логин', secs);
  // Окно уходит сразу: оно открывалось ради одного действия. Закреплённое
  // остаётся — его для того и закрепляли.
  if (!settings?.tray_pinned) setTimeout(() => currentWindow().hide(), 700);
}

/** Строка «разблокирован · 12 мин» — сколько осталось до автоблокировки. */
async function tickCountdown() {
  const left = await guard(() => lockCountdown(), { silent: true });
  refs.lockLabel.textContent = left === null || left === undefined
    ? 'разблокирован'
    : `разблокирован · ${fmtCountdown(left)}`;
}

// ── загрузка ───────────────────────────────────────────────────────────────

async function load() {
  settings = await getSettings();
  drawPin();
  refs.hotkey.textContent = (settings.hotkey || '')
    .replace(/CmdOrCtrl/gi, 'Ctrl').split('+').map((s) => s.trim()).join(' + ');

  const st = await status();
  if (!st.unlocked) {
    await currentWindow().hide();
    await call('open_window', { label: 'main' });
    return;
  }
  refs.searchLabel.textContent = `Поиск по ${st.entry_count ?? 0} ${plural(st.entry_count ?? 0)}`;

  items = (await guard(() => recent(6), { silent: true })) || [];
  drawList();
  tickCountdown();
}

function plural(n) {
  const abs = Math.abs(n) % 100;
  const last = abs % 10;
  if (abs > 10 && abs < 20) return 'записям';
  if (last === 1) return 'записи';
  return 'записям';
}

window.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') currentWindow().hide();
});

listen('quick-opened', () => load());
listen('entries-changed', () => load());
// То же и здесь: список недавних записей — содержимое закрытого сейфа.
wipeOnLock(() => {
  items = [];
  drawList();
});

listen('vault-locked', () => currentWindow().hide());

build();
trackActivity();
load();
// Обратный отсчёт до блокировки идёт раз в 15 секунд: чаще незачем,
// подпись всё равно округляется до минут.
countdownTimer = setInterval(tickCountdown, 15_000);
