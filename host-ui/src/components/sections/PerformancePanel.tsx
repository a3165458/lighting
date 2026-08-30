import { Activity } from 'lucide-react'
import { PerformanceCard } from '@/components/ui/PerformanceCard'
import { buildPerformanceMetrics } from '@/lib/format'
import type { HostState } from '@/lib/host'

type Props = {
  host: HostState
}

export function PerformancePanel({ host }: Props) {
  const metrics = buildPerformanceMetrics({
    transport: host.transport,
    codec: host.codec,
    latencyMs: host.latencyMs,
    lossPermille: host.lossPermille,
    sharing: host.sharing,
  })

  return (
    <section className="glass-card p-[var(--space-card-pad)]" aria-label="传输与性能">
      <header className="mb-5 flex items-center gap-3">
        <span className="flex size-9 items-center justify-center rounded-[var(--radius-icon)] bg-brand-soft text-brand">
          <Activity className="size-4" strokeWidth={2} />
        </span>
        <h2 className="text-lg font-bold text-text">传输与性能</h2>
      </header>

      <div className="grid grid-cols-2 gap-4">
        {metrics.map((metric) => (
          <PerformanceCard key={metric.id} metric={metric} />
        ))}
      </div>
    </section>
  )
}
