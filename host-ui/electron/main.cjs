const { app, BrowserWindow, ipcMain, shell } = require('electron')
const path = require('node:path')
const { HostIpcClient } = require('./hostIpc.cjs')
const bootstrap = require('./bootstrap.cjs')

const isDev = !app.isPackaged
const WINDOW_WIDTH = 1440
const WINDOW_HEIGHT = 900
const MIN_WIDTH = 1280
const MIN_HEIGHT = 720

/** @type {BrowserWindow | null} */
let mainWindow = null
const host = new HostIpcClient()
/** @type {Promise<unknown> | null} */
let bootPromise = null

function createWindow() {
  mainWindow = new BrowserWindow({
    width: WINDOW_WIDTH,
    height: WINDOW_HEIGHT,
    minWidth: MIN_WIDTH,
    minHeight: MIN_HEIGHT,
    show: false,
    frame: false,
    backgroundColor: '#F6F7FC',
    title: 'Lighting 副屏',
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      nodeIntegration: false,
      sandbox: false,
    },
  })

  mainWindow.once('ready-to-show', () => {
    mainWindow?.show()
  })

  mainWindow.webContents.setWindowOpenHandler(({ url }) => {
    shell.openExternal(url)
    return { action: 'deny' }
  })

  if (isDev) {
    const url = process.env.VITE_DEV_SERVER_URL || 'http://127.0.0.1:5173'
    mainWindow.loadURL(url)
  } else {
    mainWindow.loadFile(path.join(__dirname, '..', 'dist', 'index.html'))
  }

  mainWindow.on('closed', () => {
    mainWindow = null
  })
}

async function bootPipeline() {
  await bootstrap.ensureRuntime()
  await host.ensureConnected()
}

app.whenReady().then(() => {
  createWindow()
  bootPromise = bootPipeline().catch((err) => {
    console.warn('[lighting-bootstrap]', err.message || err)
  })
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  host.dispose()
  if (process.platform !== 'darwin') app.quit()
})

app.on('before-quit', () => {
  host.dispose()
})

ipcMain.handle('window:minimize', () => {
  mainWindow?.minimize()
})

ipcMain.handle('window:maximize', () => {
  if (!mainWindow) return false
  if (mainWindow.isMaximized()) {
    mainWindow.unmaximize()
    return false
  }
  mainWindow.maximize()
  return true
})

ipcMain.handle('window:close', () => {
  mainWindow?.close()
})

ipcMain.handle('window:isMaximized', () => {
  return Boolean(mainWindow?.isMaximized())
})

ipcMain.handle('host:getBootstrap', async () => bootstrap.getStatus())

ipcMain.handle('host:getState', async () => {
  if (bootPromise) {
    try {
      await bootPromise
    } catch {
      /* status exposed via bootstrap */
    }
  }
  const boot = bootstrap.getStatus()
  if (!boot.ready) {
    return {
      connected: false,
      sharing: false,
      phase: boot.phase,
      detail: boot.detail,
      transport: '',
      clientName: '',
      clientAddr: '',
      codec: '',
      frames: 0,
      bitrateKbps: 0,
      latencyMs: 0,
      lossPermille: 0,
      bytesSent: 0,
      connectedSecs: 0,
      usbHint: boot.error || boot.detail || '正在准备运行环境…',
      usbTone: boot.phase === 'error' ? 'bad' : 'info',
      deviceDetected: false,
      clientAppMissing: false,
      canInstallApk: false,
      installInflight: false,
      multiDevice: false,
      displays: [],
      devices: [],
      settings: null,
      lastError: boot.error || '',
      hostVersion: '',
      bootstrap: boot,
    }
  }
  const state = await host.getState()
  return { ...state, bootstrap: boot }
})

ipcMain.handle('host:refresh', async () => host.invoke('refresh'))
ipcMain.handle('host:startShare', async () => host.invoke('startShare'))
ipcMain.handle('host:stopShare', async () => host.invoke('stopShare'))
ipcMain.handle('host:setSettings', async (_e, patch) => host.invoke('setSettings', patch))
ipcMain.handle('host:installClient', async () => host.invoke('installClient'))
ipcMain.handle('host:ping', async () => host.invoke('ping'))
ipcMain.handle('host:retryBootstrap', async () => {
  bootPromise = bootPipeline()
  await bootPromise
  return bootstrap.getStatus()
})
