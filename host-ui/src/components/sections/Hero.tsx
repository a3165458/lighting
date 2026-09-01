import { Wifi } from 'lucide-react'
import { motion } from 'framer-motion'

export function Hero() {
  return (
    <section
      className="flex h-[var(--size-hero-h)] items-center justify-between gap-8"
      aria-label="产品介绍"
    >
      <div className="min-w-0 max-w-xl">
        <h1 className="text-3xl font-bold leading-tight text-text">Lighting 副屏</h1>
        <p className="mt-2 text-md text-text-secondary">
          按平板分辨率，把电脑画面投到你的平板 / 手机
        </p>
        <p className="mt-3 text-base font-semibold text-brand">
          低延迟 · 高画质 · 流畅回传
        </p>
      </div>

      <motion.div
        className="relative hidden h-full w-[320px] shrink-0 md:block"
        initial={{ opacity: 0, y: 8 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.35, ease: 'easeOut' }}
        aria-hidden
      >
        {/* Laptop */}
        <div className="absolute bottom-4 left-0 w-[220px]">
          <div className="overflow-hidden rounded-t-xl border border-border bg-[#1e2030] p-1.5 shadow-[var(--shadow-card)]">
            <div className="h-[110px] rounded-lg bg-brand-gradient opacity-90" />
          </div>
          <div className="mx-auto h-2 w-[240px] -translate-x-2.5 rounded-b-md bg-[#c8cad8]" />
          <div className="mx-auto h-1.5 w-[160px] rounded-b-md bg-[#b0b3c4]" />
        </div>

        {/* Tablet */}
        <div className="absolute right-0 top-2 w-[140px] rounded-[18px] border border-border bg-white p-2 shadow-[var(--shadow-card-hover)]">
          <div className="relative h-[168px] overflow-hidden rounded-[12px] bg-brand-gradient">
            <div className="absolute inset-0 opacity-40"
              style={{
                background:
                  'radial-gradient(circle at 30% 20%, rgba(255,255,255,0.55), transparent 55%)',
              }}
            />
            <span className="absolute top-2 right-2 flex size-7 items-center justify-center rounded-full bg-white/90 text-brand shadow-sm">
              <Wifi className="size-3.5" strokeWidth={2.5} />
            </span>
          </div>
        </div>
      </motion.div>
    </section>
  )
}
