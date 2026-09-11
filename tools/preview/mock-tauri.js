/* Подставной __TAURI__ для визуальной проверки экранов в обычном браузере.
   Данные — те же, что нарисованы в макете, чтобы скриншоты можно было
   сравнивать со снимком дизайна один в один. В приложение не попадает. */
(function () {
  const P = new URLSearchParams(location.search);
  const MODE = P.get('mock') || 'ok';
  const now = new Date();
  const iso = (d) => new Date(now - d * 86400000).toISOString();

  const mk = (o) => ({
    id: o.id, kind: o.kind || 'password', kind_title: {
      password: 'Пароли', note: 'Заметки', api_key: 'Ключи API', document: 'Документы',
    }[o.kind || 'password'],
    title: o.title, subtitle: o.sub || '', username: o.username || '', email: o.email || '',
    url: o.url || '', host: o.host || '', note: o.note || '', icon: o.icon || 'key',
    tags: o.tags || [], folder: o.folder || null, favorite: !!o.fav,
    quick_access: true, has_password: o.pw !== false, has_totp: !!o.totp,
    password_masked: '••••••••••••••••',
    strength: o.pw === false ? null : { bits: o.bits ?? 96, label: o.label || 'надёжный', fill: (o.bits ?? 96) / 120 },
    custom: o.custom || [], history: o.history || [],
    created_at: iso(540), modified_at: iso(o.mod ?? 22),
    password_modified_at: o.pw === false ? null : iso(118), password_age_days: o.pw === false ? null : 118,
    expires_at: o.exp ? iso(-21) : null, expires_in_days: o.exp ? 21 : null,
    last_used_at: iso(1), usage_count: o.used || 0, deleted: false,
  });

  const ENTRIES = [
    mk({ id: '1', title: 'GitHub', username: 'annakuz', email: 'anna.k@fastmail.com', url: 'https://github.com/login', host: 'github.com', icon: 'github-logo', tags: ['работа', 'разработка'], fav: true, totp: true, used: 42, mod: 22, note: 'Рабочий аккаунт. Коды восстановления — в записи «GitHub · коды восстановления».', history: [{ masked: '••••••••••••', replaced_at: iso(500) }, { masked: '••••••••••', replaced_at: iso(880) }] }),
    mk({ id: '2', title: 'Notion', username: 'anna.k@fastmail.com', url: 'notion.so', host: 'notion.so', icon: 'notion-logo', used: 31, mod: 63, bits: 74, label: 'хороший' }),
    mk({ id: '3', title: 'AWS · production', username: 'AKIA••••••••7Q2F', kind: 'api_key', icon: 'code', used: 18, mod: 37, exp: true, pw: false }),
    mk({ id: '4', title: 'Аэрофлот Бонус', username: '•••• 4471', icon: 'airplane-tilt', mod: 74, bits: 57, label: 'средняя' }),
    mk({ id: '5', title: 'Банковские реквизиты', kind: 'note', sub: 'заметка', icon: 'bank', mod: 30, pw: false }),
    mk({ id: '6', title: 'Google', username: 'anna.kuznetsova@gmail.com', url: 'google.com', host: 'google.com', icon: 'google-logo', mod: 30, bits: 42, label: 'слабый' }),
    mk({ id: '7', title: 'Паспорт РФ', username: '45 08 •• ••••', kind: 'document', icon: 'identification-card', mod: 46, pw: false }),
    mk({ id: '8', title: 'Figma', username: 'anna.k@fastmail.com', url: 'figma.com', host: 'figma.com', icon: 'figma-logo', mod: 65, bits: 88 }),
    mk({ id: '9', title: 'Тинькофф', username: '+7 918 ••• 41 20', email: 'anna.kuznetsova@gmail.com', icon: 'bank', mod: 57, totp: true }),
    mk({ id: '10', title: 'OpenAI · sk-proj', username: 'sk-proj-••••••3Xm', kind: 'api_key', icon: 'code', mod: 81, pw: false }),
  ];

  const SETTINGS = {
    vault_path: '/home/anna/.local/share/seif/seif.vault',
    autolock_secs: 600, clipboard_clear_secs: 30,
    hotkey: 'CmdOrCtrl+Shift+Space', biometrics: false, mask_secrets: true,
    show_key_hints: true, tray_pinned: false,
    main_view: P.get('view') === 'table' ? 'table' : 'panels',
    quick_view: P.get('quick') || 'fields',
    launch_at_startup: false,
    seed_vault_path: '/home/anna/.local/share/seif/wallets.seed',
    seed_autolock_secs: 120,
    seed_require_password_on_reveal: true,
    seed_hide_after_secs: 30,
  };

  // Раздел сид-фраз. `?seed=` задаёт его состояние: locked, empty, new.
  const SEED_MODE = P.get('seed') || 'ok';
  const SEED = [
    { id: 's1', title: 'Ledger основной', wallet: 'Ledger Nano S', network: 'BTC',
      derivation: "m/44'/0'/0'", note: 'Бумажная копия — в банковской ячейке.',
      word_count: 24, has_passphrase: true, standard: true,
      created_at: iso(400), modified_at: iso(400), last_viewed_at: iso(36), view_count: 3 },
    { id: 's2', title: 'Metamask', wallet: 'Metamask', network: 'ETH', derivation: "m/44'/60'/0'",
      note: '', word_count: 12, has_passphrase: false, standard: true,
      created_at: iso(220), modified_at: iso(220), last_viewed_at: null, view_count: 0 },
    { id: 's3', title: 'Monero', wallet: 'Monero GUI', network: 'XMR', derivation: '',
      note: '', word_count: 24, has_passphrase: false, standard: false,
      created_at: iso(90), modified_at: iso(90), last_viewed_at: null, view_count: 0 },
  ];
  const PHRASE = ('legal winner thank year wave sausage worth useful legal winner thank year ' +
    'wave sausage worth useful legal winner thank year wave sausage worth title').split(' ');

  const HANDLERS = {
    status: () => ({
      exists: MODE !== 'new', unlocked: MODE === 'ok', path: SETTINGS.vault_path,
      entry_count: 148, has_recovery_key: true, format_version: 3,
    }),
    get_settings: () => SETTINGS,
    set_settings: (a) => Object.assign(SETTINGS, a.settings),
    list_entries: () => ENTRIES,
    search: () => ENTRIES.slice(0, 3),
    recent: () => ENTRIES.slice(0, 4),
    get_entry: (a) => ENTRIES.find((e) => e.id === a.id) || ENTRIES[0],
    counts: () => ({ total: 148, passwords: 96, notes: 23, api_keys: 18, documents: 11, favorites: 6, trash: 2 }),
    folders: () => [{ id: 'f1', name: 'Работа', count: 34 }, { id: 'f2', name: 'Личное', count: 21 }],
    tags: () => [['работа', 34], ['разработка', 12], ['финансы', 8]],
    audit_report: () => ({
      weak: 3, reused: 3, stale: 1, expiring: 1,
      findings: [
        { entry_id: '6', title: 'Google', issue: 'weak', detail: '42 бит · слабый' },
        { entry_id: '2', title: 'Notion', issue: 'reused', detail: 'тот же пароль есть в другой записи' },
        { entry_id: '3', title: 'AWS · production', issue: 'expiring', detail: 'истекает через 21 дн.' },
        { entry_id: '4', title: 'Аэрофлот Бонус', issue: 'stale', detail: 'не менялся 412 дн.' },
      ],
    }),
    load_draft: (a) => ({
      id: a.id || '1', kind: 'password', title: 'GitHub', username: 'annakuz',
      email: 'anna.k@fastmail.com',
      password: 'k7$Rm2-vQx9Lp!Zt', url: 'https://github.com/login',
      note: 'Рабочий аккаунт. Коды восстановления — в записи «GitHub · коды восстановления».',
      totp: 'otpauth://totp/GitHub:annakuz?secret=••••••••',
      custom: [{ label: 'Ключ восстановления', value: '1234-5678-9012-3456', secret: true }],
      folder: 'f1', tags: ['работа', 'разработка'], favorite: true, quick_access: true,
      icon: 'github-logo', expires_at: null,
    }),
    estimate: () => ({ bits: 96, label: 'надёжный', fill: 0.8 }),
    generate_password: () => ({ value: 'k7$Rm2-vQx9Lp!Zt', entropy_bits: 96, label: 'надёжный', fill: 0.8 }),
    lock_countdown: () => 480,
    backups: () => [
      { name: 'seif-20260903-091200.vault', bytes: 48213, modified: iso(0) },
      { name: 'seif-20260902-183000.vault', bytes: 47980, modified: iso(1) },
    ],
    copy_field: () => 30, copy_text: () => 30, copy_custom_field: () => 30,
    reveal_field: () => 'k7$Rm2-vQx9Lp!Zt',
    ping: () => 600,

    seed_status: () => ({
      configured: SEED_MODE !== 'new',
      exists: SEED_MODE !== 'new',
      unlocked: SEED_MODE === 'ok' || SEED_MODE === 'empty',
      path: SETTINGS.seed_vault_path,
      entry_count: SEED_MODE === 'empty' ? 0 : SEED.length,
      format_version: 1,
      require_password_on_reveal: P.get('nopw') !== '1',
      hide_after_secs: 30,
      autolock_secs: 120,
      min_password_len: 12,
    }),
    seed_list: () => (SEED_MODE === 'empty' ? [] : SEED),
    seed_get: (a) => SEED.find((e) => e.id === a.id) || SEED[0],
    seed_countdown: () => 95,
    seed_ping: () => 120,
    seed_lock: () => null,
    seed_reveal: (a) => ({
      words: PHRASE.slice(0, (SEED.find((e) => e.id === a.id) || SEED[0]).word_count),
      hide_after_secs: 30,
    }),
    seed_reveal_passphrase: () => 'дополнительное-слово',
    seed_verify_phrase: () => true,
    seed_clear_clipboard: () => null,
    bip39_suggest: (a) => ['abandon', 'ability', 'able', 'about', 'above', 'absent']
      .filter((w) => w.startsWith((a.prefix || '').toLowerCase())),
    bip39_check: () => null,
  };

  // Неизвестная команда в подставном мосте — это забытый обработчик, а не
  // пустой ответ: молчаливый null потом ищется по всему экрану.
  const invoke = (cmd, args = {}) => {
    if (!(cmd in HANDLERS)) {
      console.error(`[mock] нет обработчика команды «${cmd}»`);
      return Promise.resolve(null);
    }
    return Promise.resolve(HANDLERS[cmd](args));
  };

  // Снимок экрана, которому нужен не первый экран. `?click=Сид-фразы` нажимает
  // кнопку или ссылку с таким текстом (или с такой подсказкой) — несколько
  // подряд через «|», с паузой на перерисовку между ними.
  const CLICK = P.get('click');
  if (CLICK) {
    const steps = CLICK.split('|');
    window.addEventListener('load', () => {
      steps.forEach((label, i) => setTimeout(() => {
        const hit = [...document.querySelectorAll('button, a')].find(
          (n) => n.textContent.trim() === label || n.title === label);
        if (hit) hit.click();
        else console.error(`[mock] не нашёл, на что нажать: «${label}»`);
      }, 300 + i * 350));
    });
  }

  window.__TAURI__ = {
    core: { invoke },
    event: { listen: () => Promise.resolve(() => {}), emit: () => Promise.resolve() },
    window: {
      getCurrentWindow: () => ({
        label: P.get('label') || 'main',
        hide: async () => {}, show: async () => {}, setFocus: async () => {},
        minimize: async () => {}, startDragging: async () => {},
      }),
    },
  };
})();
