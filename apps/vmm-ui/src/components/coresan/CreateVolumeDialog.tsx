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
  // RAID disk requirements
  const claimedDisks = status?.claimed_disks || 0
  const minDisks: Record<string, number> = { stripe: 1, mirror: 2, stripe_mirror: 4 }

  // Get raw disk capacity from SAN status (available even before any volumes exist)
  const rawTotal = status?.storage_total_bytes || 0
  const rawFree = status?.storage_free_bytes || 0

  // RAID-corrected capacity based on selected RAID mode and claimed disk count
  const raidFactor = newVolRaid === 'mirror' ? (claimedDisks || 1) :
                     newVolRaid === 'stripe_mirror' ? 2 : 1
  const diskTotal = Math.floor(rawTotal / raidFactor)
  const diskFree = Math.floor(rawFree / raidFactor)

  const totalAllocated = volumes.reduce((sum, v) => sum + (v.max_size_bytes || 0), 0)
  const freeAfterAlloc = diskTotal > 0 ? Math.max(0, Math.min(diskFree, diskTotal - totalAllocated)) : 0
  const maxSizeGb = Math.floor(freeAfterAlloc / (1024 * 1024 * 1024))
  const raidMinDisks = minDisks[newVolRaid] || 1
  const raidDiskOk = claimedDisks >= raidMinDisks
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
        <FormField label={`Maximum Size (GB) — up to ${maxSizeGb} GB available`}>
          <input type="number" min={1} max={maxSizeGb || 999999} value={newVolSizeGb}
            onChange={(e) => setNewVolSizeGb(Math.max(1, Math.min(maxSizeGb || 999999, Number(e.target.value))))}
            className="w-full px-3 py-2 rounded-lg bg-vmm-input border border-vmm-border text-vmm-text text-sm
              focus:outline-none focus:ring-1 focus:ring-vmm-accent focus:border-vmm-accent" />
          {/* Capacity bar */}
          {diskTotal > 0 && (
            <div className="mt-2">
              <div className="flex justify-between text-[10px] text-vmm-text-muted mb-1">
                <span>Disk pool: {formatBytes(diskTotal)} total, {formatBytes(diskFree)} free</span>
                {totalAllocated > 0 && <span>{formatBytes(totalAllocated)} allocated</span>}
              </div>
              <div className="w-full h-2 bg-vmm-border rounded-full overflow-hidden flex">
                <div className="bg-vmm-accent h-full" style={{ width: `${Math.round(((diskTotal - diskFree) / diskTotal) * 100)}%` }} />
                <div className="bg-vmm-warning/50 h-full" style={{ width: `${Math.round((newVolSizeGb * 1024 * 1024 * 1024 / diskTotal) * 100)}%` }} />
              </div>
              <div className="flex gap-4 mt-1 text-[9px] text-vmm-text-muted">
                <span className="flex items-center gap-1"><span className="w-2 h-2 rounded-full bg-vmm-accent" /> Used</span>
                <span className="flex items-center gap-1"><span className="w-2 h-2 rounded-full bg-vmm-warning/50" /> This volume</span>
                <span className="flex items-center gap-1"><span className="w-2 h-2 rounded-full bg-vmm-border" /> Free</span>
              </div>
            </div>
          )}
          {newVolSizeGb > maxSizeGb && maxSizeGb > 0 && (
            <p className="text-[10px] text-vmm-danger mt-1 font-medium">
              Exceeds available space! Maximum is {maxSizeGb} GB.
            </p>
          )}
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
            { value: 'stripe', label: `Stripe (RAID-0) — 1+ disk${claimedDisks >= 1 ? '' : ' (need to claim disks!)'}` },
            { value: 'mirror', label: `Mirror (RAID-1) — 2+ disks${claimedDisks >= 2 ? '' : ' (need ' + (2 - claimedDisks) + ' more!)'}` },
            { value: 'stripe_mirror', label: `Stripe+Mirror (RAID-10) — 4+ disks${claimedDisks >= 4 ? '' : ' (need ' + (4 - claimedDisks) + ' more!)'}` },
          ]} />
          {!raidDiskOk && (
            <div className="flex items-center gap-2 mt-1.5 p-2 rounded-lg bg-vmm-danger/10 border border-vmm-danger/30 text-[11px] text-vmm-danger">
              <AlertTriangle size={14} />
              {newVolRaid} requires {raidMinDisks} claimed disks, but only {claimedDisks} available. Claim more disks first.
            </div>
          )}
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
          FUSE mount at <code className="text-vmm-accent">/vmm/san/{newVolName || '<name>'}</code> on each host.
          Data distributed across claimed disks with {newVolRaid} RAID.
        </p>

        <div className="flex justify-end gap-2 pt-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={onSubmit}
            disabled={
              !newVolName.trim() ||
              !raidDiskOk ||
              (maxSizeGb > 0 && newVolSizeGb > maxSizeGb) ||
              (newVolFtt > 0 && (1 + newVolSelectedHosts.length) < (newVolFtt + 1))
            }>
            Create Volume ({newVolSizeGb} GB, {newVolRaid}, FTT={newVolFtt})
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
