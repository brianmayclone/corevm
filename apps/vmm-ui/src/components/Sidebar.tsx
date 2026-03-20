import { NavLink } from 'react-router-dom'
import { Monitor, HardDrive, Network, Settings, Zap, LifeBuoy, FileText } from 'lucide-react'

const navItems = [
  { to: '/', icon: Monitor, label: 'My Machines' },
  { to: '/storage', icon: HardDrive, label: 'Storage' },
  { to: '/networks', icon: Network, label: 'Networks' },
  { to: '/settings', icon: Settings, label: 'Settings' },
]

export default function Sidebar() {
  return (
    <aside className="w-56 bg-vmm-sidebar border-r border-vmm-border flex flex-col h-full">
      {/* Branding */}
      <div className="px-5 pt-5 pb-4">
        <div className="flex items-center gap-2.5">
          <div className="w-8 h-8 rounded-lg bg-vmm-accent/20 flex items-center justify-center">
            <Monitor size={16} className="text-vmm-accent" />
          </div>
          <div>
            <div className="text-sm font-bold text-vmm-text">CoreVM</div>
            <div className="text-[10px] text-vmm-text-muted font-mono tracking-wider">V2.4.0-ENTERPRISE</div>
          </div>
        </div>
      </div>

      {/* Navigation */}
      <nav className="flex-1 px-3 space-y-0.5">
        {navItems.map(({ to, icon: Icon, label }) => (
          <NavLink
            key={to}
            to={to}
            end={to === '/'}
            className={({ isActive }) =>
              `flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors
              ${isActive
                ? 'bg-vmm-sidebar-active text-vmm-accent border-l-2 border-vmm-accent'
                : 'text-vmm-text-dim hover:text-vmm-text hover:bg-vmm-surface-hover'
              }`
            }
          >
            <Icon size={18} />
            {label}
          </NavLink>
        ))}
      </nav>

      {/* Upgrade banner */}
      <div className="px-3 pb-3">
        <button className="w-full flex items-center justify-center gap-2 px-4 py-2.5
          bg-vmm-accent/10 hover:bg-vmm-accent/20 border border-vmm-accent/30
          rounded-lg text-sm font-medium text-vmm-accent transition-colors cursor-pointer">
          <Zap size={14} />
          Upgrade Cluster
        </button>
      </div>

      {/* Footer links */}
      <div className="border-t border-vmm-border px-4 py-3 space-y-1">
        <a href="#" className="flex items-center gap-2 text-xs text-vmm-text-muted hover:text-vmm-text-dim">
          <LifeBuoy size={13} /> Support
        </a>
        <a href="#" className="flex items-center gap-2 text-xs text-vmm-text-muted hover:text-vmm-text-dim">
          <FileText size={13} /> Logs
        </a>
      </div>
    </aside>
  )
}
