import { Minus, Square, X } from 'lucide-react'
import { useEffect, useState, type CSSProperties } from 'react'
import { isElectronShell } from '@/lib/desktop'
import { cn } from '@/lib/cn'

export function TitleBar() {
  const [visible, setVisible] = useState(false)
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    if (!isElectronShell()) return
    setVisible(true)
    void window.lightingDesktop?.isMaximized().then(setMaximized)
  }, [])

  if (!visible) return null

  const drag: CSSProperties = { WebkitAppRegion: 'drag' } as CSSProperties
  const noDrag: CSSProperties = { WebkitAppRegion: 'no-drag' } as CSSProperties

  return (
    <div
      className={cn(
        'flex h-9 shrink-0 items-center justify-between border-b border-border',
        'bg-sidebar/80 backdrop-blur-[var(--blur-glass)]',
        'select-none',
      )}
      style={drag}
    >
      <div className="flex items-center gap-2 px-3">
        <span className="flex size-5 items-center justify-center rounded-[6px] bg-brand-gradient text-[9px] font-bold text-white">
          Li
        </span>
        <span className="text-sm font-semibold text-text">Lighting 副屏</span>
      </div>

      <div className="flex h-full items-stretch" style={noDrag}>
        <button
          type="button"
          aria-label="Minimize"
          className="flex w-11 items-center justify-center text-text-secondary transition-ui hover:bg-brand-softer hover:text-text"
          onClick={() => void window.lightingDesktop?.minimize()}
        >
          <Minus className="size-4" />
        </button>
        <button
          type="button"
          aria-label={maximized ? 'Restore' : 'Maximize'}
          className="flex w-11 items-center justify-center text-text-secondary transition-ui hover:bg-brand-softer hover:text-text"
          onClick={() => {
            void window.lightingDesktop?.maximize().then(setMaximized)
          }}
        >
          <Square className="size-3.5" />
        </button>
        <button
          type="button"
          aria-label="Close"
          className="flex w-11 items-center justify-center text-text-secondary transition-ui hover:bg-danger hover:text-white"
          onClick={() => void window.lightingDesktop?.close()}
        >
          <X className="size-4" />
        </button>
      </div>
    </div>
  )
}
