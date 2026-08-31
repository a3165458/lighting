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
    hint: '与主屏同画面，并自动缩放到平板分辨率（不会把 2K 硬塞给非 2K 平板）。',
  },
  {
    id: 'extend',
    label: '扩展屏（推荐）',
    hint: '独立扩展桌面。首次会自动准备虚拟屏（可能弹出一次管理员确认），之后也可用 Win+P「扩展」。',
  },
  {
    id: 'external',
    label: '仅第二屏',
    hint: '桌面只出现在扩展屏/平板（相当于 Win+P「仅第二屏幕」）。首次同样会自动准备。',
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
