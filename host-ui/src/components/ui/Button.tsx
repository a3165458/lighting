import type { ButtonHTMLAttributes, ReactNode } from 'react'
import { cn } from '@/lib/cn'

type Variant = 'primary' | 'ghost' | 'outline'

type Props = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant
  icon?: ReactNode
  fullWidth?: boolean
}

const variants: Record<Variant, string> = {
  primary: cn(
    'bg-brand-gradient text-white shadow-[var(--shadow-button)]',
    'hover:brightness-[1.03] hover:-translate-y-px',
    'active:translate-y-0 active:brightness-95',
    'disabled:opacity-[var(--disabled-opacity)] disabled:pointer-events-none disabled:translate-y-0',
  ),
  ghost: cn(
    'bg-transparent text-text-secondary',
    'hover:bg-brand-soft hover:text-brand',
    'active:bg-brand-softer',
    'disabled:opacity-[var(--disabled-opacity)] disabled:pointer-events-none',
  ),
  outline: cn(
    'bg-white/70 text-text border border-border',
    'hover:border-brand/40 hover:text-brand',
    'active:bg-brand-softer',
    'disabled:opacity-[var(--disabled-opacity)] disabled:pointer-events-none',
  ),
}

export function Button({
  variant = 'primary',
  icon,
  fullWidth,
  className,
  children,
  type = 'button',
  ...props
}: Props) {
  return (
    <button
      type={type}
      className={cn(
        'inline-flex items-center justify-center gap-[var(--space-icon-gap)]',
        'rounded-[var(--radius-control)] px-6 py-2.5 min-h-11 text-sm font-semibold transition-ui',
        'focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-brand/40',
        fullWidth && 'w-full',
        variants[variant],
        className,
      )}
      {...props}
    >
      {icon}
      {children}
    </button>
  )
}
