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
  showVoiceSection(tab === 'voice' ? localStorage.getItem(VOICE_SECTION_KEY) || 'controls' : null);
}

const VOICE_SECTION_KEY = 'genshin-utility.voice-section';

// Voice Chat settings sections, picked from the sub-tabs under "Voice Chat" in the sidebar.
// `section` null hides them (another tab is open).
function showVoiceSection(section) {
  const subtabs = document.getElementById('voice-subtabs');
  if (!subtabs) return;
  subtabs.hidden = section === null;
  document.querySelectorAll('.sub-tab-btn').forEach((btn) => {
    btn.classList.toggle('active', btn.dataset.voiceSection === section);
  });
  document.querySelectorAll('.voice-section').forEach((panel) => {
    panel.hidden = panel.dataset.voiceSection !== section;
  });
  if (section) localStorage.setItem(VOICE_SECTION_KEY, section);
}

function updateTabDot(scriptName, on) {
  const dot = document.querySelector(`.tab-dot[data-for="${scriptName}"]`);
  if (dot) dot.classList.toggle('on', on);
}

// ── Page settings that live in the browser (kept across restarts, included in backups) ─────

const UI_SETTINGS_KEY = 'genshin-utility.ui';
// Settings key -> input id.
const UI_FIELDS = {
  dialogue_speed: 'dialogue-speed',
  auto_message_text: 'auto-message-text',
  auto_message_count: 'auto-message-count',
};

function readUiSettings() {
  const ui = {};
  for (const [key, id] of Object.entries(UI_FIELDS)) {
    const input = document.getElementById(id);
    if (input) ui[key] = input.type === 'number' ? Number(input.value) : input.value;
  }
  return ui;
}

function saveUiSettings() {
  localStorage.setItem(UI_SETTINGS_KEY, JSON.stringify(readUiSettings()));
}

async function restoreUiSettings() {
  let saved = {};
  try {
    saved = JSON.parse(localStorage.getItem(UI_SETTINGS_KEY) || '{}');
  } catch {
    saved = {};
  }
  for (const [key, id] of Object.entries(UI_FIELDS)) {
    const input = document.getElementById(id);
    if (!input) continue;
    if (saved[key] !== undefined && saved[key] !== null) input.value = saved[key];
    input.addEventListener('change', saveUiSettings);
  }
  const speed = Number(saved.dialogue_speed);
  if (speed >= 50) {
    await invoke('set_dialogue_speed', { speed }).catch(console.error);
  }
}

function initBackup() {
  const exportBtn = document.getElementById('settings-export');
  const importBtn = document.getElementById('settings-import');
  if (!exportBtn || !importBtn) return;

  exportBtn.addEventListener('click', async () => {
    exportBtn.disabled = true;
    try {
      const now = new Date();
      const date = now.toISOString().slice(0, 10);
      const path = await invoke('export_settings', {
        ui: readUiSettings(),
        exportedAt: now.toISOString(),
        fileName: `genshin-utility-settings-${date}.json`,
      });
      if (path) setStatus('Settings exported (includes API keys — keep the file private)');
    } catch (error) {
      setStatus(`Export error: ${error}`);
    }
    exportBtn.disabled = false;
  });

  importBtn.addEventListener('click', async () => {
    importBtn.disabled = true;
    try {
      const ui = await invoke('import_settings');
      if (ui) {
        localStorage.setItem(UI_SETTINGS_KEY, JSON.stringify(ui || {}));
        // Reload so every field and toggle picks up the imported values.
        window.location.reload();
        return;
      }
    } catch (error) {
      setStatus(`Import error: ${error}`);
    }
    importBtn.disabled = false;
  });
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
      const result = await invoke('set_auto_open', { enabled: toggle.checked });
      toggle.checked = result.enabled;
      if (result.enabled) {
        setStatus(`Will open when Genshin starts (${result.game_path || 'GenshinImpact.exe'})`);
      } else {
        setStatus('Open with Genshin turned off');
      }
    } catch (error) {
      toggle.checked = !toggle.checked;
      setStatus(`Error: ${error}`);
    }
    toggle.disabled = false;
  });
}


