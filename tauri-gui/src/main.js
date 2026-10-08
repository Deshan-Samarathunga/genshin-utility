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
  // Auto Dialogue and Story Mode switch each other off.
  const other = { auto_dialogue: 'story_mode', story_mode: 'auto_dialogue' }[scriptName];
  const otherBox = other && document.querySelector(`input[data-script="${other}"]`);
  if (otherBox) updateToggleState(otherBox);
}

// ── Hotkeys (assignable in Settings; saved with the page settings and in backups) ──────────

const HOTKEY_LABELS = {
  dialogue: 'Auto Dialogue / Story Mode start & stop',
  artifacts_one: 'Remove artifacts: this character',
  artifacts_all: 'Remove artifacts: every character',
  prev_character: 'Previous character',
  next_character: 'Next character',
  auto_message: 'Auto Message start',
};
let hotkeys = {};
let defaultHotkeys = {};

const KEY_NAMES = {
  8: 'Backspace', 9: 'Tab', 13: 'Enter', 19: 'Pause', 20: 'Caps Lock', 27: 'Esc', 32: 'Space',
  33: 'Page Up', 34: 'Page Down', 35: 'End', 36: 'Home', 37: 'Left', 38: 'Up', 39: 'Right', 40: 'Down',
  45: 'Insert', 46: 'Delete', 106: 'Num *', 107: 'Num +', 109: 'Num -', 110: 'Num .', 111: 'Num /',
  186: ';', 187: '=', 188: ',', 189: '-', 190: '.', 191: '/', 192: '`', 219: '[', 220: '\\', 221: ']', 222: "'",
};

function keyName(vk) {
  if (vk >= 112 && vk <= 135) return `F${vk - 111}`;
  if ((vk >= 48 && vk <= 57) || (vk >= 65 && vk <= 90)) return String.fromCharCode(vk);
  if (vk >= 96 && vk <= 105) return `Num ${vk - 96}`;
  return KEY_NAMES[vk] || `Key ${vk}`;
}

function hotkeyText(h) {
  if (!h) return '—';
  return [h.ctrl && 'Ctrl', h.alt && 'Alt', h.shift && 'Shift', keyName(h.vk)].filter(Boolean).join('+');
}

function renderHotkeys() {
  document.querySelectorAll('[data-hotkey]').forEach((el) => {
    el.textContent = hotkeyText(hotkeys[el.dataset.hotkey]);
  });
  document.querySelectorAll('.hotkey-btn').forEach((btn) => {
    if (!btn.classList.contains('listening')) btn.textContent = hotkeyText(hotkeys[btn.dataset.action]);
  });
}

async function applyHotkeys(next) {
  await invoke('set_hotkeys', { bindings: next });
  hotkeys = next;
  renderHotkeys();
  saveUiSettings();
}

// Waits for the next key combination pressed in the app window.
function captureHotkey(btn) {
  document.querySelectorAll('.hotkey-btn.listening').forEach((b) => b.classList.remove('listening'));
  btn.classList.add('listening');
  btn.textContent = 'Press keys…';
  const onKey = async (e) => {
    e.preventDefault();
    e.stopPropagation();
    if ([16, 17, 18, 91, 92, 93].includes(e.keyCode)) return; // modifier alone: keep waiting
    window.removeEventListener('keydown', onKey, true);
    btn.classList.remove('listening');
    if (e.keyCode === 27 && !e.ctrlKey && !e.altKey && !e.shiftKey) {
      renderHotkeys();
      return;
    }
    const next = { ...hotkeys, [btn.dataset.action]: { vk: e.keyCode, ctrl: e.ctrlKey, alt: e.altKey, shift: e.shiftKey } };
    try {
      await applyHotkeys(next);
      setStatus(`${HOTKEY_LABELS[btn.dataset.action]}: ${hotkeyText(next[btn.dataset.action])}`);
    } catch (error) {
      setStatus(`Error: ${error}`);
      renderHotkeys();
    }
  };
  window.addEventListener('keydown', onKey, true);
}

