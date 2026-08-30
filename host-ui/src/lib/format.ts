export type NavId = 'home' | 'settings' | 'shortcuts' | 'about'

export type PerformanceMetric = {
  id: string
  label: string
  value: string
  icon: 'link' | 'video' | 'zap' | 'target'
  tint: 'purple' | 'blue' | 'pink' | 'mint'
}

export const NAV_ITEMS: { id: NavId; label: string }[] = [
  { id: 'home', label: '首页' },
  { id: 'settings', label: '通用设置' },
  { id: 'shortcuts', label: '快捷键' },
  { id: 'about', label: '关于我们' },
]

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

export function buildPerformanceMetrics(input: {
  transport: string
  codec: string
  latencyMs: number
  lossPermille: number
  sharing: boolean
}): PerformanceMetric[] {
  const protocol = input.transport
    ? input.transport.includes('USB') || input.transport.includes('usb')
      ? 'USB 优化'
      : input.transport
    : '自适应优化'
  const codec = input.codec
    ? input.codec.toUpperCase().includes('HEVC') || input.codec.toUpperCase().includes('H265')
      ? 'HEVC'
      : 'AVC（推荐）'
    : 'AVC（推荐）'
  const latency =
    !input.sharing || input.latencyMs <= 0
      ? '—'
      : input.latencyMs <= 60
        ? `低延迟 · ${input.latencyMs} ms`
        : `${input.latencyMs} ms`
  const loss =
    !input.sharing
      ? '—'
      : input.lossPermille <= 0
        ? '自动优化'
        : `${(input.lossPermille / 10).toFixed(1)}%`

  return [
    { id: 'protocol', label: '传输协议', value: protocol, icon: 'link', tint: 'purple' },
    { id: 'codec', label: '编码方式', value: codec, icon: 'video', tint: 'blue' },
    { id: 'latency', label: '延迟', value: latency, icon: 'zap', tint: 'pink' },
    { id: 'loss', label: '丢包率', value: loss, icon: 'target', tint: 'mint' },
  ]
}
