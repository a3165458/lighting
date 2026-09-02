import { RefreshCw } from 'lucide-react'
import { Button } from '@/components/ui/Button'
import type { BootstrapStatus } from '@/lib/host'

type Props = {
  boot?: BootstrapStatus
  onRetry?: () => void
}

export function BootstrapBanner({ boot, onRetry }: Props) {
  if (!boot || boot.ready) return null

  const failed = boot.phase === 'error'
  return (
    <section
      className="glass-card-lg flex items-center justify-between gap-4 px-6 py-4"
      aria-live="polite"
    >
      <div className="min-w-0">
        <h2 className="text-md font-bold text-text">
          {failed ? '运行环境准备失败' : '首次启动，正在自动准备…'}
        </h2>
        <p className="mt-1 text-base text-text-secondary">
          {boot.error || boot.detail || '下载 USB 工具与编码组件，无需手动安装'}
        </p>
      </div>
      {failed && onRetry && (
        <Button
          variant="outline"
          icon={<RefreshCw className="size-4" />}
          onClick={onRetry}
          className="h-10 px-4 shrink-0"
        >
          重试
        </Button>
      )}
    </section>
  )
}
