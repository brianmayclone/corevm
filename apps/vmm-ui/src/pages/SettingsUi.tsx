/** UI Settings — branding, theme, accent color. All changes apply immediately. */
import { Moon, Sun, Monitor } from 'lucide-react'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import { useUiStore } from '../stores/uiStore'

export default function SettingsUi() {
  const {
    theme, accentColor, brandName, dashboardRefreshSecs,
    setTheme, setAccentColor, setBrandName, setDashboardRefreshSecs,
  } = useUiStore()

  const accents = ['#38bdf8', '#818cf8', '#a78bfa', '#f472b6', '#34d399', '#fbbf24', '#ef4444']

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">UI Settings</h1>
        <p className="text-sm text-vmm-text-muted mt-1">Customize appearance and branding — changes apply immediately</p>
      </div>

      {/* Theme */}
      <Card>
        <SectionLabel className="mb-4">Theme</SectionLabel>
        <div className="grid grid-cols-3 gap-3">
          {([
            { id: 'dark', label: 'Dark Mode', icon: <Moon size={20} />, desc: 'Optimized for low-light environments' },
            { id: 'light', label: 'Light Mode', icon: <Sun size={20} />, desc: 'Bright theme for well-lit spaces' },
            { id: 'system', label: 'System', icon: <Monitor size={20} />, desc: 'Follow operating system preference' },
          ] as const).map((t) => (
            <button key={t.id} onClick={() => setTheme(t.id)}
              className={`p-4 rounded-xl border text-left transition-colors cursor-pointer
                ${theme === t.id
                  ? 'bg-vmm-accent/10 border-vmm-accent text-vmm-accent'
                  : 'bg-vmm-bg-alt border-vmm-border text-vmm-text-dim hover:border-vmm-border-light'}`}
            >
              <div className="flex items-center gap-2 mb-2">{t.icon}<span className="font-semibold text-sm">{t.label}</span></div>
              <p className="text-xs text-vmm-text-muted">{t.desc}</p>
            </button>
          ))}
        </div>
      </Card>

      {/* Accent Color */}
      <Card>
        <SectionLabel className="mb-4">Accent Color</SectionLabel>
        <div className="flex items-center gap-3">
          {accents.map((color) => (
            <button key={color} onClick={() => setAccentColor(color)}
              className={`w-10 h-10 rounded-xl cursor-pointer transition-transform
                ${accentColor === color ? 'ring-2 ring-offset-2 ring-offset-vmm-bg scale-110' : 'hover:scale-105'}`}
              style={{ backgroundColor: color }}
            />
          ))}
          <div className="w-px h-8 bg-vmm-border mx-1" />
          <input type="color" value={accentColor} onChange={(e) => setAccentColor(e.target.value)}
            className="w-10 h-10 rounded-lg cursor-pointer border-0 bg-transparent" title="Custom color" />
        </div>
        <p className="text-xs text-vmm-text-muted mt-2">
          Current: <span className="font-mono" style={{ color: accentColor }}>{accentColor}</span>
        </p>
      </Card>

      {/* Branding */}
      <Card>
        <SectionLabel className="mb-4">Branding</SectionLabel>
        <div>
          <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Product Name</label>
          <input value={brandName} onChange={(e) => setBrandName(e.target.value)}
            className="w-full max-w-xs bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none"
            placeholder="CoreVM" />
          <p className="text-xs text-vmm-text-muted mt-1">Shown in the sidebar header</p>
        </div>
      </Card>

      {/* Dashboard */}
      <Card>
        <SectionLabel className="mb-4">Dashboard</SectionLabel>
        <div>
          <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Auto-Refresh Interval (seconds)</label>
          <input type="number" min={1} max={300} value={dashboardRefreshSecs}
            onChange={(e) => setDashboardRefreshSecs(Math.max(1, parseInt(e.target.value) || 10))}
            className="w-full max-w-xs bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none" />
          <p className="text-xs text-vmm-text-muted mt-1">How often the dashboard polls for updates</p>
        </div>
      </Card>
    </div>
  )
}
