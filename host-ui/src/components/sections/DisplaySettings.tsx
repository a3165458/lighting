import { Gauge, Monitor, Volume2, Waves } from 'lucide-react'
import { Dropdown } from '@/components/ui/Dropdown'
import { SettingRow } from '@/components/ui/SettingRow'
import { SliderControl } from '@/components/ui/SliderControl'
import { ToggleSwitch } from '@/components/ui/ToggleSwitch'
import type { HostSettingsPatch, HostState } from '@/lib/host'

type Props = {
  host: HostState
  onChange: (patch: HostSettingsPatch) => void
  disabled?: boolean
}

export function DisplaySettings({ host, onChange, disabled }: Props) {
  const settings = host.settings
  const displays = host.displays.map((d) => ({ id: d.id, label: d.label }))
  const displayId = String(settings?.selectedDisplay ?? 0)
  const quality = settings?.qualityPct ?? 100
  const fps = settings?.fps ?? 60
  const bitrate = settings?.bitrateKbps ?? 25000
  const audioSync = settings?.sendAudio ?? true

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
                value={displayId}
                options={
                  displays.length > 0
                    ? displays
                    : [{ id: '0', label: '未检测到显示器' }]
                }
                onChange={(id) => onChange({ selectedDisplay: Number(id) })}
                disabled={disabled || displays.length === 0}
                ariaLabel="选择显示器"
              />
            </div>
          }
        />

        <SettingRow
          icon={Gauge}
          label="画质"
          valueSlot={`${quality}%`}
          control={
            <div className="w-[200px]">
              <SliderControl
                value={quality}
                min={40}
                max={100}
                step={5}
                onChange={(qualityPct) => onChange({ qualityPct })}
                disabled={disabled}
                ariaLabel="画质"
              />
            </div>
          }
        />

        <SettingRow
          icon={Waves}
          label="帧率"
          valueSlot={`${fps} fps`}
          control={
            <div className="w-[200px]">
              <SliderControl
                value={fps}
                min={30}
                max={120}
                step={5}
                onChange={(next) => onChange({ fps: next })}
                disabled={disabled}
                ariaLabel="帧率"
              />
            </div>
          }
        />

        <SettingRow
          icon={Gauge}
          label="码率"
          valueSlot={`${bitrate} kbps`}
          control={
            <div className="w-[200px]">
              <SliderControl
                value={bitrate}
                min={5000}
                max={50000}
                step={1000}
                onChange={(bitrateKbps) => onChange({ bitrateKbps })}
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
              checked={audioSync}
              onChange={(sendAudio) => onChange({ sendAudio })}
              disabled={disabled}
              ariaLabel="系统声音同步"
            />
          }
        />
      </div>
    </section>
  )
}
