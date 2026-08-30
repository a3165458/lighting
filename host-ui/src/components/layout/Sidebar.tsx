import { Home, Info, Keyboard, Settings } from 'lucide-react'
import type { NavId } from '@/lib/mock'
import { NAV_ITEMS } from '@/lib/mock'
import { cn } from '@/lib/cn'

const ICONS = {
  home: Home,
  settings: Settings,
  shortcuts: Keyboard,
  about: Info,
} as const

type Props = {
  active: NavId
  onNavigate: (id: NavId) => void
}

export function Sidebar({ active, onNavigate }: Props) {
  return (
    <aside
      className={cn(
        'flex h-full w-[var(--space-sidebar-width)] shrink-0 flex-col',
        'border-r border-border bg-sidebar',
        'backdrop-blur-[var(--blur-glass)]',
      )}
    >
      <div className="flex items-center gap-3 px-5 pt-6 pb-8">
        <div
          className={cn(
            'flex size-10 items-center justify-center rounded-[var(--radius-icon)]',
            'bg-brand-gradient text-sm font-bold text-white shadow-[var(--shadow-button)]',
          )}
          aria-hidden
        >
          Li
        </div>
        <div className="text-lg font-bold text-text">Lighting 副屏</div>
      </div>

      <nav className="flex flex-col gap-[var(--space-nav-gap)] px-[var(--space-nav-pad-x)]">
        {NAV_ITEMS.map((item) => {
          const Icon = ICONS[item.id]
          const selected = item.id === active
          return (
            <button
              key={item.id}
              type="button"
              onClick={() => onNavigate(item.id)}
              className={cn(
                'flex h-[var(--size-nav-item-h)] items-center gap-3',
                'rounded-[var(--radius-control)] px-4 text-base transition-ui',
                'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/40',
                selected
                  ? 'bg-brand-soft font-semibold text-brand'
                  : 'text-text-secondary hover:bg-brand-softer hover:text-text',
              )}
            >
              <Icon
                className={cn('size-5', selected ? 'text-brand' : 'text-text-muted')}
                strokeWidth={2}
              />
              {item.label}
            </button>
          )
        })}
      </nav>
    </aside>
  )
}
