import type { LucideIcon } from 'lucide-react'
import type { ReactNode } from 'react'
import { cn } from '@/lib/cn'

type Props = {
  icon: LucideIcon
  label: string
  description?: string
  control: ReactNode
  valueSlot?: ReactNode
  tall?: boolean
}

export function SettingRow({
  icon: Icon,
  label,
  description,
  control,
  valueSlot,
  tall,
}: Props) {
  return (
    <div
      className={cn(
        'flex items-center gap-3',
        tall ? 'min-h-[var(--size-interaction-row-h)]' : 'min-h-[var(--size-setting-row-h)]',
      )}
    >
      <span
        className={cn(
          'flex size-9 shrink-0 items-center justify-center',
          'rounded-[var(--radius-icon)] bg-brand-soft text-brand',
        )}
      >
        <Icon className="size-4" strokeWidth={2} />
      </span>

      <div className="min-w-[88px] shrink-0">
        <div className="text-base font-semibold text-text">{label}</div>
        {description && (
          <div className="mt-0.5 max-w-[220px] text-sm text-text-secondary">{description}</div>
        )}
      </div>

      <div className="flex min-w-0 flex-1 items-center justify-end gap-3">
        <div className="flex min-w-0 items-center justify-end">{control}</div>
        {valueSlot && (
          <div className="w-[100px] shrink-0 text-right text-sm font-semibold tabular-nums text-text-secondary">
            {valueSlot}
          </div>
        )}
      </div>
    </div>
  )
}