async function initHotkeys() {
  defaultHotkeys = await invoke('default_hotkeys').catch(() => ({}));
  let saved = {};
  try {
    saved = JSON.parse(localStorage.getItem(UI_SETTINGS_KEY) || '{}').hotkeys || {};
  } catch {
    saved = {};
  }
  hotkeys = { ...defaultHotkeys, ...saved };
  try {
    await invoke('set_hotkeys', { bindings: hotkeys });
  } catch (error) {
    // Saved keys clash (e.g. after an update): fall back to the defaults.
    hotkeys = { ...defaultHotkeys };
    setStatus(`Hotkeys reset: ${error}`);
  }

  const rows = document.getElementById('hotkey-rows');
  for (const [action, label] of Object.entries(HOTKEY_LABELS)) {
    const row = document.createElement('div');
    row.className = 'settings-row';
    const name = document.createElement('div');
    name.className = 'settings-label';
    name.textContent = label;
    const btn = document.createElement('button');
    btn.type = 'button';
    btn.className = 'btn hotkey-btn';
    btn.dataset.action = action;
    btn.addEventListener('click', () => captureHotkey(btn));
    row.append(name, btn);
    rows.append(row);
  }
  document.getElementById('hotkeys-reset').addEventListener('click', async () => {
    try {
      await applyHotkeys({ ...defaultHotkeys });
      setStatus('Hotkeys reset to defaults');
    } catch (error) {
      setStatus(`Error: ${error}`);
    }
  });
  renderHotkeys();
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
  // A saved sub-tab that no longer exists (API keys moved to its own tab) falls back to the first.
  const saved = localStorage.getItem(VOICE_SECTION_KEY);
  const known = document.querySelector(`.sub-tab-btn[data-voice-section="${saved}"]`);
  showVoiceSection(tab === 'voice' ? (known ? saved : 'controls') : null);
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
  artifact_speed: 'artifact-speed',
  auto_message_text: 'auto-message-text',
  auto_message_count: 'auto-message-count',
  story_speed: 'story-speed',
  story_match_length: 'story-match-length',
  story_detail: 'story-detail',
  story_provider: 'story-provider',
  story_model: 'story-model',
};

function readUiSettings() {
  const ui = { hotkeys };
  for (const [key, id] of Object.entries(UI_FIELDS)) {
    const input = document.getElementById(id);
    if (!input) continue;
    if (input.type === 'checkbox') ui[key] = input.checked;
    else ui[key] = input.type === 'number' ? Number(input.value) : input.value;
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
    if (saved[key] !== undefined && saved[key] !== null) {
      if (input.type === 'checkbox') input.checked = !!saved[key];
      else input.value = saved[key];
    }
    input.addEventListener('change', saveUiSettings);
  }
  const speed = Number(saved.dialogue_speed);
  if (speed >= 50) {
    await invoke('set_dialogue_speed', { speed }).catch(console.error);
  }
  const artifactSpeed = document.getElementById('artifact-speed');
  const artifactSpeedValue = document.getElementById('artifact-speed-value');
  // Older saves stored "normal" / "fast" / "fastest".
  const legacy = { normal: 1, fast: 5, fastest: 9 };
  if (legacy[saved.artifact_speed]) artifactSpeed.value = legacy[saved.artifact_speed];
  const pushArtifactSpeed = () => {
    artifactSpeedValue.textContent = artifactSpeed.value;
    invoke('set_artifact_speed', { level: Number(artifactSpeed.value) }).catch(console.error);
  };
  artifactSpeed.addEventListener('input', pushArtifactSpeed);
  artifactSpeed.addEventListener('change', saveUiSettings);
  pushArtifactSpeed();
  const storySpeed = Number(saved.story_speed);
  if (storySpeed >= 50) {
    await invoke('set_story_speed', { speed: storySpeed }).catch(console.error);
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
  const portable = await invoke('is_portable').catch(() => false);

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
      button.textContent = portable ? `Download v${update.version}` : `Update to v${update.version}`;
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
    if (portable) {
      window.__TAURI__.opener.openUrl('https://github.com/Deshan-Samarathunga/genshin-utility/releases/latest');
      return;
    }
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

  const storySpeedInput = document.getElementById('story-speed');
  storySpeedInput.addEventListener('change', async (e) => {
    const speed = parseInt(e.target.value, 10);
    if (speed >= 50) await invoke('set_story_speed', { speed }).catch(console.error);
  });

  // Progress of Shift+F7 (artifacts from every character).
  listen('artifact-status', ({ payload }) => setStatus(payload));

  initHotkeys();
  initVoiceChat();
  initStory();
});

// ── Auto Dialogue story mode ─────────────────────────────────────────────────

