// Общая обвязка окна: своя полоса заголовка, всплывающие сообщения и
// отметки активности. Подключается каждым экраном первой строкой.

import { call, listen, currentWindow, logToFile } from './api.js';
import { h, icon, $, mount } from './ui.js';

/**
 * Собирает полосу заголовка. Системные рамки выключены, поэтому окно
 * тянется за неё саму (`data-tauri-drag-region`), а кнопки — обычные
 * элементы поверх.
 *
 * `buttons` — какие кнопки нужны: 'min', 'max', 'close'. В макете у разных
 * окон разный набор: у окна разблокировки нет разворачивания, у мини-окна
 * нет вообще ничего.
 */
export function titlebar(title, buttons = ['min', 'max', 'close']) {
  const label = currentWindow().label;
  const bar = h('div', { class: 'titlebar', 'data-tauri-drag-region': '' },
    h('span', { class: 'tb-title' }, title));

  const actions = h('div', { class: 'tb-actions' });
  if (buttons.includes('min')) {
    mount(actions, h('button', {
      class: 'tb-btn', title: 'Свернуть', type: 'button',
      onClick: () => call('minimize_window', { label }),
    }, icon('minus')));
  }
  if (buttons.includes('max')) {
    mount(actions, h('button', {
      class: 'tb-btn', title: 'Развернуть', type: 'button',
      onClick: () => call('toggle_maximize', { label }),
    }, icon('square', { size: 12 })));
  }
  if (buttons.includes('close')) {
    mount(actions, h('button', {
      class: 'tb-btn tb-close', title: 'Закрыть', type: 'button',
      onClick: () => call('close_window', { label }),
    }, icon('x')));
  }
  mount(bar, actions);
  return bar;
}

// ── всплывающие сообщения ──────────────────────────────────────────────────

let toastNode = null;
let toastTimer = null;

/**
 * Показывает короткое сообщение внизу окна. Сюда попадают и подтверждения
 * («Пароль скопирован»), и ошибки — на одном месте их не приходится искать.
 */
export function toast(message, { error = false, glyph, ms = 2600 } = {}) {
  if (!toastNode) {
    toastNode = h('div', { class: 'toast' });
    mount(document.body, toastNode);
  }
  toastNode.replaceChildren(
    icon(glyph || (error ? 'warning-circle' : 'check-circle')),
    h('span', {}, message)
  );
  toastNode.classList.toggle('is-error', error);
  toastNode.classList.add('is-open');
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => toastNode.classList.remove('is-open'), ms);
}

/**
 * Оборачивает действие: ошибку показывает сообщением, а не роняет экран.
 * Возвращает результат или `undefined`.
 */
export async function guard(fn, { silent = false } = {}) {
  try {
    return await fn();
  } catch (e) {
    if (!silent) toast(e.message, { error: true });
    return undefined;
  }
}

/**
 * Сообщение о копировании — с обратным отсчётом до очистки буфера, если она
 * включена. Ровно та строка, что в макете 1b.
 */
export function toastCopied(what, secs) {
  if (secs) {
    toast(`${what} скопирован · буфер очистится через ${secs} с`, { glyph: 'clock-countdown' });
  } else {
    toast(`${what} скопирован`, { glyph: 'copy' });
  }
}

// ── активность и блокировка ────────────────────────────────────────────────

/**
 * Раз в несколько секунд сообщает ядру, что пользователь здесь, — на этом
 * держится автоблокировка. Реже, чем на каждое событие: смысл в том, чтобы
 * отличить «работает» от «отошёл», а не считать нажатия.
 */
export function trackActivity() {
  let pending = false;
  const mark = () => {
    if (pending) return;
    pending = true;
    setTimeout(() => {
      pending = false;
      call('ping').catch(() => {});
    }, 3000);
  };
  for (const ev of ['keydown', 'pointerdown', 'pointermove', 'wheel']) {
    window.addEventListener(ev, mark, { passive: true });
  }
}

// ── уборка секретов при блокировке ─────────────────────────────────────────

const wipers = new Set();

/**
 * Регистрирует уборку, которую экран выполняет, когда сейф закрывается.
 *
 * В Rust «заблокировать» — это уничтожить ключ и затереть строки. В разметке
 * так нельзя, и есть отдельная причина, почему уборку нужно звать явно:
 * вспомогательные окна по блокировке только **прячутся**, вебвью продолжает
 * жить. Без уборки открытая карточка так и стоит с паролем в поле ввода —
 * у закрытого сейфа, сколько угодно долго.
 *
 * Затереть память этим не выйдет: строки в JavaScript неизменяемы, `zeroize`
 * здесь не существует. Можно только отпустить ссылки и отдать остальное
 * сборщику мусора — но это разница между «висит в форме» и «лежит мусором
 * до ближайшей сборки».
 */
export function wipeOnLock(fn) {
  wipers.add(fn);
}

listen('vault-locked', () => {
  for (const wipe of [...wipers]) {
    // Одна упавшая уборка не должна отменить остальные: незачищенный экран
    // хуже, чем запись в журнале.
    try {
      wipe();
    } catch (e) {
      logToFile('error', `уборка при блокировке: ${e?.message || e}`);
    }
  }
});

/**
 * Очищает поля ввода внутри узла. Значение поля живёт в разметке само по
 * себе — убрать узел со страницы недостаточно.
 */
export function wipeInputs(root) {
  for (const node of root.querySelectorAll('input, textarea')) node.value = '';
}

/**
 * Реакция на блокировку сейфа. Главное окно показывает экран разблокировки,
 * остальные просто закрываются — держать на экране карточку записи,
 * когда сейф уже закрыт, нельзя.
 */
export function onLocked(handler) {
  listen('vault-locked', (event) => handler(event.payload));
}

/** Закрывает окно по Esc — так ведут себя все вспомогательные окна макета. */
export function closeOnEscape(before) {
  window.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    e.preventDefault();
    if (before && before() === false) return;
    call('close_window', { label: currentWindow().label });
  });
}

/**
 * Готовит вспомогательное окно: заголовок, Esc, активность и авто-закрытие
 * при блокировке сейфа.
 */
export function setupAuxWindow(title, { buttons, onEscape } = {}) {
  const root = $('#app');
  root.prepend(titlebar(title, buttons));
  trackActivity();
  closeOnEscape(onEscape);
  onLocked(() => call('close_window', { label: currentWindow().label }));
  return root;
}