async function initAbout() {
  const version = document.getElementById('app-version');
  const button = document.getElementById('check-updates');
  const desc = document.getElementById('update-desc');
  const bar = document.getElementById('update-progress');
  const settingsTab = document.querySelector('.tab-settings');
  let available = null;

  try {
    version.textContent = `v${await window.__TAURI__.app.getVersion()}`;
  } catch {
    version.textContent = '';
  }

  const showAvailable = (update) => {
    available = update;
    settingsTab.classList.toggle('has-update', !!update);
    if (update) {
      desc.textContent = `v${update.version} available`;
      button.textContent = `Update to v${update.version}`;
    }
  };

  const check = async (quiet) => {
    button.disabled = true;
    if (!quiet) desc.textContent = 'Checking…';
    try {
      const update = await invoke('check_update');
      showAvailable(update);
      if (!update && !quiet) desc.textContent = 'Up to date';
    } catch (error) {
      if (!quiet) desc.textContent = String(error);
    } finally {
      button.disabled = false;
    }
  };

  const install = async () => {
    button.disabled = true;
    bar.hidden = false;
    desc.textContent = `Downloading v${available.version}…`;
    const fill = bar.firstElementChild;
    const unlisten = await window.__TAURI__.event.listen('update-progress', ({ payload }) => {
      const mb = (payload.downloaded / 1048576).toFixed(1);
      if (payload.total) {
        const pct = Math.min(100, Math.round((payload.downloaded / payload.total) * 100));
        fill.style.width = `${pct}%`;
        desc.textContent = `Downloading v${available.version}… ${pct}%`;
      } else {
        desc.textContent = `Downloading v${available.version}… ${mb} MB`;
      }
    });
    try {
      // On success the installer closes the app, so this normally never resolves.
      await invoke('install_update');
      desc.textContent = 'Installing…';
    } catch (error) {
      desc.textContent = String(error);
      bar.hidden = true;
      fill.style.width = '0';
      showAvailable(null);
      button.textContent = 'Check for updates';
      button.disabled = false;
    } finally {
      unlisten();
    }
  };

  button.addEventListener('click', () => (available ? install() : check(false)));
  // Quiet check shortly after start: only a dot on the Settings tab if something new is out.
  setTimeout(() => check(true), 3000);
}

function initTabs() {
  const buttons = document.querySelectorAll('.tab-btn');
  buttons.forEach((btn) => btn.addEventListener('click', () => selectTab(btn.dataset.tab)));
  document.querySelectorAll('.sub-tab-btn').forEach((btn) => {
    btn.addEventListener('click', () => showVoiceSection(btn.dataset.voiceSection));
  });
  const saved = localStorage.getItem(TAB_KEY);
  const exists = [...buttons].some((btn) => btn.dataset.tab === saved);
  selectTab(exists ? saved : buttons[0].dataset.tab);
}