const STORY_MODELS = {
  groq: 'llama-3.3-70b-versatile',
  gemini: 'gemini-3.5-flash-lite',
  openai: 'gpt-4o-mini',
  mistral: 'mistral-small-latest',
  custom: 'gpt-4o-mini',
};

// History session that new Story Mode runs are added to (one story over several sittings).
const STORY_CONTINUE_KEY = 'genshin-utility.story-continue';
let storyContinue = Number(localStorage.getItem(STORY_CONTINUE_KEY)) || null;

function setStoryContinue(id) {
  storyContinue = id || null;
  if (storyContinue) localStorage.setItem(STORY_CONTINUE_KEY, String(storyContinue));
  else localStorage.removeItem(STORY_CONTINUE_KEY);
  pushStorySettings();
}

// Suggestions in the Model box; any model the provider offers can be typed in.
const STORY_MODEL_OPTIONS = {
  groq: ['llama-3.3-70b-versatile', 'llama-3.1-8b-instant'],
  gemini: ['gemini-3.5-flash-lite', 'gemini-3.5-flash', 'gemini-3.1-pro-preview'],
  openai: ['gpt-4o-mini', 'gpt-4o'],
  mistral: ['mistral-small-latest', 'mistral-large-latest'],
  custom: [],
};
let storyProviderShown = null;

// Keeps the Model box showing the model that's really used: switching provider swaps in the new
// provider's default unless a model of your own was typed.
function syncStoryModel() {
  const provider = document.getElementById('story-provider').value;
  const model = document.getElementById('story-model');
  const previousDefault = STORY_MODELS[storyProviderShown] || '';
  if (!model.value.trim() || (storyProviderShown && storyProviderShown !== provider && model.value.trim() === previousDefault)) {
    model.value = STORY_MODELS[provider] || '';
  }
  storyProviderShown = provider;
  const list = document.getElementById('story-model-options');
  list.replaceChildren(...(STORY_MODEL_OPTIONS[provider] || []).map((m) => new Option(m, m)));
}

function pushStorySettings() {
  const provider = document.getElementById('story-provider');
  const model = document.getElementById('story-model');
  syncStoryModel();
  invoke('set_story_settings', {
    settings: {
      provider: provider.value,
      model: model.value.trim(),
      match_length: document.getElementById('story-match-length').checked,
      continue_from: storyContinue,
      detail: document.getElementById('story-detail').value,
    },
  }).catch(console.error);
}

// ── Story history: every run is kept; tick sessions to merge them, related ones are suggested ──

let storySessions = [];
const storyTicked = new Set();
const storyOpen = new Set();
// Sessions whose dialogue list is showing, and their loaded transcripts.
const storyDialogueOpen = new Set();
const storyTranscripts = new Map();

// Splits "Speaker: text" (speaker is a short name, no sentence) from a transcript line.
function splitSpeaker(line) {
  const m = line.match(/^([^:.!?]{1,40}):\s+(.+)$/);
  return m ? [m[1], m[2]] : ['', line];
}

function renderDialogue(container, lines, filter) {
  container.replaceChildren();
  const needle = filter.trim().toLowerCase();
  let shown = 0;
  for (const line of lines) {
    if (needle && !line.toLowerCase().includes(needle)) continue;
    shown++;
    const row = document.createElement('div');
    if (line.startsWith('--- ')) {
      row.className = 'dlg-break';
      row.textContent = line.replace(/^-+\s*|\s*-+$/g, '');
    } else if (line.startsWith('> ')) {
      row.className = 'dlg-choice';
      row.textContent = line.replace(/^>\s*(Chose:)?\s*/, '');
    } else {
      const [speaker, text] = splitSpeaker(line);
      row.className = 'dlg-line';
      if (speaker) {
        const who = document.createElement('span');
        who.className = 'dlg-speaker';
        who.textContent = speaker;
        row.append(who);
      }
      const said = document.createElement('span');
      said.textContent = text;
      row.append(said);
    }
    container.append(row);
  }
  if (!shown) {
    const empty = document.createElement('p');
    empty.className = 'voice-note';
    empty.textContent = needle ? 'No lines match' : 'No dialogue saved';
    container.append(empty);
  }
}

