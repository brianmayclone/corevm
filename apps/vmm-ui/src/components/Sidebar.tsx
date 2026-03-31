import { useState } from 'react'
import { NavLink, useLocation } from 'react-router-dom'
import { useUiStore } from '../stores/uiStore'
import { useClusterStore } from '../stores/clusterStore'
import InventoryTree from './InventoryTree'
import { Monitor, HardDrive, Network, Settings, Zap, LifeBuoy, FileText, ChevronDown, Cable, Layers, Globe, Unplug, Database, Share2, Gauge, Disc, Palette, Users, Clock, Server, Shield, LayoutDashboard, List, FolderOpen, Activity, Bell, CheckSquare, Workflow, Boxes, TreePine, LayoutList } from 'lucide-react'

interface NavItem {
  to: string
  icon: React.ElementType
  label: string
  children?: { to: string; icon: React.ElementType; label: string }[]
}

/** Navigation items for standalone vmm-server mode */
const standaloneNavItems: NavItem[] = [
  {
    to: '/machines', icon: Monitor, label: 'Machines',
    children: [
      { to: '/machines/overview', icon: LayoutDashboard, label: 'Dashboard' },
      { to: '/machines/list', icon: List, label: 'All Machines' },
      { to: '/machines/resource-groups', icon: FolderOpen, label: 'Resource Groups' },
    ],
  },
  {
    to: '/storage', icon: HardDrive, label: 'Storage',
    children: [
      { to: '/storage/overview', icon: HardDrive, label: 'Overview' },
      { to: '/storage/local', icon: Database, label: 'Local Storage' },
      { to: '/storage/shared', icon: Share2, label: 'Shared Storage' },
      { to: '/storage/coresan', icon: Boxes, label: 'CoreSAN' },
      { to: '/storage/object-storage', icon: Globe, label: 'Object Storage' },
      { to: '/storage/disks', icon: Disc, label: 'Disk Management' },
      { to: '/storage/qos', icon: Gauge, label: 'QoS Policies' },
    ],
  },
  {
    to: '/networks', icon: Network, label: 'Networks',
    children: [
      { to: '/networks/overview', icon: Network, label: 'Overview' },
      { to: '/networks/nat', icon: Globe, label: 'NAT Bridges' },
      { to: '/networks/host-only', icon: Unplug, label: 'Host-Only' },
      { to: '/networks/adapters', icon: Cable, label: 'Adapter Bindings' },
      { to: '/networks/vlans', icon: Layers, label: 'VLAN Config' },
    ],
  },
  {
    to: '/settings', icon: Settings, label: 'Settings',
    children: [
      { to: '/settings/ui', icon: Palette, label: 'UI & Branding' },
      { to: '/settings/users', icon: Users, label: 'Users' },
      { to: '/settings/groups', icon: Shield, label: 'Groups & Roles' },
      { to: '/settings/time', icon: Clock, label: 'Date & Time' },
      { to: '/settings/server', icon: Server, label: 'Server' },
    ],
  },
]

