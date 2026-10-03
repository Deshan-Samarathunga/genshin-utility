const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const statusBar = document.getElementById('status-bar');
const statusPill = document.getElementById('status-pill');

const cooldowns = new Map();
const COOLDOWN_MS = 4000;

function setStatus(msg) {
  if (!statusBar) return;
  statusBar.textContent = msg;
  // Long messages are cut off with "…"; the full text shows on hover.
  statusPill.title = msg;
  statusPill.classList.toggle('error', /error/i.test(msg));
  setTimeout(() => {
    if (statusBar.textContent === msg) {
      statusBar.textContent = 'Ready';
      statusPill.title = '';
      statusPill.classList.remove('error');
    }
  }, 3000);
}

async function updateToggleState(checkbox) {
  const scriptName = checkbox.getAttribute('data-script');

  const cooldownUntil = cooldowns.get(scriptName) || 0;
  if (Date.now() < cooldownUntil) return;

  try {
    const isRunning = await invoke('check_status', { scriptName });
    if (checkbox.checked !== isRunning) {
      checkbox.checked = isRunning;
    }
    updateTabDot(scriptName, isRunning);
  } catch (error) {
    console.error(`Failed to check status for ${scriptName}:`, error);
  }
}

async function handleToggle(event) {
  const checkbox = event.target;
  const scriptName = checkbox.getAttribute('data-script');
  
  checkbox.disabled = true;
  cooldowns.set(scriptName, Date.now() + COOLDOWN_MS);
  
  if (checkbox.checked) {
    try {
      let args = null;
      if (scriptName === 'auto_message') {
        const text = document.getElementById('auto-message-text').value;
        const count = document.getElementById('auto-message-count').value;
        args = [text, count];
      }
      
      const msg = await invoke('start_script', { scriptName, args });
      setStatus(msg);
    } catch (error) {
      checkbox.checked = false;
      setStatus(`Error: ${error}`);
    }
  } else {
    try {
      const msg = await invoke('stop_script', { scriptName });
      setStatus(msg);
    } catch (error) {
      checkbox.checked = true;
      setStatus(`Error: ${error}`);
    }
  }
  
  checkbox.disabled = false;
  updateTabDot(scriptName, checkbox.checked);
}

// ── Scale to window size ─────────────────────────────────────

// The layout is designed at this size; larger or smaller windows zoom the whole page uniformly.
const DESIGN_WIDTH = 900;
const DESIGN_HEIGHT = 680;
const MIN_ZOOM = 0.8;
const MAX_ZOOM = 1.8;

async function initScaling() {
  const { window: tauriWindow, webview: tauriWebview } = window.__TAURI__;
  if (!tauriWindow || !tauriWebview) return;
  const appWindow = tauriWindow.getCurrentWindow();
  const webview = tauriWebview.getCurrentWebview();
  let current = 0;

  // Uses the window's real size (not the page's), so changing the zoom can't feed back into itself.
  const apply = async () => {
    const [size, factor] = await Promise.all([appWindow.innerSize(), appWindow.scaleFactor()]);
    if (!size.width || !size.height) return; // minimized
    const fit = Math.min(size.width / factor / DESIGN_WIDTH, size.height / factor / DESIGN_HEIGHT);
    const zoom = Math.round(Math.min(Math.max(fit, MIN_ZOOM), MAX_ZOOM) * 100) / 100;
    if (zoom !== current) {
      current = zoom;
      await webview.setZoom(zoom);
    }
  };

  let pending = null;
  await appWindow.onResized(() => {
    clearTimeout(pending);
    pending = setTimeout(() => apply().catch(console.error), 50);
  });
  await apply();
}

// ── Sidebar tabs ─────────────────────────────────────────────

const TAB_KEY = 'genshin-utility.tab';

function selectTab(tab) {
  document.querySelectorAll('.tab-btn').forEach((btn) => {
    const active = btn.dataset.tab === tab;
    btn.classList.toggle('active', active);
    btn.setAttribute('aria-selected', String(active));
  });
  document.querySelectorAll('.tab-panel').forEach((panel) => {
    panel.hidden = panel.dataset.tab !== tab;
  });
  localStorage.setItem(TAB_KEY, tab);
}

function updateTabDot(scriptName, on) {
  const dot = document.querySelector(`.tab-dot[data-for="${scriptName}"]`);
  if (dot) dot.classList.toggle('on', on);
}

