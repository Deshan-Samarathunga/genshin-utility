const { app, BrowserWindow, ipcMain, Tray, Menu, nativeImage } = require('electron');
const path = require('path');
const { execFile, execFileSync, spawn } = require('child_process');
const fs = require('fs');

let mainWindow = null;
let tray = null;

// ---------------------------------------------------------------------------
//  Administrator elevation
// ---------------------------------------------------------------------------
//  The AutoHotkey scripts drive Genshin Impact, which runs at a raised
//  integrity level because of its anti-cheat. Windows UIPI silently drops
//  synthetic keyboard/mouse input sent from a lower-integrity process, so the
//  scripts only work when they run elevated ("as administrator"). Rather than
//  prompt for every script, we elevate the whole app once at startup; the
//  scripts we spawn then inherit administrator rights automatically.

function isElevated() {
  if (process.platform !== 'win32') return true;
  try {
    // `fltmc` requires admin and, unlike `net session`, doesn't depend on any
    // Windows service being running, which makes it a reliable elevation probe.
    execFileSync('fltmc', ['filters'], { stdio: 'ignore', windowsHide: true });
    return true;
  } catch (_) {
    return false;
  }
}

function runPowerShell(psScript) {
  // -EncodedCommand takes base64 UTF-16LE, which sidesteps all the quoting
  // pitfalls of passing a complex command string through argv.
  const encoded = Buffer.from(psScript, 'utf16le').toString('base64');
  const args = ['-NoProfile', '-NonInteractive', '-EncodedCommand', encoded];
  return new Promise((resolve) => {
    execFile(
      'powershell.exe',
      args,
      { windowsHide: true, encoding: 'utf8', maxBuffer: 1024 * 1024 },
      (err, stdout) => resolve(err || !stdout ? '' : stdout)
    );
  });
}

function relaunchElevated() {
  const exe = process.execPath;
  // Packaged: execPath is our own exe. Dev: execPath is electron.exe, so we
  // must also hand it the app directory to load. `--relaunched` is a sentinel
  // that stops us re-elevating forever if the probe above ever false-negatives.
  const relaunchArgs = app.isPackaged
    ? ['--relaunched']
    : [app.getAppPath(), '--relaunched'];
  // PowerShell's -ArgumentList joins array items with spaces before passing
  // them to the target process.  Paths like "C:\Users\Deshan Samarathunga\…"
  // would be split into two argv entries.  Passing a single string with
  // embedded double-quotes keeps each argument intact for the target process.
  const q = (s) => `'${String(s).replace(/'/g, "''")}'`;       // PS string literal
  const innerArgs = relaunchArgs.map((a) => `"${a}"`).join(' ');
  const psCommand =
    `Start-Process -FilePath ${q(exe)} -ArgumentList '${innerArgs}' -Verb RunAs`;

  // Spawn PowerShell WITHOUT -NonInteractive so the UAC dialog can appear.
  // We wait for it to exit before quitting, so the elevated child is fully
  // launched before this instance tears down.
  const child = spawn('powershell.exe', ['-NoProfile', '-Command', psCommand], {
    stdio: 'ignore',
    windowsHide: true
  });
  child.on('exit', () => {
    app.quit();
  });
  // Safety: if PowerShell hangs for >15s, quit anyway
  setTimeout(() => app.quit(), 15000);
}

// ---------------------------------------------------------------------------
//  AutoHotkey discovery + process tracking
// ---------------------------------------------------------------------------

let cachedAhkExe;
function resolveAhkExe() {
  if (cachedAhkExe !== undefined) return cachedAhkExe;
  const local = process.env.LOCALAPPDATA || '';
  const candidates = [
    'C:\\Program Files\\AutoHotkey\\v2\\AutoHotkey64.exe',
    'C:\\Program Files\\AutoHotkey\\v2\\AutoHotkey32.exe',
    'C:\\Program Files (x86)\\AutoHotkey\\v2\\AutoHotkey64.exe',
    local && path.join(local, 'Programs', 'AutoHotkey', 'v2', 'AutoHotkey64.exe'),
    'C:\\Program Files\\AutoHotkey\\AutoHotkey.exe' // older single-binary install
  ].filter(Boolean);
  cachedAhkExe = candidates.find((c) => fs.existsSync(c)) || null;
  return cachedAhkExe;
}

function getAhkDir() {
  // Packaged builds copy ../ahk into resources via extraResources; in dev the
  // scripts sit next to the gui folder.
  return app.isPackaged
    ? path.join(process.resourcesPath, 'ahk')
    : path.join(__dirname, '..', 'ahk');
}

// Enumerate running AutoHotkey processes (any version) with their command
// lines so a script can be matched by its file path. Cached briefly because
// the renderer polls status for every toggle every couple of seconds.
let procCache = { at: 0, promise: null };
function invalidateProcCache() {
  procCache = { at: 0, promise: null };
}

