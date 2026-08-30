import { useEffect, useId, useRef, useState, type ReactNode } from 'react'
import { ChevronDown } from 'lucide-react'
import { cn } from '@/lib/cn'

type Option = { id: string; label: string }

type Props = {
  value: string
  options: Option[]
  onChange: (id: string) => void
  disabled?: boolean
  ariaLabel?: string
  leading?: ReactNode
}

export function Dropdown({
  value,
  options,
  onChange,
  disabled,
  ariaLabel,
  leading,
}: Props) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const listId = useId()
  const selected = options.find((o) => o.id === value) ?? options[0]

  useEffect(() => {
    if (!open) return
    const onDoc = (e: MouseEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false)
    }
    document.addEventListener('mousedown', onDoc)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('mousedown', onDoc)
      document.removeEventListener('keydown', onKey)
    }
  }, [open])

  return (
    <div ref={rootRef} className="relative min-w-0 flex-1">
      <button
        type="button"
        aria-label={ariaLabel}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        disabled={disabled}
        onClick={() => setOpen((v) => !v)}
        className={cn(
          'flex h-10 w-full items-center gap-2 rounded-[var(--radius-control)]',
          'border border-border bg-white/80 px-3 text-left text-base text-text',
          'transition-ui hover:border-brand/45',
          'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/40',
          'disabled:opacity-[var(--disabled-opacity)] disabled:pointer-events-none',
          open && 'border-brand/50',
        )}
      >
        {leading}
        <span className="min-w-0 flex-1 truncate">{selected?.label}</span>
        <ChevronDown
          className={cn(
            'size-4 shrink-0 text-text-muted transition-ui',
            open && 'rotate-180 text-brand',
          )}
        />
      </button>

      {open && (
        <ul
          id={listId}
          role="listbox"
          className={cn(
            'absolute z-30 mt-2 w-full overflow-hidden rounded-[var(--radius-control)]',
            'border border-border bg-white/95 shadow-[var(--shadow-card-hover)]',
            'backdrop-blur-[var(--blur-glass)]',
          )}
        >
          {options.map((opt) => {
            const active = opt.id === value
            return (
              <li key={opt.id} role="option" aria-selected={active}>
                <button
                  type="button"
                  className={cn(
                    'flex w-full px-3 py-2.5 text-left text-base transition-ui',
                    active
                      ? 'bg-brand-soft text-brand font-semibold'
                      : 'text-text hover:bg-brand-softer',
                  )}
                  onClick={() => {
                    onChange(opt.id)
                    setOpen(false)
                  }}
                >
                  {opt.label}
                </button>
              </li>
            )
          })}
        </ul>
      )}
    </div>
  )
}
