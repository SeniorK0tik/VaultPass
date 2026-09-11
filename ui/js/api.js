// Тонкая обёртка над IPC. Всё общение интерфейса с ядром идёт отсюда,
// поэтому обработку ошибок и нормализацию достаточно написать один раз.

const tauri = window.__TAURI__;

/**
 * Пишет строку в общий журнал приложения.
 *
 * Аргументы команд сюда не попадают намеренно: в них бывают пароли.
 * В журнал идут только имя команды, код ошибки и её текст.
 */
export function logToFile(level, message) {
  // Через сырой invoke, а не через `call`: если сломается сама запись в
  // журнал, попытка записать это в журнал зациклилась бы.
  tauri.core.invoke('log_ui', { level, message: String(message) }).catch(() => {});
}

/**
 * Вызывает команду Rust.
 *
 * Ядро возвращает ошибки в виде `{ code, message }`. Иногда наружу
 * прорывается и обычная строка (например, из плагина), поэтому здесь всё
 * приводится к одной форме — вызывающему коду не приходится гадать.
 *
 * Неудачу заодно видно в журнале: ошибку могли показать всплывающим
 * сообщением, которое пользователь не успел прочитать, а разбираться потом.
 */
export async function call(cmd, args = {}) {
  try {
    return await tauri.core.invoke(cmd, args);
  } catch (raw) {
    const err = new Error(
      typeof raw === 'string' ? raw : raw?.message || 'Что-то пошло не так.'
    );
    err.code = typeof raw === 'object' && raw ? raw.code || 'other' : 'other';
    // Отказ уже записан в журнал на стороне Rust; здесь добавляется главное,
    // чего там нет, — какая команда его вызвала.
    logToFile(err.code === 'locked' ? 'info' : 'error',
      `${window.location.pathname.replace(/^\/|\.html$/g, '') || 'main'}: ${cmd} → [${err.code}] ${err.message}`);
    throw err;
  }
}

/**
 * Перехват необработанных ошибок разметки.
 *
 * Без него исключение в обработчике события уходит в консоль вебвью, куда
 * пользователь не заглядывает: на экране просто ничего не происходит.
 * Теперь такая ошибка попадает в тот же файл, что и всё остальное.
 */
function installGlobalErrorCapture() {
  const where = window.location.pathname.replace(/^\/|\.html$/g, '') || 'main';

  window.addEventListener('error', (e) => {
    const at = e.filename ? ` (${e.filename.split('/').pop()}:${e.lineno}:${e.colno})` : '';
    logToFile('error', `${where}: необработанная ошибка: ${e.message}${at}\n${e.error?.stack || ''}`);
  });

  window.addEventListener('unhandledrejection', (e) => {
    const r = e.reason;
    logToFile('error',
      `${where}: необработанный отказ промиса: ${r?.message || r}\n${r?.stack || ''}`);
  });
}

installGlobalErrorCapture();

export const listen = (event, handler) => tauri.event.listen(event, handler);
export const currentWindow = () => tauri.window.getCurrentWindow();

// ── команды, которыми пользуются несколько экранов ──────────────────────────

export const status = () => call('status');
export const lockVault = () => call('lock_vault');
export const getSettings = () => call('get_settings');
export const setSettings = (settings) => call('set_settings', { settings });

export const listEntries = (opts = {}) => call('list_entries', opts);
export const getEntry = (id) => call('get_entry', { id });
export const search = (query, limit) => call('search', { query, limit });
export const recent = (limit) => call('recent', { limit });
export const counts = () => call('counts');
export const folders = () => call('folders');
export const tags = () => call('tags');
export const auditReport = () => call('audit_report');

export const revealField = (id, field) => call('reveal_field', { id, field });
export const copyField = (id, field) => call('copy_field', { id, field });
export const copyCustomField = (id, fieldId) =>
  call('copy_custom_field', { id, fieldId });
export const revealCustomField = (id, fieldId) =>
  call('reveal_custom_field', { id, fieldId });
export const copyText = (text) => call('copy_text', { text });
export const quickCopy = (id) => call('quick_copy', { id });
export const autofillEntry = (id, submit = false) =>
  call('autofill_entry', { id, submit });
export const openEntryUrl = (id) => call('open_entry_url', { id });

export const loadDraft = (id = null) => call('load_draft', { id });
export const saveDraft = (draft) => call('save_draft', { draft });
export const deleteEntry = (id) => call('delete_entry', { id });
export const restoreEntry = (id) => call('restore_entry', { id });
export const purgeEntry = (id) => call('purge_entry', { id });
export const emptyTrash = () => call('empty_trash');
export const toggleFavorite = (id) => call('toggle_favorite', { id });

export const generatePassword = (options) => call('generate_password', { options });
export const estimate = (password) => call('estimate', { password });

export const openWindow = (label) => call('open_window', { label });
export const openEditor = (id = null) => call('open_editor', { id });
export const lockCountdown = () => call('lock_countdown');

export const logInfo = () => call('log_info');
export const logTail = (lines) => call('log_tail', { lines });
export const openLogDir = () => call('open_log_dir');

// ── раздел сид-фраз ─────────────────────────────────────────────────────────
//
// Команды, которая копировала бы сид-фразу, здесь нет намеренно: в ядре её
// тоже не существует. Наверх фраза приходит одной `seedReveal`, и живёт в
// разметке считанные секунды.

export const seedStatus = () => call('seed_status');
export const seedPickFile = (mode) => call('seed_pick_file', { mode });
export const seedForget = () => call('seed_forget');
export const seedCreate = (masterPassword) => call('seed_create', { masterPassword });
export const seedUnlock = (masterPassword) => call('seed_unlock', { masterPassword });
export const seedLock = () => call('seed_lock');
export const seedChangePassword = (current, next) =>
  call('seed_change_password', { current, new: next });
export const seedPing = () => call('seed_ping');
export const seedCountdown = () => call('seed_countdown');

export const seedList = () => call('seed_list');
export const seedGet = (id) => call('seed_get', { id });
export const seedAdd = (draft) => call('seed_add', { draft });
export const seedUpdateDetails = (id, details) => call('seed_update_details', { id, details });
export const seedReplacePhrase = (id, words, passphrase, standard) =>
  call('seed_replace_phrase', { id, words, passphrase, standard });
export const seedDelete = (id, confirmTitle) => call('seed_delete', { id, confirmTitle });

export const seedReveal = (id, masterPassword) => call('seed_reveal', { id, masterPassword });
export const seedRevealPassphrase = (id, masterPassword) =>
  call('seed_reveal_passphrase', { id, masterPassword });
export const seedVerifyPhrase = (id, words) => call('seed_verify_phrase', { id, words });
export const seedClearClipboard = () => call('seed_clear_clipboard');

export const bip39Suggest = (prefix, limit) => call('bip39_suggest', { prefix, limit });
export const bip39Check = (words) => call('bip39_check', { words });
