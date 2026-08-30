import { Link2, Target, Video, Zap, type LucideIcon } from 'lucide-react'
import type { PerformanceMetric } from '@/lib/format'
import { cn } from '@/lib/cn'

const ICONS: Record<PerformanceMetric['icon'], LucideIcon> = {
  link: Link2,
  video: Video,
  zap: Zap,
  target: Target,
}

const TINTS: Record<PerformanceMetric['tint'], string> = {
  purple: 'bg-brand-soft text-brand',
  blue: 'bg-tint-blue-bg text-tint-blue-fg',
  pink: 'bg-tint-pink-bg text-tint-pink-fg',
  mint: 'bg-tint-mint-bg text-tint-mint-fg',
}

type Props = {
  metric: PerformanceMetric
}

export function PerformanceCard({ metric }: Props) {
  const Icon = ICONS[metric.icon]

  return (
    <article
      className={cn(
        'flex h-[var(--size-perf-card-h)] flex-col justify-between',
        'rounded-[var(--radius-card)] border border-border',
        'bg-card-soft p-5 shadow-[var(--shadow-card)] transition-ui',
        'hover:-translate-y-0.5 hover:shadow-[var(--shadow-card-hover)]',
      )}
    >
      <span
        className={cn(
          'flex size-[var(--size-icon-lg)] items-center justify-center rounded-full',
          TINTS[metric.tint],
        )}
      >
        <Icon className="size-5" strokeWidth={2} />
      </span>
      <div>
        <div className="text-sm text-text-secondary">{metric.label}</div>
        <div className="mt-1 text-md font-semibold text-text">{metric.value}</div>
      </div>
    </article>
  )
}
