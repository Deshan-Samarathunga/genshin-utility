const { contextBridge, ipcRenderer } = require('electron');

contextBridge.exposeInMainWorld('electronAPI', {
  startScript: (scriptName) => ipcRenderer.invoke('start-script', scriptName),
  stopScript: (scriptName) => ipcRenderer.invoke('stop-script', scriptName),
  checkStatus: (scriptName) => ipcRenderer.invoke('check-status', scriptName)
});
