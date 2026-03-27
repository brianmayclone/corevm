import { Server, AlertTriangle } from 'lucide-react'
import type { CoreSanStatus, CoreSanVolume, Host } from '../../api/types'
import { formatBytes } from '../../utils/format'
import Dialog from '../Dialog'
import FormField from '../FormField'
import TextInput from '../TextInput'
import Select from '../Select'
import Button from '../Button'

interface Props {
  open: boolean
  onClose: () => void
  onSubmit: () => void
  status: CoreSanStatus | null
  sanHosts: Host[]
  availableHosts: Host[]
  newVolName: string
  setNewVolName: (v: string) => void
  newVolSizeGb: number
  setNewVolSizeGb: (v: number) => void
  newVolFtt: number
  setNewVolFtt: (v: number) => void
  newVolRaid: string
  setNewVolRaid: (v: string) => void
  newVolSelectedHosts: string[]
  setNewVolSelectedHosts: (v: string[] | ((prev: string[]) => string[])) => void
  newVolError: string
  volumes: CoreSanVolume[]
}

export default function CreateVolumeDialog({
  open, onClose, onSubmit, status, sanHosts, availableHosts,
  newVolName, setNewVolName, newVolSizeGb, setNewVolSizeGb, newVolFtt, setNewVolFtt, newVolRaid, setNewVolRaid,
  newVolSelectedHosts, setNewVolSelectedHosts, newVolError, volumes,
}: Props) {
  const totalCapacity = volumes.reduce((sum, v) => sum + v.total_bytes, 0)
  const totalUsed = volumes.reduce((sum, v) => sum + (v.total_bytes - v.free_bytes), 0)
  const totalAllocated = volumes.reduce((sum, v) => sum + (v.max_size_bytes || 0), 0)
  const freeAfterAlloc = totalCapacity > 0 ? Math.max(0, totalCapacity - totalAllocated) : 0
  return (
    <Dialog open={open} title="Create Volume" onClose={onClose} width="max-w-xl">
      <div className="space-y-4">
        {newVolError && (
          <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">
            {newVolError}
          </div>
        )}
        <FormField label="Volume Name">
          <TextInput value={newVolName} onChange={(e) => setNewVolName(e.target.value)} placeholder="e.g. pool-a" />
        </FormField>
        <FormField label="Maximum Size (GB)">
          <input type="number" min={1} value={newVolSizeGb}
            onChange={(e) => setNewVolSizeGb(Math.max(1, Number(e.target.value)))}
            className="w-full px-3 py-2 rounded-lg bg-vmm-input border border-vmm-border text-vmm-text text-sm
              focus:outline-none focus:ring-1 focus:ring-vmm-accent focus:border-vmm-accent" />
          <p className="text-[10px] text-vmm-text-muted mt-1">
            Available: {formatBytes(freeAfterAlloc)} of {formatBytes(totalCapacity)} total
            {totalAllocated > 0 && ` (${formatBytes(totalAllocated)} allocated to existing volumes)`}
          </p>
        </FormField>
        <FormField label="Failures To Tolerate (FTT)">
          <Select value={String(newVolFtt)} onChange={(e) => setNewVolFtt(Number(e.target.value))} options={[
            { value: '0', label: 'FTT=0 — No protection (data on 1 host only)' },
            { value: '1', label: 'FTT=1 — Tolerates 1 host failure (2 copies)' },
            { value: '2', label: 'FTT=2 — Tolerates 2 host failures (3 copies)' },
          ]} />
        </FormField>
        <FormField label="Local RAID (per host)">
          <Select value={newVolRaid} onChange={(e) => setNewVolRaid(e.target.value)} options={[
            { value: 'stripe', label: 'Stripe (RAID-0) — chunks distributed across local disks' },
            { value: 'mirror', label: 'Mirror (RAID-1) — every chunk on every local disk' },
            { value: 'stripe_mirror', label: 'Stripe+Mirror (RAID-10) — striped with local mirror' },
          ]} />
        </FormField>

        {/* Host Selection */}
        <div>
          <label className="block text-[11px] font-semibold tracking-widest text-vmm-text-muted uppercase mb-2">
            Hosts ({1 + newVolSelectedHosts.length} selected — {newVolFtt + 1} required for FTT={newVolFtt})
          </label>
          <div className="space-y-1.5">
            <div className="flex items-center gap-3 p-2.5 rounded-lg bg-vmm-accent/5 border border-vmm-accent/30">
              <input type="checkbox" checked disabled className="accent-vmm-accent" />
              <Server size={14} className="text-vmm-success" />
              <span className="text-sm text-vmm-text">{status?.hostname}</span>
              <span className="text-[10px] text-vmm-text-muted">(this node — always included)</span>
            </div>

            {sanHosts.filter(h => h.san_node_id && h.san_node_id !== status?.node_id).map(h => {
              const checked = newVolSelectedHosts.includes(h.id)
              return (
                <label key={h.id} className={`flex items-center gap-3 p-2.5 rounded-lg border cursor-pointer transition-colors
                  ${checked ? 'bg-vmm-accent/5 border-vmm-accent/30' : 'border-vmm-border hover:border-vmm-accent/20'}`}>
                  <input type="checkbox" checked={checked} onChange={() => {
                    setNewVolSelectedHosts(prev => checked ? prev.filter(id => id !== h.id) : [...prev, h.id])
                  }} className="accent-vmm-accent" />
                  <Server size={14} className={h.status === 'online' ? 'text-vmm-success' : 'text-vmm-text-muted'} />
                  <span className="text-sm text-vmm-text">{h.hostname}</span>
                  <span className="text-[10px] text-vmm-text-muted">{h.address}</span>
                </label>
              )
            })}

            {availableHosts.length > 0 && (
              <p className="text-[10px] text-vmm-text-muted pt-1">
                {availableHosts.length} host{availableHosts.length !== 1 ? 's' : ''} without CoreSAN not shown.
                Enable CoreSAN on them first.
              </p>
            )}

            {newVolFtt > 0 && (1 + newVolSelectedHosts.length) < (newVolFtt + 1) && (
              <div className="flex items-center gap-2 p-2.5 rounded-lg bg-vmm-warning/10 border border-vmm-warning/30 text-xs text-vmm-warning">
                <AlertTriangle size={14} />
                FTT={newVolFtt} requires {newVolFtt + 1} hosts. Select {newVolFtt - newVolSelectedHosts.length} more.
              </div>
            )}
          </div>
        </div>

        <p className="text-[10px] text-vmm-text-muted">
          Storage will be automatically provisioned at <code className="text-vmm-accent">/vmm/san-data/{newVolName || '<name>'}</code> on each host.
        </p>

        <div className="flex justify-end gap-2 pt-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={onSubmit}
            disabled={!newVolName.trim() || (newVolFtt > 0 && (1 + newVolSelectedHosts.length) < (newVolFtt + 1))}>
            Create Volume
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
