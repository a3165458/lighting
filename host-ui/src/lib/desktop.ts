export type LightingDesktopApi = {
  platform: string
  minimize: () => Promise<void>
  maximize: () => Promise<boolean>
  close: () => Promise<void>
  isMaximized: () => Promise<boolean>
}

declare global {
  interface Window {
    lightingDesktop?: LightingDesktopApi
  }
}

export function isElectronShell(): boolean {
  return typeof window !== 'undefined' && Boolean(window.lightingDesktop)
}
