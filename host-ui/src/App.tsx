import { useCallback, useEffect, useState } from 'react'
import { AppShell } from '@/components/layout/AppShell'
import { Hero } from '@/components/sections/Hero'
import { ConnectionCard } from '@/components/sections/ConnectionCard'
import { DisplaySettings } from '@/components/sections/DisplaySettings'
import { InteractionSettings } from '@/components/sections/InteractionSettings'
import { PerformancePanel } from '@/components/sections/PerformancePanel'
import {
  INITIAL_SESSION,
  INITIAL_SETTINGS,
  type AppSettings,
  type NavId,
  type SessionState,
} from '@/lib/mock'

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
  const [settings, setSettings] = useState<AppSettings>(INITIAL_SETTINGS)
  const [session, setSession] = useState<SessionState>(INITIAL_SESSION)

  const patchSettings = useCallback((patch: Partial<AppSettings>) => {
    setSettings((prev) => ({ ...prev, ...patch }))
  }, [])

  const toggleShare = useCallback(() => {
    setSession((prev) => {
      if (prev.sharing) {
        return { ...prev, sharing: false }
      }
      return {
        ...prev,
        sharing: true,
        deviceDetected: true,
        elapsedSecs: 0,
        bytesSent: 0,
      }
    })
  }, [])

  useEffect(() => {
    if (!session.sharing) return
    const id = window.setInterval(() => {
      setSession((prev) => ({
        ...prev,
        elapsedSecs: prev.elapsedSecs + 1,
        bytesSent: prev.bytesSent + 48_000 + Math.floor(Math.random() * 12_000),
      }))
    }, 1000)
    return () => window.clearInterval(id)
  }, [session.sharing])

  return (
    <AppShell
      activeNav={nav}
      onNavigate={setNav}
      session={session}
      onToggleShare={toggleShare}
      onAdvanced={() => setNav('settings')}
      onAbout={() => setNav('about')}
    >
      {nav === 'home' && (
        <div className="flex flex-col gap-[var(--space-card-gap)]">
          <Hero />
          <ConnectionCard session={session} onToggleShare={toggleShare} />

          <div className="grid grid-cols-1 gap-[var(--space-card-gap)] xl:grid-cols-[3fr_2fr]">
            <div className="flex flex-col gap-[var(--space-card-gap)]">
              <DisplaySettings settings={settings} onChange={patchSettings} />
              <InteractionSettings settings={settings} onChange={patchSettings} />
            </div>
            <PerformancePanel />
          </div>
        </div>
      )}

      {nav === 'settings' && (
        <Placeholder
          title="通用设置"
          body="网络偏好、启动选项与通知等通用配置将放在这里。"
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
    </AppShell>
  )
}
