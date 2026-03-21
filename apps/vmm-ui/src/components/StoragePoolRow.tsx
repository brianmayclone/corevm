import { HardDrive, Plus, Edit, Trash2, Power, PowerOff } from 'lucide-react'
import ProgressBar from './ProgressBar'
import ContextMenu from './ContextMenu'
import { formatBytes } from '../utils/format'
import type { StoragePool } from '../api/types'

interface Props {
  pool: StoragePool
  onAdd?: () => void
  onEdit?: () => void
  onDelete?: () => void
}

export default function StoragePoolRow({ pool, onAdd, onEdit, onDelete }: Props) {
  const usedBytes = pool.total_bytes - pool.free_bytes
  const usagePercent = pool.total_bytes > 0 ? Math.round((usedBytes / pool.total_bytes) * 100) : 0
  const isOffline = pool.total_bytes === 0

  const typeLabel = pool.pool_type.toUpperCase()
  const typeBadgeColor = pool.shared
    ? 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30'
    : 'bg-vmm-text-muted/20 text-vmm-text-dim border-vmm-text-muted/30'

  const usageColor = usagePercent > 80 ? 'bg-vmm-danger' : usagePercent > 60 ? 'bg-vmm-warning' : 'bg-vmm-accent'
  const usageLabel = usagePercent > 80 ? 'High Utilization' : usagePercent > 60 ? 'Moderate' : 'Optimal Performance'

  const menuItems = [
    ...(onEdit ? [{ label: 'Edit Pool', icon: <Edit size={14} />, onClick: onEdit }] : []),
    ...(onDelete ? [{ label: 'Delete Pool', icon: <Trash2 size={14} />, danger: true, onClick: onDelete }] : []),
  ]

  return (
    <div className="flex items-center gap-4 bg-vmm-surface border border-vmm-border rounded-xl px-5 py-4 hover:border-vmm-border-light transition-colors">
      <div className="w-11 h-11 rounded-lg bg-vmm-bg-alt flex items-center justify-center flex-shrink-0">
        <HardDrive size={18} className={isOffline ? 'text-vmm-danger' : 'text-vmm-text-muted'} />
      </div>

      <div className="min-w-[200px]">
        <div className="flex items-center gap-2">
          <span className="text-sm font-semibold text-vmm-text">{pool.name}</span>
          <span className={`px-1.5 py-0.5 text-[9px] font-bold tracking-wider rounded border ${typeBadgeColor}`}>
            {typeLabel}
          </span>
          {pool.shared && (
            <span className="px-1.5 py-0.5 text-[9px] font-bold tracking-wider rounded border bg-purple-500/20 text-purple-400 border-purple-500/30">
              SHARED
            </span>
          )}
        </div>
        <div className="text-xs text-vmm-text-muted font-mono mt-0.5">
          {pool.mount_source || pool.path}
        </div>
      </div>

      <div className="flex-1 flex items-center gap-4">
        {isOffline ? (
          <>
            <span className="text-xs text-vmm-text-muted">STATE</span>
            <span className="text-xs text-vmm-danger font-medium">DISCONNECTED</span>
          </>
        ) : (
          <>
            <span className="text-[10px] text-vmm-text-muted uppercase tracking-wider">USAGE</span>
            <div className="w-32">
              <ProgressBar value={usagePercent} color={usageColor} />
            </div>
            <span className="text-xs text-vmm-text-muted">{usagePercent}%</span>
            <div className="text-right">
              <div className="text-sm font-medium text-vmm-text">
                {formatBytes(usedBytes)} / {formatBytes(pool.total_bytes)}
              </div>
              <div className="text-[10px] text-vmm-text-muted">{usageLabel}</div>
            </div>
          </>
        )}
      </div>

      <div className="flex items-center gap-1.5 ml-4">
        {onAdd && (
          <button onClick={onAdd}
            className="w-9 h-9 rounded-full bg-vmm-accent flex items-center justify-center hover:bg-vmm-accent-hover transition-colors cursor-pointer">
            <Plus size={16} className="text-white" />
          </button>
        )}
        {menuItems.length > 0 && <ContextMenu items={menuItems} />}
      </div>
    </div>
  )
}
