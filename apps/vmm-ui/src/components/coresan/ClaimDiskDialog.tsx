import { Disc, AlertTriangle } from 'lucide-react'
import type { DiscoveredDisk } from '../../api/types'
import Dialog from '../Dialog'
import Button from '../Button'
import { formatBytes } from '../../utils/format'

interface Props {
  disk: DiscoveredDisk | null
  onClose: () => void
  onSubmit: () => void
  claimConfirm: boolean
  setClaimConfirm: (v: boolean) => void
  claimError: string
}

export default function ClaimDiskDialog({
  disk, onClose, onSubmit, claimConfirm, setClaimConfirm, claimError,
}: Props) {
  return (
    <Dialog open={!!disk} title="Claim Disk" onClose={onClose}>
      {disk && (
        <div className="space-y-4">
          <div className="flex items-center gap-3 p-3 rounded-lg bg-vmm-bg/50 border border-vmm-border">
            <Disc size={20} className="text-vmm-accent" />
            <div>
              <div className="text-sm font-bold text-vmm-text">{disk.path}</div>
              <div className="text-xs text-vmm-text-muted">{formatBytes(disk.size_bytes)} — {disk.model || 'Unknown model'}</div>
            </div>
          </div>

          {disk.status === 'has_data' && (
            <div className="flex items-center gap-2 p-3 rounded-lg bg-vmm-danger/10 border border-vmm-danger/30 text-sm text-vmm-danger">
              <AlertTriangle size={16} />
              This disk has existing data ({disk.fs_type || 'unknown'}). It will be wiped and reformatted. All data will be lost!
            </div>
          )}

          {claimError && (
            <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">{claimError}</div>
          )}

          {disk.status === 'has_data' && (
            <label className="flex items-center gap-2 cursor-pointer">
              <input type="checkbox" checked={claimConfirm} onChange={e => setClaimConfirm(e.target.checked)} className="accent-vmm-danger" />
              <span className="text-sm text-vmm-text">I confirm all data on this disk will be destroyed</span>
            </label>
          )}

          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" onClick={onClose}>Cancel</Button>
            <Button variant="primary" onClick={onSubmit}
              disabled={disk.status === 'has_data' && !claimConfirm}>
              Claim & Format Disk
            </Button>
          </div>
        </div>
      )}
    </Dialog>
  )
}
