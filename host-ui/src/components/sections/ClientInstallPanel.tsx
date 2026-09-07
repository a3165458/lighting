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
  const apkVersion = host.clientAppVersion?.trim()
  const expected = (host.hostVersion || host.appVersion || '').trim()
  const stale =
    Boolean(apkVersion) &&
    Boolean(expected) &&
    apkVersion.replace(/^v/, '') !== expected.replace(/^v/, '')
  const hint = !host.connected
    ? '等待本地主机就绪…'
    : !host.canInstallApk
      ? '未找到 Lighting.apk。请使用官方便携版，或把 APK 放到程序目录。'
      : !host.deviceDetected
        ? '请用数据线连接平板并开启 USB 调试。'
        : host.clientAppMissing
          ? '平板上还没有客户端，点下方安装。多数平板不会弹「允许安装」，请保持亮屏。'
          : stale
            ? `平板还是 v${apkVersion}，便携包是 v${expected}。请点重新安装（先覆盖，签名冲突才会卸载）。`
            : apkVersion
              ? `当前平板客户端 v${apkVersion}。装不上时再覆盖安装。`
              : '已检测到客户端，但读不到版本号。点重新安装覆盖，不会先卸载。'

  return (
    <section className="glass-card-lg p-8" aria-label="安装客户端">
      <header className="mb-5 flex items-start gap-3">
        <span className="flex size-10 shrink-0 items-center justify-center rounded-[var(--radius-icon)] bg-brand-soft text-brand">
          <Tablet className="size-5" strokeWidth={1.75} />
        </span>
        <div className="min-w-0 flex-1">
          <h1 className="text-2xl font-bold text-text">安装 / 更新 APK</h1>
          <p className="mt-1 text-md text-text-secondary">{hint}</p>
          {apkVersion && !host.clientAppMissing && (
            <p className="mt-2 text-sm font-medium text-text">
              平板版本 <span className="text-brand">v{apkVersion}</span>
              {stale ? (
                <span className="text-danger">（低于便携包 v{expected}）</span>
              ) : null}
            </p>
          )}
        </div>
      </header>

      <Button
        onClick={onInstallClient}
        disabled={busy || !host.connected || host.installInflight || !host.canInstallApk}
        icon={<Download className="size-4 shrink-0" />}
        className="min-w-[9.5rem]"
      >
        {host.installInflight ? '正在安装…' : host.clientAppMissing ? '安装到平板' : '重新安装'}
      </Button>

      {host.lastError && <p className="mt-3 text-sm text-danger">{host.lastError}</p>}
    </section>
  )
}
