import { useCallback, useEffect, useState } from 'react'
import { AppShell } from '@/components/layout/AppShell'
import { Hero } from '@/components/sections/Hero'
import { BootstrapBanner } from '@/components/sections/BootstrapBanner'
import { ConnectionCard } from '@/components/sections/ConnectionCard'
import { ClientInstallPanel } from '@/components/sections/ClientInstallPanel'
import { DisplaySettings } from '@/components/sections/DisplaySettings'
import { InteractionSettings } from '@/components/sections/InteractionSettings'
import { PerformancePanel } from '@/components/sections/PerformancePanel'
import type { NavId } from '@/lib/format'
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
      setHost((prev) => ({
        ...prev,
        settings: prev.settings
          ? {
              ...prev.settings,
              ...(patch.selectedDisplay !== undefined
                ? { selectedDisplay: patch.selectedDisplay }
                : {}),
              ...(patch.shareMode !== undefined ? { shareMode: patch.shareMode } : {}),
              ...(patch.qualityPct !== undefined ? { qualityPct: patch.qualityPct } : {}),
              ...(patch.fps !== undefined ? { fps: patch.fps } : {}),
              ...(patch.bitrateKbps !== undefined ? { bitrateKbps: patch.bitrateKbps } : {}),
              ...(patch.sendAudio !== undefined ? { sendAudio: patch.sendAudio } : {}),
              ...(patch.preferHevc !== undefined ? { preferHevc: patch.preferHevc } : {}),
              ...(patch.resCap !== undefined ? { resCap: patch.resCap } : {}),
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

  const retryBootstrap = useCallback(async () => {
    if (!window.lightingHost?.retryBootstrap) return
    setBusy(true)
    try {
      await window.lightingHost.retryBootstrap()
      await refresh()
    } finally {
      setBusy(false)
    }
  }, [refresh])

  const sessionForShell = {
    sharing: host.sharing,
    bytesSent: host.bytesSent,
    elapsedSecs: host.connectedSecs,
    phase: host.activityTitle || host.phase,
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
          <BootstrapBanner boot={host.bootstrap} onRetry={() => void retryBootstrap()} />
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
                onInstallClient={() => void installClient()}
                busy={busy}
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
        <div className="flex flex-col gap-[var(--space-card-gap)]">
          <ClientInstallPanel
            host={host}
            busy={busy}
            onInstallClient={() => void installClient()}
          />
          <DisplaySettings
            host={host}
            onChange={(patch) => void patchSettings(patch)}
            onInstallClient={() => void installClient()}
            busy={busy}
            disabled={!host.connected || host.sharing}
          />
          <section className="glass-card p-6">
            <h2 className="text-lg font-bold text-text">运行环境</h2>
            <p className="mt-2 text-md text-text-secondary">
              {host.connected
                ? `应用 v${host.appVersion || host.hostVersion || '—'} · 主机 v${host.hostVersion || '—'}。首次启动会自动准备 adb / ffmpeg。`
                : '正在连接本地主机，或首次启动正在下载运行组件…'}
            </p>
            {host.connected &&
              host.appVersion &&
              host.hostVersion &&
              host.appVersion !== host.hostVersion && (
                <p className="mt-2 text-sm text-warning">
                  检测到旧主机进程仍在运行，正在切换到本包内主机…
                </p>
              )}
          </section>
        </div>
      )}
      {nav === 'shortcuts' && (
        <Placeholder title="快捷键" body="自定义开始 / 停止共享、画质切换等快捷键。" />
      )}
      {nav === 'about' && (
        <Placeholder
          title="关于我们"
          body="Lighting 副屏 — 镜像主屏、双屏扩展，或「仅平板」关掉电脑屏躺着用。Windows 锁屏后无法抓屏，躺着用请关自动锁屏、合盖设为不采取任何操作。"
        />
      )}
    </AppShell>
  )
}
