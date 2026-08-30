export type NavId = 'home' | 'settings' | 'shortcuts' | 'about'

export type DisplayOption = {
  id: string
  label: string
}

export type PerformanceMetric = {
  id: string
  label: string
  value: string
  icon: 'link' | 'video' | 'zap' | 'target'
  tint: 'purple' | 'blue' | 'pink' | 'mint'
}

export type AppSettings = {
  displayId: string
  quality: number
  fps: number
  bitrate: number
  audioSync: boolean
  touchRelay: boolean
  keyboardRelay: boolean
}

export type SessionState = {
  sharing: boolean
  deviceDetected: boolean
  bytesSent: number
  elapsedSecs: number
}

export const DISPLAY_OPTIONS: DisplayOption[] = [
  { id: '1', label: '主显示器 #1 （2560 × 1440）' },
  { id: '2', label: '显示器 #2 （1920 × 1080）' },
  { id: '3', label: '显示器 #3 （3840 × 2160）' },
]

export const NAV_ITEMS: { id: NavId; label: string }[] = [
  { id: 'home', label: '首页' },
  { id: 'settings', label: '通用设置' },
  { id: 'shortcuts', label: '快捷键' },
  { id: 'about', label: '关于我们' },
]

export const PERFORMANCE_METRICS: PerformanceMetric[] = [
  { id: 'protocol', label: '传输协议', value: '自适应优化', icon: 'link', tint: 'purple' },
  { id: 'codec', label: '编码方式', value: 'AVC（推荐）', icon: 'video', tint: 'blue' },
  { id: 'latency', label: '延迟', value: '低延迟', icon: 'zap', tint: 'pink' },
  { id: 'loss', label: '丢包率', value: '自动优化', icon: 'target', tint: 'mint' },
]

export const INITIAL_SETTINGS: AppSettings = {
  displayId: '1',
  quality: 100,
  fps: 60,
  bitrate: 30000,
  audioSync: true,
  touchRelay: true,
  keyboardRelay: true,
}

export const INITIAL_SESSION: SessionState = {
  sharing: false,
  deviceDetected: false,
  bytesSent: 0,
  elapsedSecs: 0,
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
}

export function formatDuration(secs: number): string {
  const h = Math.floor(secs / 3600)
  const m = Math.floor((secs % 3600) / 60)
  const s = secs % 60
  return [h, m, s].map((n) => String(n).padStart(2, '0')).join(':')
}
