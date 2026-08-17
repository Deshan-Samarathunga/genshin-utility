const { invoke } = window.__TAURI__.core;
const statusBar = document.getElementById('status-bar');

const cooldowns = new Map();
const COOLDOWN_MS = 4000;

function setStatus(msg) {
  if (!statusBar) return;
  statusBar.textContent = msg;
  setTimeout(() => {
    if (statusBar.textContent === msg) {
      statusBar.textContent = 'Ready';
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
      if (scriptName === 'auto_message.ahk') {
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
}

document.addEventListener('DOMContentLoaded', () => {
  const checkboxes = document.querySelectorAll('input[type="checkbox"]');
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
});
