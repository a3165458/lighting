import { cn } from '@/lib/cn'

type Props = {
  checked: boolean
  onChange: (checked: boolean) => void
  disabled?: boolean
  ariaLabel?: string
}

export function ToggleSwitch({ checked, onChange, disabled, ariaLabel }: Props) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={ariaLabel}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        'relative h-7 w-12 shrink-0 rounded-full transition-ui',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/40',
        'disabled:opacity-[var(--disabled-opacity)] disabled:pointer-events-none',
        checked ? 'bg-brand' : 'bg-toggle-off',
      )}
    >
      <span
        className={cn(
          'absolute top-0.5 left-0.5 size-6 rounded-full bg-white shadow-sm transition-ui',
          checked && 'translate-x-5',
        )}
      />
    </button>
  )
}
