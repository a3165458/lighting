import { Download, Tablet } from 'lucide-react'
import { Button } from '@/components/ui/Button'
import type { HostState } from '@/lib/host'

type Props = {
  host: HostState
  busy?: boolean
  onInstallClient: () => void
}

/** Always-visible APK install / upgrade controls for the Settings page. */
export function ClientInstallPanel({ host, busy, onInstallClient }: Props) {
  const hint = !host.connected
    ? '等待本地主机就绪…'
    : !host.canInstallApk
      ? '未找到 Lighting.apk。请使用官方便携版，或把 APK 放到程序目录。'
      : !host.deviceDetected
        ? '请用数据线连接平板并开启 USB 调试。'
        : host.clientAppMissing
          ? '平板上还没有客户端，点下方安装。'
          : '已检测到客户端。若版本偏旧，可重新安装覆盖。'

  return (
    <section className="glass-card-lg p-8" aria-label="安装客户端">
      <header className="mb-4 flex items-center gap-3">
        <span className="flex size-10 items-center justify-center rounded-[var(--radius-icon)] bg-brand-soft text-brand">
          <Tablet className="size-5" strokeWidth={1.75} />
        </span>
        <div>
          <h1 className="text-2xl font-bold text-text">安装 / 更新 APK</h1>
          <p className="mt-1 text-md text-text-secondary">{hint}</p>
        </div>
      </header>

      <Button
        onClick={onInstallClient}
        disabled={busy || !host.connected || host.installInflight || !host.canInstallApk}
        icon={<Download className="size-4" />}
        className="mt-2"
      >
        {host.installInflight
          ? '正在安装…'
          : host.clientAppMissing
            ? '安装到平板'
            : '重新安装最新客户端'}
      </Button>

      {host.lastError && <p className="mt-3 text-sm text-danger">{host.lastError}</p>}
    </section>
  )
}
