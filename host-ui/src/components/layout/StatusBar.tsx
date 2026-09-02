import { formatBytes, formatDuration } from '@/lib/format'
import { cn } from '@/lib/cn'

export type ShellSession = {
  sharing: boolean
  bytesSent: number
  elapsedSecs: number
  phase?: string
}

type Props = {
  session: ShellSession
}

export function StatusBar({ session }: Props) {
  return (
    <footer
      className={cn(
        'flex h-[var(--size-status-bar-h)] shrink-0 items-center justify-between',
        'border-t border-border bg-status-bar px-6 text-xs text-text-status',
      )}
    >
      <div className="flex items-center gap-2">
        <span
          className={cn(
            'size-[var(--size-dot)] rounded-full',
            session.sharing ? 'bg-success' : 'bg-text-muted',
          )}
        />
        <span>
          {session.sharing
            ? session.phase
              ? session.phase
              : '正在共享'
            : '未开始共享'}
        </span>
      </div>

      <div className="flex items-center gap-3 tabular-nums">
        <span>已传输：{formatBytes(session.bytesSent)}</span>
        <span className="text-border">|</span>
        <span>{formatDuration(session.elapsedSecs)}</span>
      </div>
    </footer>
  )
}
