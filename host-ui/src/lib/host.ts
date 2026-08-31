import type { LightingDesktopApi } from '@/lib/desktop'

export type HostSettings = {
  selectedDisplay: number
  selectedDevice: number
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
}

export type HostDevice = {
  id: string
  label: string
  serial: string
  state: string
  clientInstalled?: boolean | null
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
  canInstallApk: boolean
  installInflight: boolean
  multiDevice: boolean
  displays: HostDisplay[]
  devices: HostDevice[]
  settings: HostSettings | null
  lastError: string
  hostVersion: string
  bootstrap?: BootstrapStatus
}

export type HostSettingsPatch = Partial<{
  selectedDisplay: number
  selectedDevice: number
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
  canInstallApk: false,
  installInflight: false,
  multiDevice: false,
  displays: [],
  devices: [],
  settings: null,
  lastError: '',
  hostVersion: '',
}