async function initAutoOpen() {
  const toggle = document.getElementById('toggle-auto-open');
  if (!toggle) return;
  try {
    toggle.checked = await invoke('get_auto_open');
  } catch (error) {
    console.error('Failed to read auto-open state:', error);
  }
  toggle.addEventListener('change', async () => {
    toggle.disabled = true;
    try {
      toggle.checked = await invoke('set_auto_open', { enabled: toggle.checked });
      setStatus(toggle.checked ? 'Will open with Genshin (closing now hides to tray)' : 'Open with Genshin turned off');
    } catch (error) {
      toggle.checked = !toggle.checked;
      setStatus(`Error: ${error}`);
    }
    toggle.disabled = false;
  });
}

function initTabs() {
  const buttons = document.querySelectorAll('.tab-btn');
  buttons.forEach((btn) => btn.addEventListener('click', () => selectTab(btn.dataset.tab)));
  const saved = localStorage.getItem(TAB_KEY);
  const exists = [...buttons].some((btn) => btn.dataset.tab === saved);
  selectTab(exists ? saved : buttons[0].dataset.tab);
}

document.addEventListener('DOMContentLoaded', () => {
  initScaling().catch((error) => console.error('Window scaling unavailable:', error));
  initTabs();
  initAutoOpen();

  const checkboxes = document.querySelectorAll('input[type="checkbox"][data-script]');
  checkboxes.forEach(checkbox => {
    updateToggleState(checkbox);
    checkbox.addEventListener('change', handleToggle);
  });
  
  setInterval(() => {
    checkboxes.forEach(updateToggleState);
  }, 2000);
  
  const speedInput = document.getElementById('dialogue-speed');
  if (speedInput) {
    speedInput.addEventListener('change', async (e) => {
      const speed = parseInt(e.target.value, 10);
      if (speed >= 50) {
        try {
          await invoke('set_dialogue_speed', { speed });
          setStatus(`Speed set to ${speed}ms`);
        } catch (error) {
          setStatus(`Error: ${error}`);
        }
      }
    });
  }

  initVoiceChat();
});

// ── Voice Chat (PS5) ─────────────────────────────────────────

const voiceFields = {
  engine: 'voice-engine',
  mic_name: 'voice-mic',
  language: 'voice-language',
  local_model: 'voice-model',
  vocabulary: 'voice-vocabulary',
  replacements: 'voice-replacements',
};

const PROVIDER_INFO = {
  groq: {
    label: 'Groq',
    hint: 'Free: about 2,000 messages/day, 20/min. Whisper large-v3, very fast.',
    url: 'https://console.groq.com/keys',
    placeholder: 'gsk_...',
  },
  gemini: {
    label: 'Google Gemini',
    hint: 'Free tier from Google AI Studio (limits per model shown in AI Studio). Free-tier data may be used by Google to improve its models.',
    url: 'https://aistudio.google.com/apikey',
    placeholder: 'AIza...',
  },
  deepgram: {
    label: 'Deepgram',
    hint: '$200 free credit on sign-up, no card needed. Nova-3 is very fast and uses your names list as key terms.',
    url: 'https://console.deepgram.com/',
    placeholder: 'Deepgram API key',
  },
  elevenlabs: {
    label: 'ElevenLabs',
    hint: 'Free plan includes monthly credits usable for Scribe speech-to-text.',
    url: 'https://elevenlabs.io/app/settings/api-keys',
    placeholder: 'sk_...',
  },
  mistral: {
    label: 'Mistral',
    hint: 'Free Experiment plan (phone verification, low rate limits; requests may be used for training). Uses your names list for spelling.',
    url: 'https://console.mistral.ai/api-keys',
    placeholder: 'Mistral API key',
  },
  openai: {
    label: 'OpenAI',
    hint: 'Paid per minute of audio. gpt-4o-transcribe is the most accurate, whisper-1 is cheaper.',
    url: 'https://platform.openai.com/api-keys',
    placeholder: 'sk-...',
  },
  custom: {
    label: 'Custom',
    hint: 'Any OpenAI-compatible /audio/transcriptions server. Set the base URL ending in /v1.',
    url: null,
    placeholder: 'API key',
  },
};

const MODEL_LABELS = {
  'base.en': 'base.en — fastest (~140 MB)',
  'small.en-q5_1': 'small.en q5 — balanced (~190 MB)',
  'small.en': 'small.en — accurate (~470 MB)',
  'medium.en-q5_0': 'medium.en q5 — very accurate (~515 MB, GPU)',
  'large-v3-turbo-q5_0': 'large-v3-turbo q5 — best (~550 MB, GPU)',
};

let voiceSettings = null;

function voiceEl(id) {
  return document.getElementById(id);
}