function listAhkProcesses() {
  const now = Date.now();
  if (procCache.promise && now - procCache.at < 800) return procCache.promise;
  procCache = {
    at: now,
    promise: (async () => {
      const stdout = await runPowerShell(
        "Get-CimInstance Win32_Process -Filter \"Name LIKE 'AutoHotkey%'\" | " +
          "ForEach-Object { \"$($_.ProcessId)<<>>$($_.CommandLine)\" }"
      );
      return stdout
        .split(/\r?\n/)
        .filter(Boolean)
        .map((line) => {
          const i = line.indexOf('<<>>');
          return i === -1
            ? { pid: line.trim(), cmd: '' }
            : { pid: line.slice(0, i).trim(), cmd: line.slice(i + 4) };
        });
    })()
  };
  return procCache.promise;
}

function pidsForScript(rows, scriptName) {
  const needle = scriptName.toLowerCase();
  return rows
    .filter((r) => r.cmd.toLowerCase().includes(needle))
    .map((r) => r.pid)
    .filter(Boolean);
}

// ---------------------------------------------------------------------------
//  Window + tray
// ---------------------------------------------------------------------------

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 600,
    height: 600,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      nodeIntegration: false,
      contextIsolation: true
    },
    autoHideMenuBar: true,
    title: "Genshin Impact Utility"
  });

  mainWindow.loadFile(path.join(__dirname, 'src', 'index.html'));

  mainWindow.on('minimize', (event) => {
    event.preventDefault();
    mainWindow.hide();
  });

  mainWindow.on('close', (event) => {
    if (!app.isQuitting) {
      event.preventDefault();
      mainWindow.hide();
    }
    return false;
  });
}

app.setPath('userData', path.join(app.getPath('appData'), 'genshin-ahk-gui-v2'));

app.whenReady().then(() => {
  // Gate everything behind elevation. If we're not admin, relaunch elevated
  // (one UAC prompt) and quit this instance so only the elevated one runs.
  if (process.platform === 'win32' && !isElevated() && !process.argv.includes('--relaunched')) {
    try {
      relaunchElevated();
      // relaunchElevated() will call app.quit() after the UAC dialog completes.
      // We just return here to prevent the rest of the init from running.
      return;
    } catch (_) {
      // If the relaunch itself fails, fall through and run un-elevated rather
      // than leaving the user with nothing.
    }
  }

  createWindow();

  // Load the physical icon file generated
  const iconPath = path.join(__dirname, 'icon.png');
  const icon = nativeImage.createFromPath(iconPath);
  tray = new Tray(icon);
  tray.setToolTip('Genshin Impact Utility');

  const contextMenu = Menu.buildFromTemplate([
    { label: 'Show App', click: () => { mainWindow.show(); } },
    { label: 'Quit', click: () => { app.isQuitting = true; app.quit(); } }
  ]);

  tray.setContextMenu(contextMenu);
  tray.on('double-click', () => {
    mainWindow.show();
  });

  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) {
      createWindow();
    }
  });
});

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit();
  }
});

// ---------------------------------------------------------------------------
//  IPC Handlers
// ---------------------------------------------------------------------------

ipcMain.handle('start-script', async (event, scriptName) => {
  const scriptPath = path.join(getAhkDir(), scriptName);
  if (!fs.existsSync(scriptPath)) {
    throw new Error(`Script not found: ${scriptPath}`);
  }

  const ahk = resolveAhkExe();
  if (!ahk) {
    throw new Error('AutoHotkey v2 not found. Please install it from autohotkey.com.');
  }

  return await new Promise((resolve, reject) => {
    // The app is already elevated, so this child inherits administrator rights
    // and its input reaches the elevated game window.
    const child = spawn(ahk, [scriptPath], {
      detached: true,
      stdio: 'ignore',
      windowsHide: true
    });
    child.on('error', (err) => reject(err.message));
    child.on('spawn', () => {
      child.unref();
      invalidateProcCache(); // let the next status poll see it immediately
      resolve(`Started ${scriptName}`);
    });
  });
});

ipcMain.handle('stop-script', async (event, scriptName) => {
  const rows = await listAhkProcesses();
  const pids = pidsForScript(rows, scriptName);
  if (pids.length === 0) {
    return `Stopped ${scriptName} (was not running)`;
  }

  await Promise.all(
    pids.map(
      (pid) =>
        new Promise((resolve) => {
          // /T also kills any child interpreter; /F forces it. This works
          // because we run elevated and can terminate elevated script processes.
          execFile('taskkill', ['/PID', pid, '/T', '/F'], { windowsHide: true }, () =>
            resolve()
          );
        })
    )
  );
  invalidateProcCache();
  return `Stopped ${scriptName}`;
});

ipcMain.handle('check-status', async (event, scriptName) => {
  const rows = await listAhkProcesses();
  return pidsForScript(rows, scriptName).length > 0;
});
