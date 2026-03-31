import { create } from 'zustand'

export type SidebarMode = 'modules' | 'inventory'

interface UiState {
  theme: 'dark' | 'light' | 'system'
  accentColor: string
  brandName: string
  brandSubtitle: string
  dashboardRefreshSecs: number
  sidebarMode: SidebarMode
  setTheme: (t: 'dark' | 'light' | 'system') => void
  setAccentColor: (c: string) => void
  setBrandName: (n: string) => void
  setBrandSubtitle: (s: string) => void
  setDashboardRefreshSecs: (s: number) => void
  setSidebarMode: (m: SidebarMode) => void
  loadFromStorage: () => void
}

function persist(state: Partial<UiState>) {
  localStorage.setItem('vmm_ui', JSON.stringify(state))
}

function getStored(): Partial<UiState> {
  try {
    return JSON.parse(localStorage.getItem('vmm_ui') || '{}')
  } catch { return {} }
}

/** Resolve effective theme considering 'system' preference. */
function resolveTheme(theme: 'dark' | 'light' | 'system'): 'dark' | 'light' {
  if (theme === 'system') {
    return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
  }
  return theme
}

/** Apply theme to <html> element. */
function applyTheme(theme: 'dark' | 'light' | 'system') {
  const effective = resolveTheme(theme)
  document.documentElement.classList.toggle('dark', effective === 'dark')
  document.documentElement.classList.toggle('light', effective === 'light')
  document.documentElement.setAttribute('data-theme', effective)
}

/** Apply accent color as CSS custom property. */
function applyAccent(hex: string) {
  // Convert hex to individual R/G/B for Tailwind opacity support
  const r = parseInt(hex.slice(1, 3), 16)
  const g = parseInt(hex.slice(3, 5), 16)
  const b = parseInt(hex.slice(5, 7), 16)
  document.documentElement.style.setProperty('--vmm-accent-r', String(r))
  document.documentElement.style.setProperty('--vmm-accent-g', String(g))
  document.documentElement.style.setProperty('--vmm-accent-b', String(b))
  document.documentElement.style.setProperty('--color-vmm-accent', hex)
  document.documentElement.style.setProperty('--color-vmm-accent-hover', adjustBrightness(hex, 20))
  document.documentElement.style.setProperty('--color-vmm-accent-dim', adjustBrightness(hex, -30))
}

function adjustBrightness(hex: string, amount: number): string {
  const clamp = (v: number) => Math.max(0, Math.min(255, v))
  const r = clamp(parseInt(hex.slice(1, 3), 16) + amount)
  const g = clamp(parseInt(hex.slice(3, 5), 16) + amount)
  const b = clamp(parseInt(hex.slice(5, 7), 16) + amount)
  return `#${r.toString(16).padStart(2, '0')}${g.toString(16).padStart(2, '0')}${b.toString(16).padStart(2, '0')}`
}

export const useUiStore = create<UiState>((set, get) => ({
  theme: 'dark',
  accentColor: '#38bdf8',
  brandName: 'CoreVM',
  brandSubtitle: 'V2.4.0-ENTERPRISE',
  dashboardRefreshSecs: 10,
  sidebarMode: 'modules',

  setTheme: (t) => {
    applyTheme(t)
    set({ theme: t })
    persist({ ...getStored(), theme: t })
  },

  setAccentColor: (c) => {
    applyAccent(c)
    set({ accentColor: c })
    persist({ ...getStored(), accentColor: c })
  },

  setBrandName: (n) => {
    set({ brandName: n })
    persist({ ...getStored(), brandName: n })
  },

  setBrandSubtitle: (s) => {
    set({ brandSubtitle: s })
    persist({ ...getStored(), brandSubtitle: s })
  },

  setDashboardRefreshSecs: (s) => {
    set({ dashboardRefreshSecs: s })
    persist({ ...getStored(), dashboardRefreshSecs: s })
  },

  setSidebarMode: (m) => {
    set({ sidebarMode: m })
    persist({ ...getStored(), sidebarMode: m })
  },

  loadFromStorage: () => {
    const stored = getStored()
    const theme = (stored.theme as any) || 'dark'
    const accent = stored.accentColor || '#38bdf8'
    applyTheme(theme)
    applyAccent(accent)
    set({
      theme,
      accentColor: accent,
      brandName: stored.brandName || 'CoreVM',
      brandSubtitle: stored.brandSubtitle || 'V2.4.0-ENTERPRISE',
      dashboardRefreshSecs: stored.dashboardRefreshSecs || 10,
      sidebarMode: (stored.sidebarMode as SidebarMode) || 'modules',
    })
  },
}))
