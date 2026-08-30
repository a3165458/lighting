import { AlertTriangle, Download, Play, Square, Tablet } from 'lucide-react'
import { Button } from '@/components/ui/Button'
import type { HostState } from '@/lib/host'
import { cn } from '@/lib/cn'

type Props = {
  host: HostState
  busy?: boolean
  onToggleShare: () => void
  onInstallClient: () => void
}

export function ConnectionCard({ host, busy, onToggleShare, onInstallClient }: Props) {
  const title = host.sharing ? '正在共享' : '未开始共享'
  const subtitle = host.sharing
    ? host.detail || '画面正在传输到你的 Android 设备'
    : '请连接你的设备，点击开始共享'

  const toneClass =
    host.usbTone === 'ok'
      ? 'text-success'
      : host.usbTone === 'bad'
        ? 'text-danger'
        : 'text-warning'

  return (
    <section
      className={cn(
        'glass-card-lg flex min-h-[var(--size-connection-h)] items-center gap-5 px-6 py-5',
      )}
      aria-label="连接状态"
    >
      <div className="relative shrink-0">
        <span className="flex size-14 items-center justify-center rounded-full bg-tint-blue-bg text-tint-blue-fg">
          <Tablet className="size-7" strokeWidth={1.75} />
        </span>
        <span
          className={cn(
            'absolute -right-0.5 -bottom-0.5 size-3.5 rounded-full border-2 border-white',
            host.sharing ? 'bg-success' : host.deviceDetected ? 'bg-success' : 'bg-warning',
          )}
        />
      </div>

      <div className="min-w-0 flex-1">
        <h2 className="text-lg font-bold text-text">{title}</h2>
        <p className="mt-1 text-base text-text-secondary">{subtitle}</p>
        {host.usbHint && (
          <p className={cn('mt-2 flex items-center gap-2 text-sm font-medium', toneClass)}>
            <AlertTriangle className="size-4 shrink-0" strokeWidth={2} />
            {host.usbHint}
          </p>
        )}
        {host.lastError && (
          <p className="mt-1 text-sm text-danger">{host.lastError}</p>
        )}
        {host.clientAppMissing && host.canInstallApk && (
          <div className="mt-3">
            <Button
              variant="outline"
              disabled={busy || host.installInflight}
              onClick={onInstallClient}
              icon={<Download className="size-4" />}
              className="h-9 px-4 text-sm"
            >
              {host.installInflight ? '安装中…' : '安装到平板'}
            </Button>
          </div>
        )}
      </div>

      <Button
        onClick={onToggleShare}
        disabled={busy || !host.connected}
        icon={
          host.sharing ? (
            <Square className="size-4 fill-current" />
          ) : (
            <Play className="size-4 fill-current" />
          )
        }
        className="h-[var(--size-btn-primary-h)] w-[var(--size-btn-primary-w)] text-md"
      >
        {host.sharing ? '停止共享' : '开始共享'}
      </Button>
    </section>
  )
}
