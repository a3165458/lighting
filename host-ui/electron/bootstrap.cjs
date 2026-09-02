const { app } = require('electron')
const fs = require('node:fs')
const fsp = require('node:fs/promises')
const https = require('node:https')
const path = require('node:path')
const { execFile } = require('node:child_process')
const { promisify } = require('node:util')

const execFileAsync = promisify(execFile)

const PLATFORM_TOOLS_URL =
  'https://dl.google.com/android/repository/platform-tools-latest-windows.zip'
const FFMPEG_URL =
  'https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip'

/**
 * @typedef {{
 *   ready: boolean
 *   runtimeDir: string
 *   adbPath: string | null
 *   ffmpegPath: string | null
 *   phase: string
 *   detail: string
 *   error: string
 * }} BootstrapStatus
 */

/** @type {BootstrapStatus} */
let lastStatus = {
  ready: false,
  runtimeDir: '',
  adbPath: null,
  ffmpegPath: null,
  phase: 'idle',
  detail: '',
  error: '',
}

function runtimeRoot() {
  return path.join(app.getPath('userData'), 'runtime')
}

function setStatus(patch) {
  lastStatus = { ...lastStatus, ...patch, runtimeDir: runtimeRoot() }
  return lastStatus
}

function getStatus() {
  return { ...lastStatus, runtimeDir: runtimeRoot() }
}

function whichSync(name) {
  const pathEnv = process.env.PATH || ''
  const exts = (process.env.PATHEXT || '.EXE;.CMD;.BAT').split(';')
  for (const dir of pathEnv.split(path.delimiter)) {
    for (const ext of ['', ...exts]) {
      const full = path.join(dir, name + ext.toLowerCase())
      if (fs.existsSync(full)) return full
    }
    const plain = path.join(dir, name)
    if (fs.existsSync(plain)) return plain
  }
  return null
}

function findAdb(runtimeDir) {
  const bundled = [
    path.join(process.resourcesPath || '', 'platform-tools', 'adb.exe'),
    path.join(path.dirname(process.execPath), 'platform-tools', 'adb.exe'),
    path.join(runtimeDir, 'platform-tools', 'adb.exe'),
  ]
  for (const p of bundled) {
    if (p && fs.existsSync(p)) return p
  }
  return whichSync('adb.exe') || whichSync('adb')
}

function findFfmpeg(runtimeDir) {
  const bundled = [
    path.join(process.resourcesPath || '', 'ffmpeg', 'bin', 'ffmpeg.exe'),
    path.join(process.resourcesPath || '', 'ffmpeg.exe'),
    path.join(path.dirname(process.execPath), 'ffmpeg.exe'),
    path.join(runtimeDir, 'ffmpeg', 'bin', 'ffmpeg.exe'),
    path.join(runtimeDir, 'ffmpeg.exe'),
  ]
  for (const p of bundled) {
    if (p && fs.existsSync(p)) return p
  }
  return whichSync('ffmpeg.exe') || whichSync('ffmpeg')
}

function download(url, dest) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest)
    const go = (currentUrl, redirects) => {
      https
        .get(currentUrl, (res) => {
          if (
            res.statusCode &&
            res.statusCode >= 300 &&
            res.statusCode < 400 &&
            res.headers.location &&
            redirects < 5
          ) {
            res.resume()
            go(res.headers.location, redirects + 1)
            return
          }
          if (res.statusCode !== 200) {
            reject(new Error(`下载失败 HTTP ${res.statusCode}: ${currentUrl}`))
            res.resume()
            return
          }
          res.pipe(file)
          file.on('finish', () => file.close(() => resolve()))
        })
        .on('error', reject)
    }
    go(url, 0)
  })
}

async function extractZip(zipPath, destDir) {
  await fsp.mkdir(destDir, { recursive: true })
  // Windows 10+ tar can extract zip.
  await execFileAsync('tar', ['-xf', zipPath, '-C', destDir], {
    windowsHide: true,
  })
}

