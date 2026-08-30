import { Info, PlayCircle, Settings, Square } from 'lucide-react'
import { Button } from '@/components/ui/Button'
import { cn } from '@/lib/cn'

type Props = {
  sharing: boolean
  onToggleShare: () => void
  onAdvanced: () => void
  onAbout: () => void
}

export function BottomActionBar({
  sharing,
  onToggleShare,
  onAdvanced,
  onAbout,
}: Props) {
  return (
    <div
      className={cn(
        'flex h-[var(--size-action-bar-h)] shrink-0 items-center justify-between gap-4',
        'border-t border-border bg-action-bar px-8',
        'backdrop-blur-[var(--blur-action)]',
      )}
    >
      <Button
        variant="ghost"
        onClick={onAdvanced}
        icon={<Settings className="size-4" />}
        className="h-10 px-4 text-base"
      >
        高级设置
      </Button>

      <Button
        onClick={onToggleShare}
        icon={
          sharing ? (
            <Square className="size-5 fill-current" />
          ) : (
            <PlayCircle className="size-5" />
          )
        }
        className="h-[var(--size-btn-primary-h)] w-[min(100%,var(--size-btn-main-w))] text-md"
      >
        {sharing ? '停止共享' : '开始共享'}
      </Button>

      <Button
        variant="ghost"
        onClick={onAbout}
        icon={<Info className="size-4" />}
        className="h-10 px-4 text-base"
      >
        关于
      </Button>
    </div>
  )
}
