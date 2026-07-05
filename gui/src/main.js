const statusBar = document.getElementById('status-bar');

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