async function ensureAdb(runtimeDir) {
  const existing = findAdb(runtimeDir)
  if (existing) return existing

  setStatus({
    phase: 'adb',
    detail: '正在下载 USB 调试工具（adb）…',
    error: '',
  })
  const zip = path.join(runtimeDir, 'platform-tools.zip')
  await download(PLATFORM_TOOLS_URL, zip)
  await extractZip(zip, runtimeDir)
  try {
    await fsp.unlink(zip)
  } catch {
    /* ignore */
  }
  const adb = path.join(runtimeDir, 'platform-tools', 'adb.exe')
  if (!fs.existsSync(adb)) {
    throw new Error('adb 下载完成，但未找到 adb.exe')
  }
  return adb
}

async function ensureFfmpeg(runtimeDir) {
  const existing = findFfmpeg(runtimeDir)
  if (existing) return existing

  setStatus({
    phase: 'ffmpeg',
    detail: '正在下载画面编码组件（ffmpeg）…',
    error: '',
  })
  const zip = path.join(runtimeDir, 'ffmpeg.zip')
  await download(FFMPEG_URL, zip)
  const extractTo = path.join(runtimeDir, 'ffmpeg-extract')
  await fsp.rm(extractTo, { recursive: true, force: true })
  await extractZip(zip, extractTo)
  try {
    await fsp.unlink(zip)
  } catch {
    /* ignore */
  }

  // Flatten **/bin/ffmpeg.exe into runtime/ffmpeg/bin
  const stack = [extractTo]
  let found = null
  while (stack.length) {
    const dir = stack.pop()
    const entries = await fsp.readdir(dir, { withFileTypes: true })
    for (const ent of entries) {
      const full = path.join(dir, ent.name)
      if (ent.isDirectory()) stack.push(full)
      else if (ent.name.toLowerCase() === 'ffmpeg.exe') found = full
    }
  }
  if (!found) {
    throw new Error('ffmpeg 下载完成，但未找到 ffmpeg.exe')
  }
  const binDir = path.join(runtimeDir, 'ffmpeg', 'bin')
  await fsp.mkdir(binDir, { recursive: true })
  const target = path.join(binDir, 'ffmpeg.exe')
  await fsp.copyFile(found, target)
  // Also copy ffprobe if present next to it.
  const probeSrc = path.join(path.dirname(found), 'ffprobe.exe')
  if (fs.existsSync(probeSrc)) {
    await fsp.copyFile(probeSrc, path.join(binDir, 'ffprobe.exe'))
  }
  await fsp.rm(extractTo, { recursive: true, force: true })
  return target
}

/**
 * First-run friendly bootstrap for beginners.
 * Downloads adb + ffmpeg into %APPDATA%/Lighting副屏/runtime when missing.
 */
async function ensureRuntime() {
  const runtimeDir = runtimeRoot()
  await fsp.mkdir(runtimeDir, { recursive: true })
  setStatus({
    ready: false,
    phase: 'prepare',
    detail: '正在准备运行环境…',
    error: '',
  })

  try {
    const adbPath = await ensureAdb(runtimeDir)
    setStatus({ adbPath, detail: 'USB 工具已就绪' })
    const ffmpegPath = await ensureFfmpeg(runtimeDir)
    setStatus({
      ready: true,
      adbPath,
      ffmpegPath,
      phase: 'ready',
      detail: '环境已就绪，可以开始使用',
      error: '',
    })
    return getStatus()
  } catch (err) {
    setStatus({
      ready: false,
      phase: 'error',
      detail: '环境准备失败',
      error: String(err.message || err),
    })
    throw err
  }
}

function runtimeEnv() {
  const status = getStatus()
  const pathParts = []
  if (status.adbPath) pathParts.push(path.dirname(status.adbPath))
  if (status.ffmpegPath) pathParts.push(path.dirname(status.ffmpegPath))
  pathParts.push(process.env.PATH || '')
  return {
    ...process.env,
    PATH: pathParts.join(path.delimiter),
    LIGHTING_RUNTIME_DIR: status.runtimeDir,
    ANDROID_HOME: status.runtimeDir,
  }
}

module.exports = {
  ensureRuntime,
  getStatus,
  runtimeEnv,
  runtimeRoot,
}
