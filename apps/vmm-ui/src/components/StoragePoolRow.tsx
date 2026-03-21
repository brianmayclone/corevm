import { HardDrive, Plus, Edit, Trash2 } from 'lucide-react'
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
  const usageLabel = usagePercent > 80 ? 'High' : usagePercent > 60 ? 'Moderate' : 'Optimal'

  const menuItems = [
    ...(onEdit ? [{ label: 'Edit Pool', icon: <Edit size={14} />, onClick: onEdit }] : []),
    ...(onDelete ? [{ label: 'Delete Pool', icon: <Trash2 size={14} />, danger: true, onClick: onDelete }] : []),
  ]

  return (
    <div className="bg-vmm-surface border border-vmm-border rounded-xl px-4 py-3 sm:px-5 sm:py-4 hover:border-vmm-border-light transition-colors">
      {/* Top row: icon + name + actions */}
      <div className="flex items-center gap-3">
        <div className="w-9 h-9 sm:w-11 sm:h-11 rounded-lg bg-vmm-bg-alt flex items-center justify-center flex-shrink-0">
          <HardDrive size={16} className={isOffline ? 'text-vmm-danger' : 'text-vmm-text-muted'} />
        </div>

        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-1.5 flex-wrap">
            <span className="text-sm font-semibold text-vmm-text truncate">{pool.name}</span>
            <span className={`px-1.5 py-0.5 text-[9px] font-bold tracking-wider rounded border flex-shrink-0 ${typeBadgeColor}`}>
              {typeLabel}
            </span>
            {pool.shared && (
              <span className="px-1.5 py-0.5 text-[9px] font-bold tracking-wider rounded border bg-purple-500/20 text-purple-400 border-purple-500/30 flex-shrink-0">
                SHARED
              </span>
            )}
          </div>
          <div className="text-[11px] text-vmm-text-muted font-mono mt-0.5 truncate">
            {pool.mount_source || pool.path}
          </div>
        </div>

        <div className="flex items-center gap-1.5 flex-shrink-0">
          {onAdd && (
            <button onClick={onAdd}
              className="w-8 h-8 sm:w-9 sm:h-9 rounded-full bg-vmm-accent flex items-center justify-center hover:bg-vmm-accent-hover transition-colors cursor-pointer">
              <Plus size={14} className="text-white" />
            </button>
          )}
          {menuItems.length > 0 && <ContextMenu items={menuItems} />}
        </div>
      </div>

      {/* Bottom row: usage bar (visible when online) */}
      {isOffline ? (
        <div className="mt-2 flex items-center gap-2 ml-12 sm:ml-14">
          <span className="text-xs text-vmm-danger font-medium">DISCONNECTED</span>
        </div>
      ) : (
        <div className="mt-2 ml-12 sm:ml-14">
          <div className="flex items-center justify-between text-[10px] text-vmm-text-muted mb-1">
            <span>USAGE — {usageLabel}</span>
            <span>{formatBytes(usedBytes)} / {formatBytes(pool.total_bytes)}</span>
          </div>
          <ProgressBar value={usagePercent} color={usageColor} />
        </div>
      )}
    </div>
  )
}
