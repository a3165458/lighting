import { Gauge, Monitor, Volume2, Waves } from 'lucide-react'
import { Dropdown } from '@/components/ui/Dropdown'
import { SettingRow } from '@/components/ui/SettingRow'
import { SliderControl } from '@/components/ui/SliderControl'
import { ToggleSwitch } from '@/components/ui/ToggleSwitch'
import { DISPLAY_OPTIONS, type AppSettings } from '@/lib/mock'

type Props = {
  settings: AppSettings
  onChange: (patch: Partial<AppSettings>) => void
  disabled?: boolean
}

export function DisplaySettings({ settings, onChange, disabled }: Props) {
  return (
    <section className="glass-card p-[var(--space-card-pad)]" aria-label="扩展屏设置">
      <header className="mb-5 flex items-center gap-3">
        <span className="flex size-9 items-center justify-center rounded-[var(--radius-icon)] bg-brand-soft text-brand">
          <Monitor className="size-4" strokeWidth={2} />
        </span>
        <h2 className="text-lg font-bold text-text">扩展屏设置</h2>
      </header>

      <div className="flex flex-col gap-[var(--space-form-gap)]">
        <SettingRow
          icon={Monitor}
          label="显示器"
          control={
            <div className="w-[280px] max-w-full">
              <Dropdown
                value={settings.displayId}
                options={DISPLAY_OPTIONS}
                onChange={(displayId) => onChange({ displayId })}
                disabled={disabled}
                ariaLabel="选择显示器"
              />
            </div>
          }
        />

        <SettingRow
          icon={Gauge}
          label="画质"
          valueSlot={`${settings.quality}%`}
          control={
            <div className="w-[200px]">
              <SliderControl
                value={settings.quality}
                min={40}
                max={100}
                step={5}
                onChange={(quality) => onChange({ quality })}
                disabled={disabled}
                ariaLabel="画质"
              />
            </div>
          }
        />

        <SettingRow
          icon={Waves}
          label="帧率"
          valueSlot={`${settings.fps} fps`}
          control={
            <div className="w-[200px]">
              <SliderControl
                value={settings.fps}
                min={30}
                max={120}
                step={5}
                onChange={(fps) => onChange({ fps })}
                disabled={disabled}
                ariaLabel="帧率"
              />
            </div>
          }
        />

        <SettingRow
          icon={Gauge}
          label="码率"
          valueSlot={`${settings.bitrate} kbps`}
          control={
            <div className="w-[200px]">
              <SliderControl
                value={settings.bitrate}
                min={5000}
                max={50000}
                step={1000}
                onChange={(bitrate) => onChange({ bitrate })}
                disabled={disabled}
                ariaLabel="码率"
              />
            </div>
          }
        />

        <SettingRow
          icon={Volume2}
          label="系统声音同步"
          control={
            <ToggleSwitch
              checked={settings.audioSync}
              onChange={(audioSync) => onChange({ audioSync })}
              disabled={disabled}
              ariaLabel="系统声音同步"
            />
          }
        />
      </div>
    </section>
  )
}
