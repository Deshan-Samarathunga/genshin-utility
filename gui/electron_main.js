const { app, BrowserWindow, ipcMain, Tray, Menu, nativeImage } = require('electron');
const path = require('path');
const { exec, spawn } = require('child_process');
const fs = require('fs');

let mainWindow = null;
let tray = null;

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

// IPC Handlers

ipcMain.handle('start-script', async (event, scriptName) => {
  return new Promise((resolve, reject) => {
    // In production, extraResources are placed in process.resourcesPath. In dev, they are in the parent directory.
    const getAppPath = () => app.isPackaged ? process.resourcesPath : path.join(__dirname, '..');
    const scriptPath = path.join(getAppPath(), 'ahk', scriptName);
    
    if (!fs.existsSync(scriptPath)) {
      return reject(`Script not found: ${scriptPath}`);
    }

    // Windows start command
    exec(`start "" "${scriptPath}"`, (error) => {
      if (error) {
        reject(error.message);
      } else {
        resolve(`Started ${scriptName}`);
      }
    });
  });
});

ipcMain.handle('stop-script', async (event, scriptName) => {
  return new Promise((resolve, reject) => {
    const query = `name='${scriptName}'`;
    exec(`wmic process where "${query}" call terminate`, (error, stdout) => {
      if (error) {
        reject(error.message);
      } else {
        if (stdout.includes("ReturnValue = 0")) {
          resolve(`Stopped ${scriptName}`);
        } else {
          resolve(`Stopped ${scriptName} (might not be running)`);
        }
      }
    });
  });
});

ipcMain.handle('check-status', async (event, scriptName) => {
  return new Promise((resolve, reject) => {
    const query = `name='${scriptName}'`;
    exec(`wmic process where "${query}" get processid`, (error, stdout) => {
      if (error) {
        resolve(false);
      } else {
        const isRunning = !stdout.includes("No Instance(s) Available.") && stdout.includes("ProcessId");
        resolve(isRunning);
      }
    });
  });
});
