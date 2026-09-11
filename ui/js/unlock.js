// Экран 1f — разблокировка мастер-паролем.
//
// Тот же экран служит и созданием сейфа при первом запуске: разница только
// в подписях и во втором поле для подтверждения. Разводить два почти
// одинаковых экрана было бы дороже, чем один флаг.

import { call, status } from './api.js';
import { h, icon, clear, mount, nextPaint } from './ui.js';

export async function renderUnlock(root, onUnlocked) {
  const st = await status();
  const creating = !st.exists;
  const refs = {};

  refs.error = h('div', {
    style: 'font-size:11.5px;color:#e0808f;min-height:17px;margin-top:8px;align-self:flex-start',
  });

  refs.password = h('input', {
    class: 'input mono', id: 'mp', type: 'password',
    style: 'padding-right:38px;letter-spacing:.18em',
    autocomplete: 'off', autofocus: true,
    onKeyDown: (e) => { if (e.key === 'Enter') submit(); },
    onInput: () => { refs.error.textContent = ''; },
  });

  refs.confirm = creating ? h('input', {
    class: 'input mono', type: 'password',
    style: 'letter-spacing:.18em',
    autocomplete: 'off',
    onKeyDown: (e) => { if (e.key === 'Enter') submit(); },
  }) : null;

  const eye = h('button', {
    type: 'button', title: 'Показать пароль',
    style: 'position:absolute;right:8px;top:6px;width:24px;height:24px;border:0;background:transparent;' +
           'color:var(--color-neutral-500);cursor:pointer;display:grid;place-items:center',
    onClick: () => {
      const shown = refs.password.type === 'text';
      refs.password.type = shown ? 'password' : 'text';
      refs.password.style.letterSpacing = shown ? '.18em' : 'normal';
      eye.replaceChildren(icon(shown ? 'eye' : 'eye-slash', { size: 16 }));
    },
  }, icon('eye', { size: 16 }));

  refs.submit = h('button', {
    class: 'btn btn-primary btn-block', style: 'margin-top:14px;height:40px', type: 'button',
    onClick: submit,
  }, creating ? 'Создать сейф' : 'Разблокировать');

  async function submit() {
    const value = refs.password.value;
    if (!value) { refs.error.textContent = 'Введите мастер-пароль.'; return; }
    if (creating && value !== refs.confirm.value) {
      refs.error.textContent = 'Пароли не совпадают.';
      return;
    }

    refs.submit.disabled = true;
    refs.submit.textContent = creating ? 'Создаём…' : 'Открываем…';
    try {
      // Argon2id на 64 МиБ занимает заметную долю секунды — интерфейс
      // должен успеть перерисоваться до того, как поток встанет.
      await nextPaint();
      await call(creating ? 'create_vault' : 'unlock', { masterPassword: value });
      refs.password.value = '';
      if (refs.confirm) refs.confirm.value = '';
      onUnlocked();
    } catch (e) {
      refs.error.textContent = e.message;
      refs.submit.disabled = false;
      refs.submit.textContent = creating ? 'Создать сейф' : 'Разблокировать';
      refs.password.select();
    }
  }

  async function useRecovery() {
    const key = prompt('Ключ восстановления\n\nВведите его целиком — дефисы и регистр не важны.');
    if (!key) return;
    try {
      await call('unlock_with_recovery', { recoveryKey: key.trim() });
      onUnlocked();
    } catch (e) {
      refs.error.textContent = e.message;
    }
  }

  // Число записей на этом экране не показывается намеренно: пока сейф закрыт,
  // оно неизвестно и самой программе — снаружи файл не выдаёт даже своего объёма.
  const subtitle = creating
    ? 'Новое хранилище · шифруется на этом устройстве'
    : 'Локальное хранилище · ключ выводится из мастер-пароля';

  mount(clear(root), h('div', {
    class: 'col',
    style: 'flex:1;padding:52px 42px 32px;align-items:flex-start;max-width:460px;margin:0 auto;width:100%',
  },
    h('div', {
      style: 'width:56px;height:56px;border-radius:14px;border:1px solid var(--color-accent);' +
             'display:grid;place-items:center;color:var(--color-accent);' +
             'box-shadow:0 0 32px color-mix(in srgb,var(--color-accent) 22%,transparent)',
    }, icon('lock-key', { size: 27 })),

    h('h2', { style: 'margin:22px 0 4px' }, 'Сейф'),
    h('div', { style: 'font-size:13px;color:var(--color-neutral-500);margin-bottom:26px' }, subtitle),

    h('div', { class: 'field', style: 'width:100%' },
      h('label', { for: 'mp' }, creating ? 'Придумайте мастер-пароль' : 'Мастер-пароль'),
      h('div', { style: 'position:relative' }, refs.password, eye)),

    creating ? h('div', { class: 'field', style: 'width:100%;margin-top:10px' },
      h('label', {}, 'Повторите'), refs.confirm) : null,

    creating ? h('div', {
      style: 'font-size:11.5px;color:var(--color-neutral-600);margin-top:8px;line-height:1.5',
    }, 'Мастер-пароль нельзя восстановить: он нигде не хранится, из него выводится ключ шифрования. Забыть его — значит потерять сейф.') : null,

    refs.error,
    refs.submit,

    creating ? null : h('button', {
      class: 'btn btn-secondary btn-block', style: 'margin-top:8px;height:40px', type: 'button',
      onClick: () => {
        refs.error.textContent =
          'Вход по отпечатку появится в следующей версии — пока только мастер-пароль.';
      },
    }, icon('fingerprint', { size: 17 }), 'Войти по отпечатку'),

    h('div', { class: 'rule', style: 'width:100%;margin:26px 0 14px' }),

    h('div', {
      style: 'display:flex;align-items:center;gap:8px;font-size:11.5px;color:var(--color-neutral-600)',
    },
      icon('shield-check', { size: 15, color: 'var(--color-accent)' }),
      h('span', {}, 'Argon2id + XChaCha20-Poly1305 · ключ не покидает устройство')),

    creating ? null : h('button', {
      class: 'btn btn-ghost', style: 'font-size:11.5px;margin-top:9px;padding-inline:0', type: 'button',
      onClick: useRecovery,
    }, 'Забыли мастер-пароль? Восстановление по ключу')));

  refs.password.focus();
}
