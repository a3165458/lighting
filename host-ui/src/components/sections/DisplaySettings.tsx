import { Download, Gauge, Monitor, Sparkles, SquareStack, Volume2, Waves } from 'lucide-react'
import { Button } from '@/components/ui/Button'
import { Dropdown } from '@/components/ui/Dropdown'
import { SettingRow } from '@/components/ui/SettingRow'
import { SliderControl } from '@/components/ui/SliderControl'
import { ToggleSwitch } from '@/components/ui/ToggleSwitch'
import {
  SHARE_MODE_OPTIONS,
  type HostSettingsPatch,
  type HostState,
} from '@/lib/host'

type Props = {
  host: HostState
  onChange: (patch: HostSettingsPatch) => void
  onInstallClient?: () => void
  busy?: boolean
  disabled?: boolean
}

const RES_OPTIONS = [
  { id: 'device', label: '跟随平板（推荐）' },
  { id: 'fhd', label: '最高 1080p' },
  { id: 'uhd2k', label: '最高 2K' },
  { id: 'uhd4k', label: '最高 4K' },
]

export function DisplaySettings({
  host,
  onChange,
  onInstallClient,
  busy,
  disabled,
}: Props) {
  const settings = host.settings
  const displays = host.displays.map((d) => ({ id: d.id, label: d.label }))
  const displayId = String(settings?.selectedDisplay ?? 0)
  const shareMode = settings?.shareMode ?? 'extend'
  const shareMeta =
    SHARE_MODE_OPTIONS.find((o) => o.id === shareMode) ?? SHARE_MODE_OPTIONS[1]
  const quality = settings?.qualityPct ?? 100
  const fps = settings?.fps ?? 60
  const bitrate = settings?.bitrateKbps ?? 25000
  const audioSync = settings?.sendAudio ?? true
  const preferHevc = settings?.preferHevc ?? false
  const resCap = settings?.resCap ?? 'device'
  const hasSecondary = host.displays.some((d) => !d.primary)
  const needsSecondary = shareMode === 'extend' || shareMode === 'external'
  const showVddHint = needsSecondary && !hasSecondary

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
          icon={SquareStack}
          label="投屏模式"
          description={shareMeta.hint}
          tall
          control={
            <div className="w-[280px] max-w-full">
              <Dropdown
                value={shareMode}
                options={SHARE_MODE_OPTIONS.map((o) => ({ id: o.id, label: o.label }))}
                onChange={(id) => onChange({ shareMode: id })}
                disabled={disabled}
                ariaLabel="投屏模式"
              />
            </div>
          }
        />

        {showVddHint && (
          <p className="rounded-[var(--radius-control)] bg-warning/10 px-3 py-2 text-sm text-warning">
            尚未检测到扩展屏。点「开始共享」时会自动准备（类似 GlideX，可能弹出一次管理员确认）。
            准备完成后，也可用系统 Win+P 选择「扩展」或「仅第二屏幕」——画面会经 Lighting 显示在平板上。
          </p>
        )}

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
          icon={Monitor}
          label="分辨率上限"
          control={
            <div className="w-[280px] max-w-full">
              <Dropdown
                value={resCap}
                options={RES_OPTIONS}
                onChange={(id) => onChange({ resCap: id })}
                disabled={disabled}
                ariaLabel="分辨率上限"
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
          icon={Sparkles}
          label="优先 HEVC"
          control={
            <ToggleSwitch
              checked={preferHevc}
              onChange={(next) => onChange({ preferHevc: next })}
              disabled={disabled}
              ariaLabel="优先 HEVC"
            />
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

        {onInstallClient && (
          <SettingRow
            icon={Download}
            label="客户端 APK"
            control={
              <Button
                variant="outline"
                disabled={busy || !host.connected || host.installInflight || !host.canInstallApk}
                onClick={onInstallClient}
                icon={<Download className="size-4" />}
                className="min-w-[9.5rem]"
              >
                {host.installInflight
                  ? '安装中…'
                  : host.clientAppMissing
                    ? '安装到平板'
                    : '重新安装'}
              </Button>
            }
          />
        )}
      </div>
    </section>
  )
}
