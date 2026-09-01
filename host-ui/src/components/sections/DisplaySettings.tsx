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
  { id: 'device', label: '跟随平板（保持电脑比例）' },
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
  const quality = settings?.qualityPct ?? 100
  const fps = settings?.fps ?? 60
  const bitrate = settings?.bitrateKbps ?? 25000
  const audioSync = settings?.sendAudio ?? true
  const preferHevc = settings?.preferHevc ?? false
  const resCap = settings?.resCap ?? 'device'
  const shareMode = settings?.shareMode ?? 'mirror'
  const shareMeta =
    SHARE_MODE_OPTIONS.find((o) => o.id === shareMode) ?? SHARE_MODE_OPTIONS[0]
  const isExtend = shareMode === 'extend' || shareMode === 'external'
  const hasSecondary = host.displays.some((d) => !d.primary)

  return (
    <section className="glass-card p-[var(--space-card-pad)]" aria-label="投屏设置">
      <header className="mb-5 flex items-center gap-3">
        <span className="flex size-9 items-center justify-center rounded-[var(--radius-icon)] bg-brand-soft text-brand">
          <Monitor className="size-4" strokeWidth={2} />
        </span>
        <h2 className="text-lg font-bold text-text">投屏设置</h2>
      </header>

      <div className="flex flex-col gap-[var(--space-form-gap)]">
        <p className="rounded-[var(--radius-control)] bg-brand-soft/60 px-3 py-2 text-sm text-text-secondary">
          {isExtend
            ? '独立第二屏：平板变成单独桌面，虚拟屏直接设为平板分辨率（1:1 抓取，不改电脑主屏）。首次可能需管理员确认；失败会自动改用镜像。'
            : '镜像主屏：电脑画面同步到平板。「跟随平板」只切电脑原生比例（16:9 电脑不会切到 1920×1200，避免拉伸卡顿）。要 1:1 铺满 1920×1200，请用独立第二屏。'}
        </p>

        <SettingRow
          icon={SquareStack}
          label="投屏模式"
          description={shareMeta.hint}
          tall
          control={
            <div className="w-[280px] max-w-full">
              <Dropdown
                value={isExtend ? 'extend' : 'mirror'}
                options={SHARE_MODE_OPTIONS.map((o) => ({ id: o.id, label: o.label }))}
                onChange={(id) => onChange({ shareMode: id })}
                disabled={disabled}
                ariaLabel="投屏模式"
              />
            </div>
          }
        />

        {isExtend && !hasSecondary && (
          <p className="rounded-[var(--radius-control)] bg-warning/10 px-3 py-2 text-sm text-warning">
            尚未看到虚拟屏。点「开始共享」会自动启用驱动（首次可能需管理员确认），平板连接后会设为平板分辨率。失败则自动镜像，保证能投屏。
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
                disabled={disabled || isExtend}
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
