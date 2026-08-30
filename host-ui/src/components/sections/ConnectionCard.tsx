import { AlertTriangle, Play, Square, Tablet } from 'lucide-react'
import { Button } from '@/components/ui/Button'
import type { SessionState } from '@/lib/mock'
import { cn } from '@/lib/cn'

type Props = {
  session: SessionState
  onToggleShare: () => void
}

export function ConnectionCard({ session, onToggleShare }: Props) {
  const title = session.sharing ? '正在共享' : '未开始共享'
  const subtitle = session.sharing
    ? '画面正在传输到你的 Android 设备'
    : '请连接你的设备，点击开始共享'

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
            session.sharing ? 'bg-success' : 'bg-warning',
          )}
        />
      </div>

      <div className="min-w-0 flex-1">
        <h2 className="text-lg font-bold text-text">{title}</h2>
        <p className="mt-1 text-base text-text-secondary">{subtitle}</p>
        {!session.deviceDetected && !session.sharing && (
          <p className="mt-2 flex items-center gap-2 text-sm font-medium text-warning">
            <AlertTriangle className="size-4 shrink-0" strokeWidth={2} />
            未检测到设备，请检查设备及网络连接
          </p>
        )}
        {session.deviceDetected && !session.sharing && (
          <p className="mt-2 text-sm font-medium text-success">已检测到设备，可以开始共享</p>
        )}
      </div>

      <Button
        onClick={onToggleShare}
        icon={
          session.sharing ? (
            <Square className="size-4 fill-current" />
          ) : (
            <Play className="size-4 fill-current" />
          )
        }
        className="h-[var(--size-btn-primary-h)] w-[var(--size-btn-primary-w)] text-md"
      >
        {session.sharing ? '停止共享' : '开始共享'}
      </Button>
    </section>
  )
}
