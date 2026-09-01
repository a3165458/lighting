const net = require('node:net')
const { spawn, execFile } = require('node:child_process')
const fs = require('node:fs')
const path = require('node:path')
const { promisify } = require('node:util')
const { app } = require('electron')
const bootstrap = require('./bootstrap.cjs')

const execFileAsync = promisify(execFile)

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
    this._aligningVersion = false
  }

  async ensureConnected() {
    if (!(this.socket && this.connected)) {
      await this.startHostIfNeeded()
      await this.connectWithRetry(30, 300)
    }
    if (!this._aligningVersion) {
      await this.alignHostVersion()
    }
  }

  /**
   * Packaged Electron apps must not keep talking to a leftover older
   * lighting-host.exe still bound to 17401 (that is why UI showed v0.1.0
   * after installing 0.1.2).
   */
  async alignHostVersion() {
    this._aligningVersion = true
    try {
      const expected = app.getVersion()
      const state = await this.rawInvoke('getState')
      const hostVersion = String(state?.hostVersion || '')
      if (hostVersion && expected && hostVersion !== expected) {
        await this.replaceStaleHost()
      }
    } catch {
      /* first connect races are fine; next poll retries */
    } finally {
      this._aligningVersion = false
    }
  }

  async replaceStaleHost() {
    this.disconnectSocket()
    if (this.child && !this.child.killed) {
      try {
        this.child.kill()
      } catch {
        /* ignore */
      }
      this.child = null
    }
    await this.killStrayHosts()
    await this.spawnBundledHost()
    await this.connectWithRetry(40, 250)
  }

  async killStrayHosts() {
    if (process.platform !== 'win32') return
    try {
      await execFileAsync('taskkill', ['/F', '/IM', 'lighting-host.exe'], {
        windowsHide: true,
      })
    } catch {
      /* nothing to kill */
    }
    await new Promise((r) => setTimeout(r, 400))
  }

  hostCandidates() {
    const fromEnv = process.env.LIGHTING_HOST_PATH
    const resources = process.resourcesPath
      ? path.join(process.resourcesPath, 'lighting-host.exe')
      : null
    const nearApp = app.isPackaged
      ? path.join(path.dirname(process.execPath), 'lighting-host.exe')
      : null
    const nearDevRelease = path.resolve(
      __dirname,
      '..',
      '..',
      'host-windows',
      'target',
      'release',
      'lighting-host.exe',
    )
    const nearDevDebug = path.resolve(
      __dirname,
      '..',
      '..',
      'host-windows',
      'target',
      'debug',
      'lighting-host.exe',
    )
    const nearResourcesDev = path.resolve(__dirname, '..', 'resources', 'lighting-host.exe')
    return [fromEnv, resources, nearApp, nearResourcesDev, nearDevRelease, nearDevDebug].filter(
      Boolean,
    )
  }

  resolveHostExe() {
    return this.hostCandidates().find((p) => {
      try {
        return fs.existsSync(p)
      } catch {
        return false
      }
    })
  }

  async startHostIfNeeded() {
    if (await this.canConnectOnce(150)) return
    await this.spawnBundledHost()
  }

  async spawnBundledHost() {
    const exe = this.resolveHostExe()
    if (!exe) {
      throw new Error(
        '未找到主机组件 lighting-host.exe。请使用官方便携包，或先运行打包脚本。',
      )
    }
    if (this.child && !this.child.killed) return

    const resourcesDir =
      process.resourcesPath || path.dirname(exe)

    const env = {
      ...bootstrap.runtimeEnv(),
      [PORT_ENV]: String(this.port),
      LIGHTING_RESOURCES_DIR: resourcesDir,
    }

    this.child = spawn(exe, ['--ipc-only'], {
      windowsHide: true,
      stdio: 'ignore',
      env,
      cwd: path.dirname(exe),
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
    throw lastErr || new Error('无法连接本地主机服务')
  }

  disconnectSocket() {
    try {
      this.socket?.destroy()
    } catch {
      /* ignore */
    }
    this.socket = null
    this.connected = false
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
          p.reject(new Error('与主机断开连接'))
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
      else pending.reject(new Error(msg.error || '主机返回错误'))
    }
  }

  rawInvoke(method, params = {}) {
    return new Promise((resolve, reject) => {
      if (!this.socket) {
        reject(new Error('尚未连接主机'))
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
    })
  }

  invoke(method, params = {}) {
    return this.ensureConnected().then(() => this.rawInvoke(method, params))
  }

  async getState() {
    try {
      const state = await this.invoke('getState')
      return {
        ...state,
        connected: true,
        appVersion: app.getVersion(),
        clientAppVersion: state.clientAppVersion || '',
      }
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
        clientAppVersion: '',
        canInstallApk: false,
        installInflight: false,
        multiDevice: false,
        displays: [],
        devices: [],
        settings: null,
        lastError: String(err.message || err),
        hostVersion: '',
        appVersion: app.getVersion(),
      }
    }
  }

  dispose() {
    this.disconnectSocket()
    if (this.child && !this.child.killed) {
      try {
        this.child.kill()
      } catch {
        /* ignore */
      }
    }
    this.child = null
  }
}

module.exports = { HostIpcClient, DEFAULT_PORT }
