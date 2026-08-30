import { Activity } from 'lucide-react'
import { PerformanceCard } from '@/components/ui/PerformanceCard'
import { PERFORMANCE_METRICS } from '@/lib/mock'

export function PerformancePanel() {
  return (
    <section className="glass-card p-[var(--space-card-pad)]" aria-label="传输与性能">
      <header className="mb-5 flex items-center gap-3">
        <span className="flex size-9 items-center justify-center rounded-[var(--radius-icon)] bg-brand-soft text-brand">
          <Activity className="size-4" strokeWidth={2} />
        </span>
        <h2 className="text-lg font-bold text-text">传输与性能</h2>
      </header>

      <div className="grid grid-cols-2 gap-4">
        {PERFORMANCE_METRICS.map((metric) => (
          <PerformanceCard key={metric.id} metric={metric} />
        ))}
      </div>
    </section>
  )
}
