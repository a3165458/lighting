import type { LightingDesktopApi } from '@/lib/desktop'

export type ShareMode = 'mirror' | 'extend' | 'external'

export type ActivityStep = {
  id: string
  label: string
  state: string
}

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
  activityTitle?: string
  activityDetail?: string
  activitySteps?: ActivityStep[]
  hostElevated?: boolean
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
    label: '镜像主屏（按平板分辨率）',
    hint: '镜像电脑主屏并按平板分辨率输出。免驱动；「跟随平板」时会临时切电脑分辨率。锁屏后无法抓屏。',
  },
  {
    id: 'extend',
    label: '双屏扩展（电脑屏仍亮）',
    hint: '平板作为独立桌面（类似 GlideX）。虚拟屏 1:1 抓取，电脑屏继续亮。适合坐在电脑前。',
  },
  {
    id: 'external',
    label: '仅平板（电脑屏关闭·躺着用）',
    hint: '虚拟屏 1:1 铺满平板，平板连上后关掉电脑屏。投屏期间防休眠；合盖请设为不采取任何操作。不要锁屏。',
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
  activityTitle: '',
  activityDetail: '',
  activitySteps: [],
  hostElevated: undefined,
}
