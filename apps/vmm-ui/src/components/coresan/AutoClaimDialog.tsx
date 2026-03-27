import { Server, Disc, AlertTriangle } from 'lucide-react'
import type { CoreSanStatus, DiscoveredDisk } from '../../api/types'
import Dialog from '../Dialog'
import Button from '../Button'
import { Badge, statusColors } from './constants'
import { formatBytes } from '../../utils/format'

interface Props {
  open: boolean
  onClose: () => void
  onSubmit: () => void
  disks: DiscoveredDisk[]
  diskKey: (d: DiscoveredDisk) => string
  status: CoreSanStatus | null
  autoClaimSelected: Set<string>
  setAutoClaimSelected: (v: Set<string> | ((prev: Set<string>) => Set<string>)) => void
  autoClaimRunning: boolean
  autoClaimError: string
}

export default function AutoClaimDialog({
  open, onClose, onSubmit, disks, diskKey, status,
  autoClaimSelected, setAutoClaimSelected, autoClaimRunning, autoClaimError,
}: Props) {
  const unclaimedDisks = disks.filter(d => d.status !== 'claimed')
  const groups: Record<string, { label: string; disks: DiscoveredDisk[] }> = {}
  for (const d of unclaimedDisks) {
    const groupKey = d._host_id || '__local__'
    if (!groups[groupKey]) {
      groups[groupKey] = {
        label: d._host_name || status?.hostname || 'This node',
        disks: [],
      }
    }
    groups[groupKey].disks.push(d)
  }

  return (
    <Dialog open={open} title="Auto-Claim Disks" onClose={onClose} width="max-w-4xl">
      <div className="space-y-4">
        <p className="text-sm text-vmm-text-dim">
          Select disks to claim for CoreSAN. Empty disks are pre-selected.
          Disks with existing data must be explicitly selected (they will be formatted).
          OS disks cannot be selected.
        </p>

        {autoClaimError && (
          <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">{autoClaimError}</div>
        )}

        <div className="space-y-4 max-h-[50vh] overflow-y-auto">
          {Object.entries(groups).map(([groupKey, group]) => (
            <div key={groupKey}>
              <div className="flex items-center gap-2 mb-2">
                <Server size={14} className="text-vmm-success" />
                <span className="text-xs font-bold text-vmm-text uppercase tracking-wider">{group.label}</span>
                {groupKey === '__local__' && <span className="text-[10px] text-vmm-text-muted">(this node)</span>}
              </div>
              <div className="space-y-1.5 ml-5">
                {group.disks.map(d => {
                  const key = diskKey(d)
                  const isOsDisk = d.status === 'os_disk' || d.status === 'in_use'
                  const hasData = d.status === 'has_data'
                  const checked = autoClaimSelected.has(key)
                  return (
                    <label key={key} className={`flex items-center gap-3 p-3 rounded-lg border transition-colors
                      ${isOsDisk ? 'opacity-40 cursor-not-allowed border-vmm-border' :
                        checked ? 'bg-vmm-accent/5 border-vmm-accent/30 cursor-pointer' :
                        'border-vmm-border hover:border-vmm-accent/20 cursor-pointer'}`}>
                      <input
                        type="checkbox"
                        checked={checked}
                        disabled={isOsDisk}
                        onChange={() => {
                          setAutoClaimSelected(prev => {
                            const next = new Set(prev)
                            if (next.has(key)) next.delete(key)
                            else next.add(key)
                            return next
                          })
                        }}
                        className="accent-vmm-accent"
                      />
                      <Disc size={16} className={isOsDisk ? 'text-vmm-danger' : checked ? 'text-vmm-accent' : 'text-vmm-text-muted'} />
                      <div className="flex-1 min-w-0">
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-mono font-medium text-vmm-text">{d.path}</span>
                          <span className="text-xs text-vmm-text-dim">{formatBytes(d.size_bytes)}</span>
                          {d.model && <span className="text-xs text-vmm-text-muted">{d.model}</span>}
                        </div>
                      </div>
                      <div className="shrink-0">
                        {isOsDisk && <Badge label="OS DISK" color="bg-vmm-danger/20 text-vmm-danger border-vmm-danger/30" />}
                        {d.status === 'in_use' && <Badge label="IN USE" color={statusColors.offline} />}
                        {d.status === 'available' && <Badge label="EMPTY" color={statusColors.online} />}
                        {hasData && <Badge label={`HAS DATA (${d.fs_type || '?'})`} color={statusColors.degraded} />}
                      </div>
                    </label>
                  )
                })}
                {group.disks.length === 0 && (
                  <p className="text-xs text-vmm-text-muted py-2">No unclaimed disks on this node.</p>
                )}
              </div>
            </div>
          ))}
        </div>

        {autoClaimSelected.size > 0 && disks.some(d => autoClaimSelected.has(diskKey(d)) && d.status === 'has_data') && (
          <div className="flex items-center gap-2 p-3 rounded-lg bg-vmm-warning/10 border border-vmm-warning/30 text-xs text-vmm-warning">
            <AlertTriangle size={14} />
            {disks.filter(d => autoClaimSelected.has(diskKey(d)) && d.status === 'has_data').length} disk(s)
            with existing data selected. They will be wiped and formatted!
          </div>
        )}

        <div className="flex items-center justify-between pt-2 border-t border-vmm-border">
          <span className="text-sm text-vmm-text-dim">
            {autoClaimSelected.size} disk{autoClaimSelected.size !== 1 ? 's' : ''} selected
            ({formatBytes(disks.filter(d => autoClaimSelected.has(diskKey(d))).reduce((s, d) => s + d.size_bytes, 0))} total)
          </span>
          <div className="flex items-center gap-2">
            <Button variant="ghost" onClick={onClose}>Cancel</Button>
            <Button variant="primary" onClick={onSubmit}
              disabled={autoClaimSelected.size === 0 || autoClaimRunning}>
              {autoClaimRunning ? 'Claiming...' : `Claim ${autoClaimSelected.size} Disk${autoClaimSelected.size !== 1 ? 's' : ''}`}
            </Button>
          </div>
        </div>
      </div>
    </Dialog>
  )
}