function readVoiceForm() {
  const next = { ...voiceSettings, cloud: { ...voiceSettings.cloud } };
  for (const [key, id] of Object.entries(voiceFields)) {
    next[key] = voiceEl(id).value;
  }
  next.local_gpu = voiceEl('voice-gpu').checked;
  // Every provider's row in the keys table.
  document.querySelectorAll('#voice-provider-rows input[data-field]').forEach((input) => {
    const id = input.dataset.provider;
    next.cloud[id] = { ...next.cloud[id], [input.dataset.field]: input.value.trim() };
  });
  return next;
}

function keyLink(url, text) {
  const link = document.createElement('a');
  link.href = '#';
  link.textContent = text;
  link.addEventListener('click', (e) => {
    e.preventDefault();
    window.__TAURI__.opener.openUrl(url);
  });
  return link;
}

// Free-tier note for the selected provider.
function showProviderHint(engine) {
  if (engine === 'local') return;
  const info = PROVIDER_INFO[engine] || PROVIDER_INFO.custom;
  const hint = voiceEl('voice-provider-hint');
  hint.textContent = `${info.hint} `;
  if (info.url) hint.appendChild(keyLink(info.url, 'Get a key →'));
}

function useProvider(id) {
  const select = voiceEl('voice-engine');
  select.value = id;
  select.dispatchEvent(new Event('change'));
}

// Builds one editable row per cloud provider: key (hidden by default), model, base URL.
function renderProviderTable() {
  const tbody = voiceEl('voice-provider-rows');
  tbody.innerHTML = '';
  for (const [id, info] of Object.entries(PROVIDER_INFO)) {
    const config = voiceSettings.cloud[id] || {};
    const row = document.createElement('tr');
    row.dataset.provider = id;

    const nameCell = document.createElement('td');
    nameCell.className = 'voice-provider-name';
    const name = document.createElement('strong');
    name.textContent = info.label;
    const status = document.createElement('span');
    status.className = 'voice-key-status';
    nameCell.append(name, status);
    if (info.url) nameCell.append(keyLink(info.url, 'Get key'));
    row.append(nameCell);

    const field = (key, type, placeholder) => {
      const cell = document.createElement('td');
      const input = document.createElement('input');
      input.type = type;
      input.value = config[key] || '';
      input.placeholder = placeholder;
      input.spellcheck = false;
      input.autocomplete = 'off';
      input.dataset.provider = id;
      input.dataset.field = key;
      input.addEventListener('change', saveVoiceSettings);
      cell.append(input);
      return { cell, input };
    };

    const key = field('api_key', 'password', info.placeholder);
    const toggle = document.createElement('button');
    toggle.type = 'button';
    toggle.className = 'voice-reveal';
    toggle.textContent = 'Show';
    toggle.addEventListener('click', () => {
      const hidden = key.input.type === 'password';
      key.input.type = hidden ? 'text' : 'password';
      toggle.textContent = hidden ? 'Hide' : 'Show';
    });
    key.cell.classList.add('voice-key-cell');
    key.cell.append(toggle);
    // Model and base URL share one stacked cell to keep the table narrow.
    const endpoint = field('model', 'text', 'model');
    endpoint.cell.classList.add('voice-endpoint-cell');
    endpoint.cell.append(field('base_url', 'text', 'https://.../v1').input);
    row.append(key.cell, endpoint.cell);

    const actionCell = document.createElement('td');
    const use = document.createElement('button');
    use.type = 'button';
    use.className = 'voice-use';
    use.textContent = 'Use';
    use.addEventListener('click', () => useProvider(id));
    actionCell.append(use);
    row.append(actionCell);

    tbody.append(row);
  }
  updateProviderTable();
}

// Refreshes saved/active state and backend-filled defaults without rebuilding inputs (keeps focus).
function updateProviderTable() {
  document.querySelectorAll('#voice-provider-rows tr').forEach((row) => {
    const id = row.dataset.provider;
    const config = voiceSettings.cloud[id] || {};
    const active = voiceSettings.engine === id;
    row.classList.toggle('active', active);
    row.querySelector('.voice-key-status').textContent = config.api_key ? 'Saved' : 'No key';
    row.querySelector('.voice-key-status').classList.toggle('saved', Boolean(config.api_key));
    const use = row.querySelector('.voice-use');
    use.textContent = active ? 'In use' : 'Use';
    use.disabled = active;
    row.querySelectorAll('input[data-field]').forEach((input) => {
      if (!input.value && config[input.dataset.field]) input.value = config[input.dataset.field];
    });
  });
}