/** Additional navigation items when connected to vmm-cluster */
const clusterNavItems: NavItem[] = [
  { to: '/', icon: LayoutDashboard, label: 'Overview' },
  {
    to: '/cluster', icon: Workflow, label: 'Cluster',
    children: [
      { to: '/cluster/hosts', icon: Server, label: 'Hosts' },
      { to: '/cluster/settings', icon: Settings, label: 'Clusters' },
      { to: '/cluster/drs', icon: Activity, label: 'DRS' },
    ],
  },
  {
    to: '/machines', icon: Monitor, label: 'Machines',
    children: [
      { to: '/machines/overview', icon: LayoutDashboard, label: 'Dashboard' },
      { to: '/machines/list', icon: List, label: 'All Machines' },
      { to: '/machines/resource-groups', icon: FolderOpen, label: 'Resource Groups' },
    ],
  },
  {
    to: '/networks', icon: Network, label: 'Networks',
    children: [
      { to: '/networks/topology', icon: Share2, label: 'Topology' },
      { to: '/networks/overview', icon: Network, label: 'Virtual Networks' },
      { to: '/networks/viswitches', icon: Cable, label: 'viSwitches' },
    ],
  },
  {
    to: '/storage', icon: HardDrive, label: 'Storage',
    children: [
      { to: '/storage/overview', icon: HardDrive, label: 'Overview' },
      { to: '/storage/coresan', icon: Boxes, label: 'CoreSAN' },
      { to: '/storage/object-storage', icon: Globe, label: 'Object Storage' },
      { to: '/storage/disks', icon: Disc, label: 'Disk Management' },
    ],
  },
  {
    to: '/operations', icon: Activity, label: 'Operations',
    children: [
      { to: '/operations/logs', icon: FileText, label: 'Logs' },
      { to: '/operations/tasks', icon: CheckSquare, label: 'Tasks' },
      { to: '/operations/events', icon: Bell, label: 'Events' },
      { to: '/operations/alarms', icon: Bell, label: 'Alarms' },
      { to: '/operations/notifications', icon: Bell, label: 'Notifications' },
    ],
  },
  {
    to: '/settings', icon: Settings, label: 'Settings',
    children: [
      { to: '/settings/ui', icon: Palette, label: 'UI & Branding' },
      { to: '/settings/users', icon: Users, label: 'Users' },
      { to: '/settings/groups', icon: Shield, label: 'Groups & Roles' },
      { to: '/settings/time', icon: Clock, label: 'Date & Time' },
      { to: '/settings/server', icon: Server, label: 'Server' },
    ],
  },
]

interface SidebarProps {
  onNavigate?: () => void
}

