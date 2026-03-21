import { useNavigate } from 'react-router-dom'
import { Search, MonitorPlay, Plus, Bell, Settings, HelpCircle, Menu } from 'lucide-react'
import Button from './Button'
import { useAuthStore } from '../stores/authStore'
import { useUiStore } from '../stores/uiStore'

interface Props {
  onMenuToggle?: () => void
}

export default function Header({ onMenuToggle }: Props) {
  const navigate = useNavigate()
  const { user, logout } = useAuthStore()
  const { brandName } = useUiStore()

  return (
    <header className="h-14 bg-vmm-sidebar border-b border-vmm-border flex items-center justify-between px-3 sm:px-5 gap-2 sm:gap-4">
      {/* Mobile menu + Logo */}
      <div className="flex items-center gap-2">
        <button onClick={onMenuToggle} className="lg:hidden p-1.5 text-vmm-text-muted hover:text-vmm-text cursor-pointer">
          <Menu size={20} />
        </button>
        <div className="text-lg font-bold text-vmm-text tracking-tight hidden sm:block">
          {brandName}
        </div>
      </div>

      {/* Search — hidden on very small screens */}
      <div className="flex-1 max-w-md hidden md:block">
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
      <div className="flex items-center gap-1 sm:gap-2">
        <Button variant="outline" size="sm" icon={<MonitorPlay size={14} />} className="hidden md:inline-flex" onClick={() => navigate('/terminal')}>Terminal</Button>
        <Button variant="primary" size="sm" icon={<Plus size={14} />} onClick={() => navigate('/vms/create')}>
          <span className="hidden sm:inline">Create VM</span>
          <span className="sm:hidden">New</span>
        </Button>

        <div className="w-px h-6 bg-vmm-border mx-1 sm:mx-2 hidden sm:block" />

        <Button variant="ghost" size="icon" className="hidden sm:inline-flex"><Bell size={16} /></Button>
        <Button variant="ghost" size="icon" className="hidden md:inline-flex"><Settings size={16} /></Button>
        <Button variant="ghost" size="icon" className="hidden md:inline-flex"><HelpCircle size={16} /></Button>

        {/* Avatar */}
        <button
          onClick={logout}
          title={`${user?.username} — Click to logout`}
          className="w-8 h-8 rounded-full bg-vmm-accent/30 flex items-center justify-center
            text-xs font-bold text-vmm-accent hover:bg-vmm-accent/50 transition-colors ml-1 cursor-pointer flex-shrink-0"
        >
          {user?.username?.charAt(0).toUpperCase() || '?'}
        </button>
      </div>
    </header>
  )
}
