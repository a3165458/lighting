import { useCallback, useEffect, useState } from 'react'
import { AppShell } from '@/components/layout/AppShell'
import { Hero } from '@/components/sections/Hero'
import { ConnectionCard } from '@/components/sections/ConnectionCard'
import { DisplaySettings } from '@/components/sections/DisplaySettings'
import { InteractionSettings } from '@/components/sections/InteractionSettings'
import { PerformancePanel } from '@/components/sections/PerformancePanel'
import { NAV_ITEMS, type NavId } from '@/lib/format'
import {
  DISCONNECTED_STATE,
  hasHostBridge,
  type HostSettingsPatch,
  type HostState,
} from '@/lib/host'

function Placeholder({ title, body }: { title: string; body: string }) {
  return (
    <section className="glass-card-lg p-8">
      <h1 className="text-2xl font-bold text-text">{title}</h1>
      <p className="mt-2 text-md text-text-secondary">{body}</p>
    </section>
  )
}

export default function App() {
  const [nav, setNav] = useState<NavId>('home')
  const [host, setHost] = useState<HostState>(DISCONNECTED_STATE)
  const [busy, setBusy] = useState(false)

  const applyState = useCallback((state: HostState) => {
    setHost(state)
  }, [])

  const refresh = useCallback(async () => {
    if (!hasHostBridge() || !window.lightingHost) {
      setHost(DISCONNECTED_STATE)
      return
    }
    try {
      const state = await window.lightingHost.getState()
      applyState(state)
    } catch (err) {
      setHost({
        ...DISCONNECTED_STATE,
        usbHint: String((err as Error).message || err),
      })
    }
  }, [applyState])

  useEffect(() => {
    void refresh()
    const id = window.setInterval(() => {
      void refresh()
    }, 500)
    return () => window.clearInterval(id)
  }, [refresh])

  const toggleShare = useCallback(async () => {
    if (!window.lightingHost || busy) return
    setBusy(true)
    try {
      const state = host.sharing
        ? await window.lightingHost.stopShare()
        : await window.lightingHost.startShare()
      applyState(state)
    } catch (err) {
      setHost((prev) => ({
        ...prev,
        lastError: String((err as Error).message || err),
        usbHint: String((err as Error).message || err),
        usbTone: 'bad',
      }))
    } finally {
      setBusy(false)
    }
  }, [applyState, busy, host.sharing])

  const patchSettings = useCallback(
    async (patch: HostSettingsPatch) => {
      if (!window.lightingHost) return
      // Optimistic local update for snappy sliders.
      setHost((prev) => ({
        ...prev,
        settings: prev.settings
          ? {
              ...prev.settings,
              ...(patch.selectedDisplay !== undefined
                ? { selectedDisplay: patch.selectedDisplay }
                : {}),
              ...(patch.qualityPct !== undefined ? { qualityPct: patch.qualityPct } : {}),
              ...(patch.fps !== undefined ? { fps: patch.fps } : {}),
              ...(patch.bitrateKbps !== undefined ? { bitrateKbps: patch.bitrateKbps } : {}),
              ...(patch.sendAudio !== undefined ? { sendAudio: patch.sendAudio } : {}),
              ...(patch.touchRelay !== undefined ? { touchRelay: patch.touchRelay } : {}),
              ...(patch.keyboardRelay !== undefined
                ? { keyboardRelay: patch.keyboardRelay }
                : {}),
            }
          : prev.settings,
      }))
      try {
        const state = await window.lightingHost.setSettings(patch)
        applyState(state)
      } catch (err) {
        setHost((prev) => ({
          ...prev,
          lastError: String((err as Error).message || err),
        }))
      }
    },
    [applyState],
  )

  const installClient = useCallback(async () => {
    if (!window.lightingHost || busy) return
    setBusy(true)
    try {
      const state = await window.lightingHost.installClient()
      applyState(state)
    } catch (err) {
      setHost((prev) => ({
        ...prev,
        lastError: String((err as Error).message || err),
        usbHint: String((err as Error).message || err),
        usbTone: 'bad',
      }))
    } finally {
      setBusy(false)
    }
  }, [applyState, busy])

  const sessionForShell = {
    sharing: host.sharing,
    deviceDetected: host.deviceDetected,
    bytesSent: host.bytesSent,
    elapsedSecs: host.connectedSecs,
  }

  return (
    <AppShell
      activeNav={nav}
      onNavigate={setNav}
      session={sessionForShell}
      onToggleShare={() => void toggleShare()}
      onAdvanced={() => setNav('settings')}
      onAbout={() => setNav('about')}
    >
      {nav === 'home' && (
        <div className="flex flex-col gap-[var(--space-card-gap)]">
          <Hero />
          <ConnectionCard
            host={host}
            busy={busy}
            onToggleShare={() => void toggleShare()}
            onInstallClient={() => void installClient()}
          />

          <div className="grid grid-cols-1 gap-[var(--space-card-gap)] xl:grid-cols-[3fr_2fr]">
            <div className="flex flex-col gap-[var(--space-card-gap)]">
              <DisplaySettings
                host={host}
                onChange={(patch) => void patchSettings(patch)}
                disabled={!host.connected || host.sharing}
              />
              <InteractionSettings
                host={host}
                onChange={(patch) => void patchSettings(patch)}
                disabled={!host.connected}
              />
            </div>
            <PerformancePanel host={host} />
          </div>
        </div>
      )}

      {nav === 'settings' && (
        <Placeholder
          title="通用设置"
          body={
            host.connected
              ? `已连接主机 v${host.hostVersion || '—'}. IPC 端口由 lighting-host 提供。`
              : '尚未连接到 lighting-host.exe。请先编译主机，或设置 LIGHTING_HOST_PATH。'
          }
        />
      )}
      {nav === 'shortcuts' && (
        <Placeholder title="快捷键" body="自定义开始 / 停止共享、画质切换等快捷键。" />
      )}
      {nav === 'about' && (
        <Placeholder
          title="关于我们"
          body="Lighting 副屏 — 将 Android 平板 / 手机变成 Windows 电脑扩展屏。"
        />
      )}

      {/* keep NAV_ITEMS referenced for tree-shaking clarity */}
      <span className="hidden">{NAV_ITEMS.length}</span>
    </AppShell>
  )
}
