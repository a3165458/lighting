import { Keyboard, MousePointer2 } from 'lucide-react'
import { SettingRow } from '@/components/ui/SettingRow'
import { ToggleSwitch } from '@/components/ui/ToggleSwitch'
import type { AppSettings } from '@/lib/mock'

type Props = {
  settings: AppSettings
  onChange: (patch: Partial<AppSettings>) => void
  disabled?: boolean
}

export function InteractionSettings({ settings, onChange, disabled }: Props) {
  return (
    <section className="glass-card p-[var(--space-card-pad)]" aria-label="控制与交互">
      <header className="mb-4">
        <h2 className="text-lg font-bold text-text">控制与交互</h2>
      </header>

      <div className="flex flex-col gap-2">
        <SettingRow
          tall
          icon={MousePointer2}
          label="触控回传（模拟鼠标）"
          description="将平板触控同步回传到电脑"
          control={
            <ToggleSwitch
              checked={settings.touchRelay}
              onChange={(touchRelay) => onChange({ touchRelay })}
              disabled={disabled}
              ariaLabel="触控回传"
            />
          }
        />
        <SettingRow
          tall
          icon={Keyboard}
          label="键盘输入回传"
          description="平板键盘输入自动回传"
          control={
            <ToggleSwitch
              checked={settings.keyboardRelay}
              onChange={(keyboardRelay) => onChange({ keyboardRelay })}
              disabled={disabled}
              ariaLabel="键盘输入回传"
            />
          }
        />
      </div>
    </section>
  )
}
