// Мелкие помощники разметки. Ни фреймворка, ни шага сборки: экраны собраны
// из тех же классов, что и макет, поэтому дешевле строить узлы напрямую,
// чем тащить рантайм ради девяти окон.

/**
 * Создаёт элемент. `props` кладутся как свойства DOM, кроме `class`,
 * `style`, `dataset` и `on*` — их приходится ставить особым образом.
 */
export function h(tag, props = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(props)) {
    if (value === null || value === undefined || value === false) continue;
    if (key === 'class') node.className = value;
    else if (key === 'style') node.setAttribute('style', value);
    else if (key === 'dataset') Object.assign(node.dataset, value);
    else if (key.startsWith('on') && typeof value === 'function') {
      node.addEventListener(key.slice(2).toLowerCase(), value);
    } else if (key in node) node[key] = value;
    else node.setAttribute(key, value);
  }
  mount(node, ...children);
  return node;
}

/**
 * Добавляет детей в узел так же, как это делает `h`: массивы разворачиваются,
 * `null` и `false` пропускаются, остальное превращается в текст.
 *
 * Именно за этим она и нужна: у родного `append` массив превращается в
 * `[object HTMLButtonElement]`, а `null` — в слово «null», причём молча.
 */
export function mount(parent, ...children) {
  for (const child of children.flat(Infinity)) {
    if (child === null || child === undefined || child === false) continue;
    parent.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return parent;
}

/** Иконка Phosphor. `fill: true` берёт заливной начерк. */
export function icon(name, { fill = false, size, color, cls = '' } = {}) {
  const style = [size && `font-size:${size}px`, color && `color:${color}`]
    .filter(Boolean)
    .join(';');
  return h('i', {
    class: `${fill ? 'ph-fill ph-' : 'ph ph-'}${name} ${cls}`.trim(),
    style: style || null,
  });
}

export const $ = (sel, root = document) => root.querySelector(sel);
export const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

export function clear(node) {
  node.replaceChildren();
  return node;
}

// ── формат дат и чисел ─────────────────────────────────────────────────────

const MONTHS = ['января', 'февраля', 'марта', 'апреля', 'мая', 'июня',
                'июля', 'августа', 'сентября', 'октября', 'ноября', 'декабря'];
const MONTHS_SHORT = ['янв', 'фев', 'мар', 'апр', 'мая', 'июн',
                      'июл', 'авг', 'сен', 'окт', 'ноя', 'дек'];

/** «12 августа», а для прошлых лет — «12 августа 2024». */
export function fmtDate(iso) {
  if (!iso) return '—';
  const d = new Date(iso);
  const now = new Date();
  const tail = d.getFullYear() === now.getFullYear() ? '' : ` ${d.getFullYear()}`;
  return `${d.getDate()} ${MONTHS[d.getMonth()]}${tail}`;
}

/** «12 авг» — для узкого столбца таблицы. */
export function fmtDateShort(iso) {
  if (!iso) return '—';
  const d = new Date(iso);
  return `${d.getDate()} ${MONTHS_SHORT[d.getMonth()]}`;
}

/**
 * Правильное окончание русского существительного при числе.
 * `plural(21, 'день', 'дня', 'дней')` → «день».
 */
export function plural(n, one, few, many) {
  const abs = Math.abs(n) % 100;
  const last = abs % 10;
  if (abs > 10 && abs < 20) return many;
  if (last > 1 && last < 5) return few;
  if (last === 1) return one;
  return many;
}

export function fmtDays(days) {
  if (days === null || days === undefined) return '—';
  const n = Math.abs(Math.round(days));
  return `${n} ${plural(n, 'день', 'дня', 'дней')}`;
}

/** «обновлён 24 дня назад» / «обновлён сегодня». */
export function fmtAgo(iso) {
  if (!iso) return '—';
  const days = Math.floor((Date.now() - new Date(iso)) / 86_400_000);
  if (days <= 0) return 'сегодня';
  if (days === 1) return 'вчера';
  return `${fmtDays(days)} назад`;
}

/** «8 мин» / «0:19» — для обратных отсчётов. */
export function fmtCountdown(secs) {
  if (secs === null || secs === undefined) return '—';
  if (secs >= 90) {
    const m = Math.round(secs / 60);
    return `${m} мин`;
  }
  const m = Math.floor(secs / 60);
  const s = String(secs % 60).padStart(2, '0');
  return `${m}:${s}`;
}

export function fmtBytes(n) {
  if (n < 1024) return `${n} Б`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} КБ`;
  return `${(n / 1024 / 1024).toFixed(1)} МБ`;
}

// ── строительные блоки, повторяющиеся в макете ─────────────────────────────

/** Полоса надёжности с подписью. */
export function meter(strength, { small = false } = {}) {
  if (!strength) return null;
  const weak = strength.bits < 60;
  return h('span', { class: 'meter-row' },
    h('span', { class: `meter${small ? ' meter-sm' : ''}${weak ? ' is-weak' : ''}` },
      h('span', { style: `width:${Math.round(strength.fill * 100)}%` })),
    h('span', {
      class: weak ? 'tag tag-accent' : '',
      style: weak ? null : 'font-size:11.5px;color:var(--color-neutral-400)',
    }, strength.label));
}

/** Кружок-иконка записи, как в списках макета. */
export function entryIcon(entry, { size = 30, accent = false } = {}) {
  const radius = size >= 40 ? 12 : size >= 30 ? 8 : 7;
  const glyph = Math.round(size * 0.5);
  return h('div', {
    class: `row-icon${accent ? ' is-accent' : ''}`,
    style: `width:${size}px;height:${size}px;border-radius:${radius}px;font-size:${glyph}px`,
  }, icon(entry.icon));
}

/** Кольцевой таймер TOTP: сколько осталось от 30-секундного окна. */
export function totpRing(size = 24) {
  const r = size / 2 - 2;
  const c = 2 * Math.PI * r;
  const left = 30 - (Math.floor(Date.now() / 1000) % 30);
  const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
  svg.setAttribute('width', size);
  svg.setAttribute('height', size);
  svg.setAttribute('viewBox', `0 0 ${size} ${size}`);
  svg.setAttribute('class', 'totp-ring');
  svg.innerHTML =
    `<circle class="bg" cx="${size / 2}" cy="${size / 2}" r="${r}"></circle>` +
    `<circle class="fg" cx="${size / 2}" cy="${size / 2}" r="${r}" ` +
    `stroke-dasharray="${c.toFixed(1)}" stroke-dashoffset="${(c * (1 - left / 30)).toFixed(1)}"></circle>`;
  return svg;
}
