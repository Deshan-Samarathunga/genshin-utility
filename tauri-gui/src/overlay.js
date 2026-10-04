const { listen } = window.__TAURI__.event;

const bubble = document.getElementById('bubble');
const textEl = document.getElementById('text');
const messageEl = document.getElementById('message');

// States that stay on screen until the next state change; the rest fade out on their own.
const STICKY = new Set(['listening', 'transcribing', 'loading']);
const HIDDEN = new Set(['disabled', 'idle']);
let hideTimer = null;

listen('voice-status', ({ payload }) => {
  clearTimeout(hideTimer);
  if (HIDDEN.has(payload.state) && !payload.message) {
    bubble.classList.remove('visible');
    return;
  }

  bubble.dataset.state = payload.state;
  textEl.textContent = payload.text ? `“${payload.text}”` : '';
  messageEl.textContent = payload.message;
  bubble.classList.add('visible');

  if (!STICKY.has(payload.state)) {
    hideTimer = setTimeout(() => bubble.classList.remove('visible'), payload.state === 'error' ? 5000 : 2500);
  }
});