document.addEventListener('DOMContentLoaded', () => {
  initTabs();
  initAutoOpen();
  initBackup();
  initAbout();
  restoreUiSettings();

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

// ── Voice Chat ─────────────────────────────────────────────────

const voiceFields = {
  engine: 'voice-engine',
  mic_name: 'voice-mic',
  keyboard_mic_name: 'voice-mic-keys',
  ptt_key: 'voice-ptt-key',
  language: 'voice-language',
  local_model: 'voice-model',
};

const PROVIDER_INFO = {
  groq: {
    label: 'Groq',
    hint: 'Free: ~2,000 requests/day.',
    url: 'https://console.groq.com/keys',
    placeholder: 'gsk_...',
  },
  gemini: {
    label: 'Google Gemini',
    hint: 'Free tier (data may be used for training).',
    url: 'https://aistudio.google.com/apikey',
    placeholder: 'AIza...',
  },
  deepgram: {
    label: 'Deepgram',
    hint: '$200 free credit.',
    url: 'https://console.deepgram.com/',
    placeholder: 'Deepgram API key',
  },
  elevenlabs: {
    label: 'ElevenLabs',
    hint: 'Free monthly credits.',
    url: 'https://elevenlabs.io/app/settings/api-keys',
    placeholder: 'sk_...',
  },
  mistral: {
    label: 'Mistral',
    hint: 'Free plan (low rate limits).',
    url: 'https://console.mistral.ai/api-keys',
    placeholder: 'Mistral API key',
  },
  openai: {
    label: 'OpenAI',
    hint: 'Paid per minute.',
    url: 'https://platform.openai.com/api-keys',
    placeholder: 'sk-...',
  },
  custom: {
    label: 'Custom',
    hint: 'OpenAI-compatible server, base URL ending in /v1.',
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
  next.replacements = readCorrections();
  next.vocabulary = vocabWords.join(', ');
  // Every provider's row in the keys table.
  document.querySelectorAll('#voice-provider-rows input[data-field]').forEach((input) => {
    const id = input.dataset.provider;
    next.cloud[id] = { ...next.cloud[id], [input.dataset.field]: input.value.trim() };
  });
  return next;
}

// ── Names & words chips (stored as a comma-separated list) ──

let vocabWords = [];

function splitWords(text) {
  return (text || '').split(/[,\n]/).map((w) => w.trim()).filter(Boolean);
}

function renderVocab() {
  const box = voiceEl('voice-vocab-chips');
  box.innerHTML = '';
  vocabWords.forEach((word, index) => {
    const chip = document.createElement('span');
    chip.className = 'vocab-chip';
    chip.textContent = word;
    const remove = document.createElement('button');
    remove.type = 'button';
    remove.title = `Remove ${word}`;
    remove.textContent = '✕';
    remove.addEventListener('click', () => {
      vocabWords.splice(index, 1);
      renderVocab();
      saveVoiceSettings();
    });
    chip.append(remove);
    box.append(chip);
  });
  voiceEl('voice-vocab-count').textContent = `${vocabWords.length} words`;
}

// Adds every comma-separated word in `text`, skipping duplicates (case-insensitive).
function addVocab(text) {
  const known = new Set(vocabWords.map((w) => w.toLowerCase()));
  let added = false;
  for (const word of splitWords(text)) {
    if (!known.has(word.toLowerCase())) {
      vocabWords.push(word);
      known.add(word.toLowerCase());
      added = true;
    }
  }
  if (added) {
    renderVocab();
    saveVoiceSettings();
  }
}

function initVocab() {
  vocabWords = splitWords(voiceSettings.vocabulary);
  renderVocab();
  const input = voiceEl('voice-vocab-input');
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' || e.key === ',') {
      e.preventDefault();
      addVocab(input.value);
      input.value = '';
    } else if (e.key === 'Backspace' && !input.value && vocabWords.length) {
      vocabWords.pop();
      renderVocab();
      saveVoiceSettings();
    }
  });
  // Pasting "a, b, c" adds them all at once.
  input.addEventListener('paste', (e) => {
    const text = e.clipboardData.getData('text');
    if (/[,\n]/.test(text)) {
      e.preventDefault();
      addVocab(text);
    }
  });
  input.addEventListener('blur', () => {
    addVocab(input.value);
    input.value = '';
  });
  voiceEl('voice-vocab-box').addEventListener('click', (e) => {
    if (e.target === e.currentTarget || e.target.id === 'voice-vocab-chips') input.focus();
  });
}

// ── Auto-corrections table (stored as "heard => wanted" lines) ──