export default function Sidebar({ onNavigate }: SidebarProps) {
  const location = useLocation()
  const { brandName, brandSubtitle, sidebarMode, setSidebarMode } = useUiStore()
  const { backendMode } = useClusterStore()

  const isClusterMode = backendMode === 'cluster'
  const navItems = isClusterMode ? clusterNavItems : standaloneNavItems

  const [expandedSections, setExpandedSections] = useState<Set<string>>(
    () => new Set(navItems.filter(i => i.children && (location.pathname.startsWith(i.to + '/') || location.pathname === i.to)).map(i => i.to))
  )

  const toggleSection = (to: string) => {
    setExpandedSections((prev) => {
      const next = new Set(prev)
      if (next.has(to)) next.delete(to)
      else next.add(to)
      return next
    })
  }

  const handleUpgradeCluster = () => {
    const clusterUrl = prompt(
      'Enter the URL of the running vmm-cluster instance:\n\n(e.g. http://localhost:9443)',
      'http://localhost:9443'
    )
    if (!clusterUrl) return
    window.location.href = clusterUrl
  }

  return (
    <aside className="w-56 bg-vmm-sidebar border-r border-vmm-border flex flex-col h-full">
      {/* Branding */}
      <div className="px-5 pt-5 pb-2">
        <div className="flex items-center gap-2.5">
          <div className="w-8 h-8 rounded-lg bg-vmm-accent/20 flex items-center justify-center">
            {isClusterMode ? <Workflow size={16} className="text-vmm-accent" /> : <Monitor size={16} className="text-vmm-accent" />}
          </div>
          <div>
            <div className="text-sm font-bold text-vmm-text">{brandName}</div>
            <div className="text-[10px] text-vmm-text-muted font-mono tracking-wider">
              {isClusterMode ? 'CLUSTER' : brandSubtitle}
            </div>
          </div>
        </div>
      </div>

      {/* View mode toggle */}
      <div className="px-3 pb-2">
        <div className="flex rounded-lg bg-vmm-bg/50 p-0.5 border border-vmm-border/50">
          <button
            onClick={() => setSidebarMode('modules')}
            className={`flex-1 flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-md text-[11px] font-medium transition-all duration-200 cursor-pointer
              ${sidebarMode === 'modules'
                ? 'bg-vmm-accent/15 text-vmm-accent shadow-sm'
                : 'text-vmm-text-muted hover:text-vmm-text'
              }`}
          >
            <LayoutList size={12} />
            Modules
          </button>
          <button
            onClick={() => setSidebarMode('inventory')}
            className={`flex-1 flex items-center justify-center gap-1.5 px-2 py-1.5 rounded-md text-[11px] font-medium transition-all duration-200 cursor-pointer
              ${sidebarMode === 'inventory'
                ? 'bg-vmm-accent/15 text-vmm-accent shadow-sm'
                : 'text-vmm-text-muted hover:text-vmm-text'
              }`}
          >
            <TreePine size={12} />
            Inventory
          </button>
        </div>
      </div>

      {/* Navigation content */}
      {sidebarMode === 'modules' ? (
        <nav className="flex-1 px-3 space-y-0.5 overflow-y-auto">
          {navItems.map((item) => {
            const Icon = item.icon
            const hasChildren = !!item.children
            const isExpanded = expandedSections.has(item.to)
            const isParentActive = hasChildren && location.pathname.startsWith(item.to)

            if (hasChildren) {
              return (
                <div key={item.to}>
                  <button
                    onClick={() => toggleSection(item.to)}
                    className={`w-full flex items-center justify-between gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors cursor-pointer
                      ${isParentActive
                        ? 'bg-vmm-sidebar-active text-vmm-accent border-l-2 border-vmm-accent'
                        : 'text-vmm-text-dim hover:text-vmm-text hover:bg-vmm-surface-hover'
                      }`}
                  >
                    <span className="flex items-center gap-3">
                      <Icon size={18} />
                      {item.label}
                    </span>
                    <ChevronDown
                      size={14}
                      className={`transition-transform duration-200 ${isExpanded ? 'rotate-180' : ''}`}
                    />
                  </button>

                  {isExpanded && (
                    <div className="mt-0.5 ml-4 pl-3 border-l border-vmm-border space-y-0.5">
                      {item.children!.map((child) => {
                        const ChildIcon = child.icon
                        return (
                          <NavLink
                            key={child.to}
                            to={child.to}
                            onClick={onNavigate}
                            className={({ isActive }) =>
                              `flex items-center gap-2.5 px-3 py-2 rounded-lg text-[13px] font-medium transition-colors
                              ${isActive
                                ? 'bg-vmm-sidebar-active text-vmm-accent'
                                : 'text-vmm-text-muted hover:text-vmm-text hover:bg-vmm-surface-hover'
                              }`
                            }
                          >
                            <ChildIcon size={15} />
                            {child.label}
                          </NavLink>
                        )
                      })}
                    </div>
                  )}
                </div>
              )
            }

            return (
              <NavLink
                key={item.to}
                to={item.to}
                end={item.to === '/'}
                onClick={onNavigate}
                className={({ isActive }) =>
                  `flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-colors
                  ${isActive
                    ? 'bg-vmm-sidebar-active text-vmm-accent border-l-2 border-vmm-accent'
                    : 'text-vmm-text-dim hover:text-vmm-text hover:bg-vmm-surface-hover'
                  }`
                }
              >
                <Icon size={18} />
                {item.label}
              </NavLink>
            )
          })}
        </nav>
      ) : (
        <div className="flex-1 px-2 overflow-y-auto">
          <InventoryTree onNavigate={onNavigate} />
        </div>
      )}

      {/* Upgrade banner — only shown in standalone mode */}
      {!isClusterMode && (
        <div className="px-3 pb-3">
          <button
            onClick={handleUpgradeCluster}
            className="w-full flex items-center justify-center gap-2 px-4 py-2.5
              bg-vmm-accent/10 hover:bg-vmm-accent/20 border border-vmm-accent/30
              rounded-lg text-sm font-medium text-vmm-accent transition-colors cursor-pointer"
          >
            <Zap size={14} />
            Upgrade Cluster
          </button>
        </div>
      )}

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
