import { AlertTriangle, Download, Play, Square, Tablet } from 'lucide-react'
import { Button } from '@/components/ui/Button'
import type { HostState } from '@/lib/host'
import { shouldShowSeparateLastError } from '@/lib/host'
import { cn } from '@/lib/cn'

type Props = {
  host: HostState
  busy?: boolean
  onToggleShare: () => void
  onInstallClient: () => void
}

function stepDot(state: string) {
  if (state === 'done') return 'bg-success'
  if (state === 'current') return 'bg-brand'
  if (state === 'error') return 'bg-danger'
  return 'bg-text-muted/40'
}

export function ConnectionCard({ host, busy, onToggleShare, onInstallClient }: Props) {
  const sharing = host.sharing
  const title = sharing
    ? host.activityTitle || host.phase || '正在共享'
    : '未开始共享'
  const subtitle = sharing
    ? host.activityDetail || host.detail || '正在准备，请稍候…'
    : '请连接你的设备，点击开始共享'
  const steps = host.activitySteps ?? []
  const showSteps = sharing && steps.length > 0

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
        {showSteps && (
          <ol className="mt-3 flex flex-col gap-1.5" aria-label="当前动作">
            {steps.map((step) => (
              <li
                key={step.id}
                className={cn(
                  'flex items-center gap-2 text-sm',
                  step.state === 'current'
                    ? 'font-semibold text-text'
                    : step.state === 'done'
                      ? 'text-text-secondary'
                      : step.state === 'error'
                        ? 'font-semibold text-danger'
                        : 'text-text-muted',
                )}
              >
                <span className={cn('size-2 shrink-0 rounded-full', stepDot(step.state))} />
                <span>{step.label}</span>
                {step.state === 'current' && <span className="text-xs text-brand">进行中</span>}
              </li>
            ))}
          </ol>
        )}
        {showSteps &&
          (host.phase === '准备虚拟屏' || host.activityTitle === '正在启用虚拟屏') &&
          host.hostElevated && (
            <p className="mt-2 text-sm text-text-secondary">
              已是管理员，安装驱动时不会再弹确认窗口。若弹出 360/杀毒软件请选允许。
            </p>
          )}
        {showSteps &&
          (host.phase === '准备虚拟屏' || host.activityTitle === '正在启用虚拟屏') &&
          host.hostElevated === false && (
            <p className="mt-2 text-sm text-warning">
              请看屏幕中央或任务栏是否有蓝底「用户账户控制」。没有弹窗时请完全退出 Lighting，再右键以管理员运行。
            </p>
          )}
        {host.usbHint && host.usbHint !== subtitle && (
          <p className={cn('mt-2 flex items-center gap-2 text-sm font-medium', toneClass)}>
            <AlertTriangle className="size-4 shrink-0" strokeWidth={2} />
            {host.usbHint}
          </p>
        )}
        {shouldShowSeparateLastError(host.lastError, [
          host.usbHint,
          host.activityDetail,
          host.detail,
          subtitle,
        ]) && <p className="mt-1 text-sm text-danger">{host.lastError}</p>}
        {host.canInstallApk && (
          <div className="mt-3">
            <Button
              variant="outline"
              disabled={busy || host.installInflight}
              onClick={onInstallClient}
              icon={<Download className="size-4" />}
              className="min-w-[9.5rem]"
            >
              {host.installInflight
                ? '安装中…'
                : host.clientAppMissing
                  ? '安装到平板'
                  : '更新客户端'}
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
