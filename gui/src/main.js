const statusBar = document.getElementById('status-bar');

// After a user-initiated toggle, skip automatic status polling for this
// checkbox for a few seconds.  This prevents stale process-table data from
// bouncing the toggle back before taskkill has fully taken effect.
const cooldowns = new Map(); // data-script → timestamp
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

  // If this checkbox was just toggled by the user, skip the automatic poll
  const cooldownUntil = cooldowns.get(scriptName) || 0;
  if (Date.now() < cooldownUntil) return;

  try {
    const isRunning = await window.electronAPI.checkStatus(scriptName);
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
  
  checkbox.disabled = true; // prevent spam
  // Set cooldown so the periodic poll doesn't override this user action
  cooldowns.set(scriptName, Date.now() + COOLDOWN_MS);
  
  if (checkbox.checked) {
    try {
      const msg = await window.electronAPI.startScript(scriptName);
      setStatus(msg);
    } catch (error) {
      checkbox.checked = false; // revert on failure
      setStatus(`Error: ${error}`);
    }
  } else {
    try {
      const msg = await window.electronAPI.stopScript(scriptName);
      setStatus(msg);
    } catch (error) {
      checkbox.checked = true; // revert on failure
      setStatus(`Error: ${error}`);
    }
  }
  
  checkbox.disabled = false;
}

document.addEventListener('DOMContentLoaded', () => {
  const checkboxes = document.querySelectorAll('input[type="checkbox"]');
  checkboxes.forEach(checkbox => {
    // Initial state check
    updateToggleState(checkbox);
    
    // Add event listener
    checkbox.addEventListener('change', handleToggle);
  });
  
  // Periodically check status every 2 seconds to sync with external changes
  setInterval(() => {
    checkboxes.forEach(updateToggleState);
  }, 2000);
});