function showEngineSections() {
  const cloud = voiceEl('voice-engine').value !== 'local';
  document.querySelector('.voice-local').hidden = cloud;
  document.querySelector('.voice-cloud').hidden = !cloud;
}

async function saveVoiceSettings() {
  voiceSettings = readVoiceForm();
  showEngineSections();
  try {
    await invoke('save_voice_settings', { settings: voiceSettings });
    // Pick up defaults the backend filled in (e.g. a provider's default model).
    voiceSettings = await invoke('get_voice_settings');
    updateProviderTable();
    await refreshEngineStatus();
  } catch (error) {
    setStatus(`Error: ${error}`);
  }
}

async function refreshEngineStatus() {
  const status = await invoke('voice_engine_status');
  const modelSelect = voiceEl('voice-model');
  if (!modelSelect.options.length) {
    for (const model of status.models) {
      modelSelect.add(new Option(MODEL_LABELS[model] || model, model));
    }
    modelSelect.value = voiceSettings.local_model;
  }
  const engineReady = voiceSettings.local_gpu ? status.engine_gpu : status.engine_cpu;
  const modelReady = status.downloaded_models.includes(voiceSettings.local_model);
  voiceEl('voice-dl-engine').textContent = engineReady ? 'Re-download engine' : 'Download engine';
  voiceEl('voice-dl-model').textContent = modelReady ? 'Re-download model' : 'Download model';
  voiceEl('voice-engine-status').textContent =
    `Engine (${voiceSettings.local_gpu ? 'GPU' : 'CPU'}): ${engineReady ? 'ready' : 'not downloaded'} · ` +
    `Model: ${modelReady ? 'ready' : 'not downloaded'}`;
}

async function loadMicList() {
  const select = voiceEl('voice-mic');
  const devices = await invoke('list_input_devices');
  select.innerHTML = '';
  select.add(new Option('Windows default microphone', ''));
  const names = new Set(devices);
  // Keep the saved value selectable even if the controller is unplugged right now.
  if (voiceSettings.mic_name) names.add(voiceSettings.mic_name);
  for (const name of names) {
    const label = name === 'Wireless Controller' ? 'DualSense (Wireless Controller)' : name;
    select.add(new Option(label, name));
  }
  select.value = voiceSettings.mic_name;
}

async function voiceDownload(what, button) {
  button.disabled = true;
  try {
    const msg = await invoke('voice_download', { what });
    setStatus(msg);
  } catch (error) {
    setStatus(`Download error: ${error}`);
  }
  button.disabled = false;
  await refreshEngineStatus();
}

function formatMB(bytes) {
  return (bytes / (1024 * 1024)).toFixed(0);
}

async function initVoiceChat() {
  if (!voiceEl('voice-engine')) return;
  try {
    voiceSettings = await invoke('get_voice_settings');
  } catch (error) {
    console.error('Failed to load voice settings:', error);
    return;
  }

  for (const [key, id] of Object.entries(voiceFields)) {
    if (key !== 'mic_name' && key !== 'local_model') voiceEl(id).value = voiceSettings[key];
  }
  voiceEl('voice-gpu').checked = voiceSettings.local_gpu;
  renderProviderTable();
  showProviderHint(voiceSettings.engine);
  showEngineSections();
  await loadMicList();
  await refreshEngineStatus();

  const engineSelect = voiceEl('voice-engine');
  engineSelect.addEventListener('change', () => {
    showProviderHint(engineSelect.value);
    saveVoiceSettings();
  });
  const otherFields = Object.values(voiceFields).filter((id) => id !== 'voice-engine');
  for (const id of [...otherFields, 'voice-gpu']) {
    voiceEl(id).addEventListener('change', saveVoiceSettings);
  }
  voiceEl('voice-mic').addEventListener('focus', loadMicList);
  voiceEl('voice-dl-engine').addEventListener('click', (e) => voiceDownload('engine', e.target));
  voiceEl('voice-dl-model').addEventListener('click', (e) => voiceDownload('model', e.target));

  listen('voice-download', ({ payload }) => {
    const total = payload.total ? ` / ${formatMB(payload.total)} MB` : ' MB';
    voiceEl('voice-engine-status').textContent =
      `Downloading ${payload.what}… ${formatMB(payload.done)}${total}`;
  });

  listen('voice-status', ({ payload }) => {
    const last = voiceEl('voice-last');
    if (payload.state === 'disabled') {
      last.textContent = "Open a friend's chat in-game, then hold the mic button.";
      return;
    }
    const text = payload.text ? `“${payload.text}”` : '';
    last.textContent = [text, payload.message].filter(Boolean).join(' — ');
    last.dataset.state = payload.state;
  });
}