function storyDialogue(session) {
  const wrap = document.createElement('div');
  wrap.className = 'story-dialogue';
  const search = document.createElement('input');
  search.type = 'text';
  search.placeholder = 'Search lines or names';
  search.spellcheck = false;
  const list = document.createElement('div');
  list.className = 'dlg-list';
  const show = () => renderDialogue(list, storyTranscripts.get(session.id) || [], search.value);
  search.addEventListener('input', show);
  wrap.append(search, list);
  if (storyTranscripts.has(session.id)) {
    show();
  } else {
    list.textContent = 'Loading…';
    invoke('story_transcript', { id: session.id })
      .then((lines) => {
        storyTranscripts.set(session.id, lines);
        show();
      })
      .catch((e) => (list.textContent = String(e)));
  }
  return wrap;
}

function storyDate(ms) {
  return new Date(ms).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
}

function storyButton(label, className, onClick) {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = className;
  button.textContent = label;
  button.addEventListener('click', onClick);
  return button;
}

// Shows a summary's light Markdown (headings, bullets, **bold**) without trusting it as HTML.
function renderSummary(container, markdown) {
  container.replaceChildren();
  let list = null;
  const inline = (el, text) => {
    text.split(/(\*\*[^*]+\*\*)/).forEach((piece) => {
      if (piece.startsWith('**') && piece.endsWith('**') && piece.length > 4) {
        const strong = document.createElement('strong');
        strong.textContent = piece.slice(2, -2);
        el.append(strong);
      } else if (piece) {
        el.append(piece);
      }
    });
    return el;
  };
  for (const raw of markdown.split('\n')) {
    const line = raw.trim();
    if (!line) {
      list = null;
      continue;
    }
    const heading = line.match(/^(#{1,4})\s+(.*)$/);
    const bullet = line.match(/^[-*•]\s+(.*)$/);
    if (heading) {
      list = null;
      container.append(inline(document.createElement(heading[1].length <= 2 ? 'h4' : 'h5'), heading[2]));
    } else if (bullet) {
      if (!list) {
        list = document.createElement('ul');
        container.append(list);
      }
      list.append(inline(document.createElement('li'), bullet[1]));
    } else {
      list = null;
      container.append(inline(document.createElement('p'), line));
    }
  }
}

function storyItemBody(session) {
  const body = document.createElement('div');
  body.className = 'story-item-body';
  const text = document.createElement('div');
  text.className = 'story-summary-text';
  if (session.summary) renderSummary(text, session.summary);
  else text.textContent = 'Not summarized yet (offline or the AI was busy). It retries when the app opens, or press Retry.';
  const actions = document.createElement('div');
  actions.className = 'settings-actions';
  if (session.summary) {
    actions.append(
      storyButton('Copy', 'btn', async () => {
        await navigator.clipboard.writeText(`${session.title}\n\n${session.summary}`.trim());
        setStatus('Summary copied');
      }),
    );
    const again = storyButton('Summarize again', 'btn', () => mergeStories([session.id], again));
    again.title = 'Rewrite the summary with the current Summary setting';
    actions.append(again);
  } else {
    const retry = storyButton('Retry summary', 'btn', () => mergeStories([session.id], retry));
    actions.append(retry);
  }
  const dialogueOpen = storyDialogueOpen.has(session.id);
  actions.append(
    storyButton(dialogueOpen ? 'Hide dialogue' : `Dialogue (${session.lines})`, 'btn', () => {
      if (dialogueOpen) storyDialogueOpen.delete(session.id);
      else storyDialogueOpen.add(session.id);
      renderStoryHistory();
    }),
  );
  const continuing = storyContinue === session.id;
  const cont = storyButton(continuing ? 'Stop continuing' : 'Continue this story', 'btn', () => {
    setStoryContinue(continuing ? null : session.id);
    renderStoryHistory();
  });
  cont.title = 'Add your next Story Mode runs to this session; lines you already read are skipped';
  actions.append(cont);
  const del = storyButton('Delete', 'btn btn-danger', async () => {
    if (del.dataset.armed) {
      await invoke('story_delete', { id: session.id }).catch(console.error);
    } else {
      del.dataset.armed = '1';
      del.textContent = 'Click again to delete';
    }
  });
  actions.append(del);
  body.append(text, actions);
  if (dialogueOpen) body.append(storyDialogue(session));
  return body;
}

function renderStoryHistory() {
  const list = document.getElementById('story-history');
  // The continued session was deleted: new runs start fresh again.
  if (storyContinue && storySessions.length && !storySessions.some((s) => s.id === storyContinue)) setStoryContinue(null);
  const byId = new Map(storySessions.map((s) => [s.id, s]));
  for (const id of [...storyTicked]) if (!byId.has(id)) storyTicked.delete(id);
  // Sessions related to anything ticked are the suggestions.
  const suggested = new Set();
  storyTicked.forEach((id) => byId.get(id)?.related.forEach((r) => !storyTicked.has(r) && suggested.add(r)));

  list.replaceChildren();
  if (!storySessions.length) {
    const empty = document.createElement('p');
    empty.className = 'voice-note';
    empty.textContent = 'No sessions yet';
    list.append(empty);
  }
  for (const session of storySessions) {
    const item = document.createElement('div');
    item.className = 'story-item';
    item.classList.toggle('ticked', storyTicked.has(session.id));
    item.classList.toggle('suggested', suggested.has(session.id));

    const head = document.createElement('div');
    head.className = 'story-item-head';
    const tick = document.createElement('input');
    tick.type = 'checkbox';
    tick.checked = storyTicked.has(session.id);
    tick.addEventListener('change', () => {
      if (tick.checked) storyTicked.add(session.id);
      else storyTicked.delete(session.id);
      renderStoryHistory();
    });
    const title = storyButton(
      session.title || (session.summary ? 'Untitled story' : 'Not summarized'),
      'story-item-title',
      () => {
        if (storyOpen.has(session.id)) storyOpen.delete(session.id);
        else storyOpen.add(session.id);
        renderStoryHistory();
      },
    );
    const meta = document.createElement('span');
    meta.className = 'story-item-meta';
    meta.textContent = [storyDate(session.id), `${session.lines} lines`, session.parts > 1 ? `${session.parts} sessions` : '']
      .filter(Boolean)
      .join(' · ');
    const chevron = storyButton('', 'story-chevron', () => title.click());
    chevron.setAttribute('aria-label', storyOpen.has(session.id) ? 'Collapse' : 'Expand');
    chevron.innerHTML =
      '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M5 9l7 7 7-7"/></svg>';
    chevron.classList.toggle('open', storyOpen.has(session.id));
    head.append(tick, title, meta);
    if (storyContinue === session.id) {
      const chip = document.createElement('span');
      chip.className = 'story-chip';
      chip.textContent = 'Continuing';
      head.append(chip);
    }
    if (suggested.has(session.id)) {
      const chip = document.createElement('span');
      chip.className = 'story-chip';
      chip.textContent = 'Related';
      head.append(chip);
    } else if (session.related.length && !storyTicked.has(session.id)) {
      const pick = storyButton(`${session.related.length} related`, 'story-link', () => {
        [session.id, ...session.related].forEach((id) => storyTicked.add(id));
        renderStoryHistory();
      });
      pick.title = 'Tick this session and the ones that look like the same story';
      head.append(pick);
    }
    head.append(chevron);
    item.append(head);
    if (storyOpen.has(session.id)) item.append(storyItemBody(session));
    list.append(item);
  }

  const merge = document.getElementById('story-merge');
  merge.disabled = storyTicked.size < 2;
  merge.textContent = storyTicked.size >= 2 ? `Merge ${storyTicked.size}` : 'Merge';
  document.getElementById('story-merge-hint').textContent = suggested.size
    ? `${suggested.size} related session${suggested.size > 1 ? 's' : ''} highlighted`
    : '';
}

async function mergeStories(ids, button) {
  if (button) button.disabled = true;
  try {
    await invoke('story_merge', { ids });
    // The merged session keeps the earliest id; keep continuing it if one of the parts was.
    if (ids.includes(storyContinue)) setStoryContinue(Math.min(...ids));
    storyTicked.clear();
    storyOpen.clear();
    storyOpen.add(Math.min(...ids));
    renderStoryHistory();
  } catch (error) {
    setStatus(`Error: ${error}`);
  } finally {
    if (button) button.disabled = false;
  }
}

function initStory() {
  document.getElementById('story-model').addEventListener('input', saveUiSettings);
  ['story-provider', 'story-model', 'story-match-length', 'story-detail'].forEach((id) => {
    document.getElementById(id).addEventListener('change', pushStorySettings);
  });
  pushStorySettings();
  const status = document.getElementById('story-status');
  const merge = document.getElementById('story-merge');
  merge.addEventListener('click', () => mergeStories([...storyTicked], merge));
  // Summarize runs saved while offline or recovered after a crash (quietly; stops if still offline).
  invoke('story_retry_pending').catch(() => {});
  invoke('story_history')
    .then((sessions) => {
      storySessions = sessions || [];
      if (storySessions[0]) storyOpen.add(storySessions[0].id);
      renderStoryHistory();
    })
    .catch(console.error);
  listen('story-history', ({ payload }) => {
    storyTranscripts.clear(); // sessions may have grown (continued) or been merged
    const known = new Set(storySessions.map((s) => s.id));
    storySessions = payload;
    // Open a newly saved run so its summary shows right away.
    storySessions.filter((s) => !known.has(s.id)).forEach((s) => storyOpen.add(s.id));
    renderStoryHistory();
  });

  listen('story', ({ payload }) => {
    status.dataset.state = payload.state;
    switch (payload.state) {
      case 'reading':
        status.textContent = payload.text || `Reading… ${payload.lines} lines`;
        break;
      case 'choosing':
        status.textContent = 'Choosing…';
        break;
      case 'summarizing':
        status.textContent = payload.text || 'Summarizing…';
        break;
      case 'done':
        status.textContent = '';
        setStatus('Story summary ready');
        break;
      case 'error':
        status.textContent = payload.text;
        break;
      default:
        status.textContent = '';
    }
  });
}

// ── Voice Chat ─────────────────────────────────────────────────

const voiceFields = {
  engine: 'voice-engine',
  mic_name: 'voice-mic',
  keyboard_mic_name: 'voice-mic-keys',
  ptt_key: 'voice-ptt-key',
  language: 'voice-language',
  mic_sensitivity: 'voice-sensitivity',
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

// Suggestions in the speech Model box; any model the provider offers can be typed in.
const VOICE_MODEL_OPTIONS = {
  groq: ['whisper-large-v3', 'whisper-large-v3-turbo'],
  gemini: ['gemini-3.5-flash-lite', 'gemini-3.5-flash', 'gemini-3.1-pro-preview'],
  deepgram: ['nova-3', 'nova-2'],
  elevenlabs: ['scribe_v2', 'scribe_v1'],
  mistral: ['voxtral-mini-latest', 'voxtral-small-latest'],
  openai: ['gpt-4o-transcribe', 'gpt-4o-mini-transcribe', 'whisper-1'],
  custom: ['whisper-1'],
};
// Which provider the speech Model box is showing.
let voiceModelFor = null;

function showVoiceModel() {
  const engine = voiceEl('voice-engine').value;
  voiceModelFor = engine;
  if (engine === 'local') return;
  voiceEl('voice-cloud-model').value = voiceSettings.cloud[engine]?.model || '';
  voiceEl('voice-cloud-model-options').replaceChildren(
    ...(VOICE_MODEL_OPTIONS[engine] || []).map((m) => new Option(m, m)),
  );
}

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
  // The Model box belongs to the provider it was filled for (the engine may have just changed).
  if (voiceModelFor && voiceModelFor !== 'local') {
    next.cloud[voiceModelFor] = { ...next.cloud[voiceModelFor], model: voiceEl('voice-cloud-model').value.trim() };
  }
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
    // Models are picked on each feature's own page; this table is keys and endpoints only.
    const endpoint = field('base_url', 'text', 'https://.../v1');
    endpoint.cell.classList.add('voice-endpoint-cell');
    row.append(key.cell, endpoint.cell);

    tbody.append(row);
  }
  updateProviderTable();
}

// Refreshes saved/active state and backend-filled defaults without rebuilding inputs (keeps focus).
function updateProviderTable() {
  document.querySelectorAll('#voice-provider-rows tr').forEach((row) => {
    const id = row.dataset.provider;
    const config = voiceSettings.cloud[id] || {};
    row.querySelector('.voice-key-status').textContent = config.api_key ? 'Saved' : 'No key';
    row.querySelector('.voice-key-status').classList.toggle('saved', Boolean(config.api_key));
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
  showVoiceModel();
  await loadMicList();
  await refreshEngineStatus();

  const engineSelect = voiceEl('voice-engine');
  engineSelect.addEventListener('change', async () => {
    showProviderHint(engineSelect.value);
    await saveVoiceSettings(); // saves the old provider's model box first
    showVoiceModel();
  });
  voiceEl('voice-cloud-model').addEventListener('change', saveVoiceSettings);
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