function parseCorrections(text) {
  return (text || '')
    .split('\n')
    .map((line) => {
      const at = line.indexOf('=>');
      return at < 0 ? null : [line.slice(0, at).trim(), line.slice(at + 2).trim()];
    })
    .filter((pair) => pair && pair[0]);
}

function readCorrections() {
  return [...document.querySelectorAll('#voice-correction-rows tr')]
    .map((row) => [...row.querySelectorAll('input')].map((input) => input.value.trim()))
    .filter(([heard]) => heard)
    .map(([heard, wanted]) => `${heard} => ${wanted}`)
    .join('\n');
}

function addCorrectionRow(heard = '', wanted = '') {
  const tbody = voiceEl('voice-correction-rows');
  const row = document.createElement('tr');

  const input = (value, placeholder) => {
    const cell = document.createElement('td');
    const field = document.createElement('input');
    field.type = 'text';
    field.value = value;
    field.placeholder = placeholder;
    field.spellcheck = false;
    field.addEventListener('change', saveVoiceSettings);
    // Typing into the last row adds a fresh empty row below it.
    field.addEventListener('input', () => {
      if (row === tbody.lastElementChild && field.value) addCorrectionRow();
    });
    cell.append(field);
    return cell;
  };

  const arrow = document.createElement('td');
  arrow.className = 'voice-arrow';
  arrow.textContent = '→';

  const remove = document.createElement('td');
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'voice-remove';
  button.title = 'Remove';
  button.textContent = '✕';
  button.addEventListener('click', () => {
    row.remove();
    if (!tbody.children.length || tbody.lastElementChild.querySelector('input').value) addCorrectionRow();
    saveVoiceSettings();
  });
  remove.append(button);

  row.append(input(heard, 'heard'), arrow, input(wanted, 'wanted'), remove);
  tbody.append(row);
}

function renderCorrections() {
  voiceEl('voice-correction-rows').innerHTML = '';
  for (const [heard, wanted] of parseCorrections(voiceSettings.replacements)) addCorrectionRow(heard, wanted);
  addCorrectionRow(); // always one empty row to type into
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

// Fills both mic pickers: the controller mic and the keyboard & mouse (headset) mic.
async function loadMicList() {
  const devices = await invoke('list_input_devices');
  for (const [id, key] of [['voice-mic', 'mic_name'], ['voice-mic-keys', 'keyboard_mic_name']]) {
    const select = voiceEl(id);
    select.innerHTML = '';
    select.add(new Option('Windows default microphone', ''));
    // "Wireless Controller" matches the DualSense mic by name, even while it's unplugged.
    const names = new Set(['Wireless Controller', ...devices]);
    if (voiceSettings[key]) names.add(voiceSettings[key]);
    for (const name of names) {
      const label = name === 'Wireless Controller' ? 'DualSense controller mic' : name;
      select.add(new Option(label, name));
    }
    select.value = voiceSettings[key];
  }
}

function showPttHint() {
  const select = voiceEl('voice-ptt-key');
  const label = select.options[select.selectedIndex]?.text.split(' (')[0] || 'Off';
  voiceEl('voice-ptt-hint').textContent = label;
  voiceEl('voice-ptt-or').hidden = select.value === 'off';
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
    if (!['mic_name', 'keyboard_mic_name', 'local_model'].includes(key)) voiceEl(id).value = voiceSettings[key];
  }
  voiceEl('voice-gpu').checked = voiceSettings.local_gpu;
  renderProviderTable();
  renderCorrections();
  initVocab();
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
  for (const id of ['voice-mic', 'voice-mic-keys']) voiceEl(id).addEventListener('focus', loadMicList);
  showPttHint();
  voiceEl('voice-ptt-key').addEventListener('change', showPttHint);
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
      last.textContent = '';
      return;
    }
    const text = payload.text ? `“${payload.text}”` : '';
    last.textContent = [text, payload.message].filter(Boolean).join(' — ');
    last.dataset.state = payload.state;
  });
}
