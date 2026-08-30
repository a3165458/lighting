const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('lightingDesktop', {
  platform: process.platform,
  minimize: () => ipcRenderer.invoke('window:minimize'),
  maximize: () => ipcRenderer.invoke('window:maximize'),
  close: () => ipcRenderer.invoke('window:close'),
  isMaximized: () => ipcRenderer.invoke('window:isMaximized'),
})

contextBridge.exposeInMainWorld('lightingHost', {
  getState: () => ipcRenderer.invoke('host:getState'),
  refresh: () => ipcRenderer.invoke('host:refresh'),
  startShare: () => ipcRenderer.invoke('host:startShare'),
  stopShare: () => ipcRenderer.invoke('host:stopShare'),
  setSettings: (patch) => ipcRenderer.invoke('host:setSettings', patch),
  installClient: () => ipcRenderer.invoke('host:installClient'),
  ping: () => ipcRenderer.invoke('host:ping'),
})
