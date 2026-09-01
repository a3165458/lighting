import type { LightingDesktopApi } from '@/lib/desktop'

export type ShareMode = 'mirror' | 'extend' | 'external'

export type HostSettings = {
  selectedDisplay: number
  selectedDevice: number
  shareMode: ShareMode | string
  qualityPct: number
  fps: number
  bitrateKbps: number
  sendAudio: boolean
  preferHevc: boolean
  resCap: string
  touchRelay: boolean
  keyboardRelay: boolean
  bindHost: string
  bindPort: number
}

export type HostDisplay = {
  id: string
  label: string
  primary: boolean
  width: number
  height: number
  virtualDisplay?: boolean
}

export type HostDevice = {
  id: string
  label: string
  serial: string
  state: string
  clientInstalled?: boolean | null
  clientVersion?: string | null
}

export type BootstrapStatus = {
  ready: boolean
  runtimeDir: string
  adbPath: string | null
  ffmpegPath: string | null
  phase: string
  detail: string
  error: string
}

export type HostState = {
  connected: boolean
  sharing: boolean
  phase: string
  detail: string
  transport: string
  clientName: string
  clientAddr: string
  codec: string
  frames: number
  bitrateKbps: number
  latencyMs: number
  lossPermille: number
  bytesSent: number
  connectedSecs: number
  usbHint: string
  usbTone: string
  deviceDetected: boolean
  clientAppMissing: boolean
  clientAppVersion: string
  canInstallApk: boolean
  installInflight: boolean
  multiDevice: boolean
  displays: HostDisplay[]
  devices: HostDevice[]
  settings: HostSettings | null
  lastError: string
  hostVersion: string
  appVersion?: string
  bootstrap?: BootstrapStatus
}

export type HostSettingsPatch = Partial<{
  selectedDisplay: number
  selectedDevice: number
  shareMode: ShareMode | string
  qualityPct: number
  fps: number
  bitrateKbps: number
  sendAudio: boolean
  preferHevc: boolean
  resCap: string
  touchRelay: boolean
  keyboardRelay: boolean
  bindHost: string
  bindPort: number
}>

export type LightingHostApi = {
  getState: () => Promise<HostState>
  getBootstrap?: () => Promise<BootstrapStatus>
  retryBootstrap?: () => Promise<BootstrapStatus>
  refresh: () => Promise<HostState>
  startShare: () => Promise<HostState>
  stopShare: () => Promise<HostState>
  setSettings: (patch: HostSettingsPatch) => Promise<HostState>
  installClient: () => Promise<HostState>
  ping: () => Promise<{ pong: boolean }>
}


declare global {
  interface Window {
    lightingDesktop?: LightingDesktopApi
    lightingHost?: LightingHostApi
  }
}

export function hasHostBridge(): boolean {
  return typeof window !== 'undefined' && Boolean(window.lightingHost)
}

export const SHARE_MODE_OPTIONS = [
  {
    id: 'mirror',
    label: '镜像主屏',
    hint: '与主屏同画面，并自动缩放到平板分辨率。',
  },
  {
    id: 'extend',
    label: '扩展屏（推荐）',
    hint: '创建虚拟扩展桌面（与华硕 GlideX 同类）。平板按自身分辨率输出；失败时会自动改用镜像。',
  },
  {
    id: 'external',
    label: '仅投扩展屏',
    hint: '只把扩展桌面投到平板（按平板分辨率 1:1）。不会关掉电脑主屏。',
  },
] as const

export const DISCONNECTED_STATE: HostState = {
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
  usbHint: '正在准备一键运行环境…',
  usbTone: 'info',
  deviceDetected: false,
  clientAppMissing: false,
  clientAppVersion: '',
  canInstallApk: false,
  installInflight: false,
  multiDevice: false,
  displays: [],
  devices: [],
  settings: null,
  lastError: '',
  hostVersion: '',
  appVersion: '',
}
