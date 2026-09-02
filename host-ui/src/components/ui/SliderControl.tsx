import { cn } from '@/lib/cn'

type Props = {
  value: number
  min?: number
  max?: number
  step?: number
  onChange: (value: number) => void
  disabled?: boolean
  ariaLabel?: string
}

export function SliderControl({
  value,
  min = 0,
  max = 100,
  step = 1,
  onChange,
  disabled,
  ariaLabel,
}: Props) {
  const pct = ((value - min) / (max - min)) * 100

  return (
    <input
      type="range"
      min={min}
      max={max}
      step={step}
      value={value}
      disabled={disabled}
      aria-label={ariaLabel}
      onChange={(e) => onChange(Number(e.target.value))}
      className={cn(
        'lighting-slider h-2 w-full cursor-pointer appearance-none rounded-full',
        'disabled:cursor-not-allowed disabled:opacity-[var(--disabled-opacity)]',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/40',
      )}
      style={{
        background: `linear-gradient(to right, var(--color-brand) 0%, var(--color-brand) ${pct}%, var(--color-track) ${pct}%, var(--color-track) 100%)`,
      }}
    />
  )
}
