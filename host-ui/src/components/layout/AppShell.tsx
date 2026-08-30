import type { ReactNode } from 'react'
import { Sidebar } from '@/components/layout/Sidebar'
import { BottomActionBar } from '@/components/layout/BottomActionBar'
import { StatusBar } from '@/components/layout/StatusBar'
import type { NavId, SessionState } from '@/lib/mock'
import { cn } from '@/lib/cn'

type Props = {
  activeNav: NavId
  onNavigate: (id: NavId) => void
  session: SessionState
  onToggleShare: () => void
  onAdvanced: () => void
  onAbout: () => void
  children: ReactNode
}

export function AppShell({
  activeNav,
  onNavigate,
  session,
  onToggleShare,
  onAdvanced,
  onAbout,
  children,
}: Props) {
  return (
    <div className="flex h-full w-full items-center justify-center overflow-auto bg-[#e8eaf3] p-4">
      <div
        className={cn(
          'flex overflow-hidden bg-app-gradient shadow-[var(--shadow-card-hover)]',
          'h-[var(--size-window-h)] w-[var(--size-window-w)]',
          'max-h-full max-w-full',
          'min-h-[var(--size-window-min-h)] min-w-[min(100%,var(--size-window-min-w))]',
          'rounded-[var(--radius-window)]',
        )}
      >
        <Sidebar active={activeNav} onNavigate={onNavigate} />

        <div className="flex min-w-0 flex-1 flex-col">
          <main className="min-h-0 flex-1 overflow-y-auto px-[var(--space-main-x)] pt-6 pb-5">
            {children}
          </main>

          <BottomActionBar
            sharing={session.sharing}
            onToggleShare={onToggleShare}
            onAdvanced={onAdvanced}
            onAbout={onAbout}
          />
          <StatusBar session={session} />
        </div>
      </div>
    </div>
  )
}
