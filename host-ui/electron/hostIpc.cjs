const net = require('node:net')
const { spawn } = require('node:child_process')
const fs = require('node:fs')
const path = require('node:path')
const { app } = require('electron')

const DEFAULT_PORT = 17401
const PORT_ENV = 'LIGHTING_IPC_PORT'

class HostIpcClient {
  constructor() {
    /** @type {import('node:net').Socket | null} */
    this.socket = null
    /** @type {import('node:child_process').ChildProcess | null} */
    this.child = null
    this.buffer = ''
    this.nextId = 1
    /** @type {Map<number, {resolve: Function, reject: Function}>} */
    this.pending = new Map()
    this.port = Number(process.env[PORT_ENV] || DEFAULT_PORT)
    this.connected = false
  }

  async ensureConnected() {
    if (this.socket && this.connected) return
    await this.startHostIfNeeded()
    await this.connectWithRetry(20, 250)
  }

  hostCandidates() {
    const fromEnv = process.env.LIGHTING_HOST_PATH
    const resources = process.resourcesPath
      ? path.join(process.resourcesPath, 'lighting-host.exe')
      : null
    const nearApp = app.isPackaged
      ? path.join(path.dirname(process.execPath), 'lighting-host.exe')
      : path.resolve(__dirname, '..', '..', 'host-windows', 'target', 'release', 'lighting-host.exe')
    const nearDev = path.resolve(
      __dirname,
      '..',
      '..',
      'host-windows',
      'target',
      'debug',
      'lighting-host.exe',
    )
    return [fromEnv, resources, nearApp, nearDev].filter(Boolean)
  }

  async startHostIfNeeded() {
    if (await this.canConnectOnce(150)) return

    const exe = this.hostCandidates().find((p) => {
      try {
        return fs.existsSync(p)
      } catch {
        return false
      }
    })
    if (!exe) {
      throw new Error(
        'lighting-host.exe not found. Build host-windows first, or set LIGHTING_HOST_PATH.',
      )
    }

    if (this.child && !this.child.killed) return

    this.child = spawn(exe, ['--ipc-only'], {
      windowsHide: true,
      stdio: 'ignore',
      env: {
        ...process.env,
        [PORT_ENV]: String(this.port),
      },
    })
    this.child.on('exit', () => {
      this.child = null
      this.connected = false
    })
  }

  canConnectOnce(timeoutMs) {
    return new Promise((resolve) => {
      const socket = net.connect({ host: '127.0.0.1', port: this.port })
      const timer = setTimeout(() => {
        socket.destroy()
        resolve(false)
      }, timeoutMs)
      socket.once('connect', () => {
        clearTimeout(timer)
        socket.end()
        resolve(true)
      })
      socket.once('error', () => {
        clearTimeout(timer)
        resolve(false)
      })
    })
  }

  async connectWithRetry(attempts, delayMs) {
    let lastErr = null
    for (let i = 0; i < attempts; i += 1) {
      try {
        await this.connect()
        return
      } catch (err) {
        lastErr = err
        await new Promise((r) => setTimeout(r, delayMs))
      }
    }
    throw lastErr || new Error('IPC connect failed')
  }

  connect() {
    return new Promise((resolve, reject) => {
      if (this.socket) {
        this.socket.destroy()
        this.socket = null
      }
      const socket = net.connect({ host: '127.0.0.1', port: this.port })
      socket.setEncoding('utf8')
      socket.on('data', (chunk) => this.onData(chunk))
      socket.on('error', (err) => {
        this.connected = false
        reject(err)
      })
      socket.on('close', () => {
        this.connected = false
        this.socket = null
        for (const [, p] of this.pending) {
          p.reject(new Error('IPC disconnected'))
        }
        this.pending.clear()
      })
      socket.on('connect', () => {
        this.socket = socket
        this.connected = true
        resolve()
      })
    })
  }

  onData(chunk) {
    this.buffer += chunk
    let idx
    while ((idx = this.buffer.indexOf('\n')) >= 0) {
      const line = this.buffer.slice(0, idx).trim()
      this.buffer = this.buffer.slice(idx + 1)
      if (!line) continue
      let msg
      try {
        msg = JSON.parse(line)
      } catch {
        continue
      }
      const pending = this.pending.get(msg.id)
      if (!pending) continue
      this.pending.delete(msg.id)
      if (msg.ok) pending.resolve(msg.result)
      else pending.reject(new Error(msg.error || 'IPC error'))
    }
  }

  invoke(method, params = {}) {
    return this.ensureConnected().then(
      () =>
        new Promise((resolve, reject) => {
          if (!this.socket) {
            reject(new Error('IPC not connected'))
            return
          }
          const id = this.nextId++
          this.pending.set(id, { resolve, reject })
          const payload = `${JSON.stringify({ id, method, params })}\n`
          this.socket.write(payload, (err) => {
            if (err) {
              this.pending.delete(id)
              reject(err)
            }
          })
        }),
    )
  }

  async getState() {
    try {
      return await this.invoke('getState')
    } catch (err) {
      return {
        connected: false,
        sharing: false,
        phase: '',
        detail: '',
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
        usbHint: String(err.message || err),
        usbTone: 'warn',
        deviceDetected: false,
        clientAppMissing: false,
        canInstallApk: false,
        installInflight: false,
        multiDevice: false,
        displays: [],
        devices: [],
        settings: null,
        lastError: String(err.message || err),
        hostVersion: '',
      }
    }
  }

  dispose() {
    try {
      this.socket?.destroy()
    } catch {
      /* ignore */
    }
    this.socket = null
    if (this.child && !this.child.killed) {
      this.child.kill()
    }
    this.child = null
  }
}

module.exports = { HostIpcClient, DEFAULT_PORT }
