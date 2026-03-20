import { useNavigate } from 'react-router-dom'
import { Search, MonitorPlay, Plus, Bell, Settings, HelpCircle } from 'lucide-react'
import Button from './Button'
import { useAuthStore } from '../stores/authStore'

export default function Header() {
  const navigate = useNavigate()
  const { user, logout } = useAuthStore()

  return (
    <header className="h-14 bg-vmm-sidebar border-b border-vmm-border flex items-center justify-between px-5 gap-4">
      {/* Logo */}
      <div className="text-lg font-bold text-vmm-text tracking-tight">
        VMManager
      </div>

      {/* Search */}
      <div className="flex-1 max-w-md">
        <div className="relative">
          <Search size={15} className="absolute left-3 top-1/2 -translate-y-1/2 text-vmm-text-muted" />
          <input
            type="text"
            placeholder="Search cluster resources..."
            className="w-full bg-vmm-surface border border-vmm-border rounded-lg pl-9 pr-4 py-2
              text-sm text-vmm-text placeholder-vmm-text-muted
              focus:outline-none focus:border-vmm-accent/50"
          />
        </div>
      </div>

      {/* Actions */}
      <div className="flex items-center gap-2">
        <Button variant="outline" size="sm" icon={<MonitorPlay size={14} />}>Terminal</Button>
        <Button variant="primary" size="sm" icon={<Plus size={14} />} onClick={() => navigate('/vms/create')}>Create VM</Button>

        <div className="w-px h-6 bg-vmm-border mx-2" />

        <Button variant="ghost" size="icon"><Bell size={16} /></Button>
        <Button variant="ghost" size="icon"><Settings size={16} /></Button>
        <Button variant="ghost" size="icon"><HelpCircle size={16} /></Button>

        {/* Avatar */}
        <button
          onClick={logout}
          title={`${user?.username} — Click to logout`}
          className="w-8 h-8 rounded-full bg-vmm-accent/30 flex items-center justify-center
            text-xs font-bold text-vmm-accent hover:bg-vmm-accent/50 transition-colors ml-1 cursor-pointer"
        >
          {user?.username?.charAt(0).toUpperCase() || '?'}
        </button>
      </div>
    </header>
  )
}
