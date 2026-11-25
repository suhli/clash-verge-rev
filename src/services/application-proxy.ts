import { open } from '@tauri-apps/plugin-dialog'

import {
  clearApplicationProxyTotals,
  getApplicationProxy,
  getApplicationProxyProcesses,
  setApplicationProxy,
} from './cmds'
import { mutate } from './mutate'

export type ApplicationAction = 'proxy' | 'direct' | 'block'
export type ApplicationRule = {
  id: number
  name: string
  pattern: string
  icon: string | null
  action: ApplicationAction
  enabled: boolean
  overlay: boolean
}
export type ApplicationOverlay = {
  global: boolean
  windows: boolean
  refreshMs: number
  fontSize: number
  foreground: number
  background: number
  opacity: number
  scalePercent: number
  anchor: 'topLeft' | 'topRight' | 'bottomLeft' | 'bottomRight'
  offsetX: number
  offsetY: number
  globalX: number
  globalY: number
  delay: 'node' | 'connection'
  hideIdle: boolean
  clickThrough: boolean
  locked: boolean
  showRates: boolean
  showTotals: boolean
  showDelay: boolean
}
export type ApplicationSettings = {
  enabled: boolean
  failOpen: boolean
  rules: ApplicationRule[]
  overlay: ApplicationOverlay
}
export type ApplicationStats = {
  uploadRate: number
  downloadRate: number
  uploadTotal: number
  downloadTotal: number
  sessionUpload: number
  sessionDownload: number
  active: number
  failures: number
  connectionDelay: number | null
}
type ApplicationView = {
  settings: ApplicationSettings
  stats: {
    running: boolean
    tunConflict: boolean
    systemProxy: boolean
    endpointReady: boolean
    error: string | null
    node: string | null
    nodeDelay: number | null
    rules: Record<number, ApplicationStats>
  }
}
export type ApplicationProcess = {
  name: string
  path: string
  icon: string | null
}

export const readApplicationProxy = async () =>
  (await getApplicationProxy()) as ApplicationView | null

export const saveApplicationProxy = (settings: ApplicationSettings) =>
  mutate(async () => (await setApplicationProxy(settings)) as ApplicationView, {
    id: 'application-proxy',
  })

export const resetApplicationProxyTotals = () =>
  mutate(async () => (await clearApplicationProxyTotals()) as ApplicationView, {
    id: 'application-proxy',
  })

export const listApplicationProcesses = async () =>
  (await getApplicationProxyProcesses()) as ApplicationProcess[]

export const selectApplicationExecutables = () =>
  open({
    multiple: true,
    filters: [{ name: 'Windows executable', extensions: ['exe'] }],
  })

export const overlayDefaults: ApplicationOverlay = {
  global: false,
  windows: false,
  refreshMs: 500,
  fontSize: 12,
  foreground: 0xf4f4f5,
  background: 0x18181b,
  opacity: 224,
  scalePercent: 100,
  anchor: 'topLeft',
  offsetX: 8,
  offsetY: 8,
  globalX: 32,
  globalY: 64,
  delay: 'node',
  hideIdle: false,
  clickThrough: true,
  locked: true,
  showRates: true,
  showTotals: true,
  showDelay: true,
}
