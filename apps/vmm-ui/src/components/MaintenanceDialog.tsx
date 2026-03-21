import { useEffect, useState } from 'react'
import { Wrench, ArrowRightLeft, Power, AlertTriangle, Server, Monitor } from 'lucide-react'
import { useVmStore } from '../stores/vmStore'
import Dialog from './Dialog'
import Button from './Button'
import type { Host, VmSummary } from '../api/types'

interface Props {
  open: boolean
  onClose: () => void
  host: Host | null
  onConfirm: (mode: 'migrate' | 'shutdown') => void
}

export default function MaintenanceDialog({ open, onClose, host, onConfirm }: Props) {
  const { vms } = useVmStore()
  const [mode, setMode] = useState<'migrate' | 'shutdown'>('migrate')

  if (!host) return null

  const hostVms = vms.filter((v: any) => v.host_id === host.id)
  const runningVms = hostVms.filter(v => v.state === 'running')
  const stoppedVms = hostVms.filter(v => v.state === 'stopped')

  return (
    <Dialog open={open} onClose={onClose} title="Enter Maintenance Mode" width="max-w-lg">
      <div className="space-y-5">
        {/* Host info */}
        <div className="flex items-center gap-3 bg-vmm-bg-alt border border-vmm-border rounded-lg px-4 py-3">
          <Server size={18} className="text-vmm-text-muted" />
          <div>
            <div className="text-sm font-medium text-vmm-text">{host.hostname}</div>
            <div className="text-xs text-vmm-text-muted">{host.address}</div>
          </div>
        </div>

        {/* VM impact summary */}
        <div className="bg-yellow-500/10 border border-yellow-500/30 rounded-lg p-4">
          <div className="flex items-center gap-2 text-yellow-400 text-sm font-semibold mb-2">
            <AlertTriangle size={14} />
            {runningVms.length > 0
              ? `${runningVms.length} running VM${runningVms.length > 1 ? 's' : ''} will be affected`
              : 'No running VMs on this host'}
          </div>
          {runningVms.length > 0 && (
            <div className="space-y-1 mt-2">
              {runningVms.map(vm => (
                <div key={vm.id} className="flex items-center gap-2 text-xs text-vmm-text-dim">
                  <Monitor size={11} />
                  <span>{vm.name}</span>
                  <span className="text-vmm-text-muted">({vm.ram_mb} MB, {vm.cpu_cores} vCPU)</span>
                </div>
              ))}
            </div>
          )}
          {stoppedVms.length > 0 && (
            <div className="text-xs text-vmm-text-muted mt-2">
              + {stoppedVms.length} stopped VM{stoppedVms.length > 1 ? 's' : ''} (not affected)
            </div>
          )}
        </div>

        {/* Evacuation mode */}
        {runningVms.length > 0 && (
          <div>
            <div className="text-sm font-medium text-vmm-text mb-3">How should running VMs be handled?</div>
            <div className="space-y-2">
              <div
                onClick={() => setMode('migrate')}
                className={`flex items-center gap-3 px-4 py-3 rounded-lg border cursor-pointer transition-colors ${
                  mode === 'migrate'
                    ? 'border-vmm-accent/50 bg-vmm-accent/5'
                    : 'border-vmm-border bg-vmm-bg-alt hover:border-vmm-border-light'
                }`}
              >
                <ArrowRightLeft size={16} className={mode === 'migrate' ? 'text-vmm-accent' : 'text-vmm-text-muted'} />
                <div>
                  <div className="text-sm font-medium text-vmm-text">Migrate to other hosts</div>
                  <div className="text-xs text-vmm-text-muted">
                    VMs are stopped, moved to another host with shared storage, and restarted (cold migration)
                  </div>
                </div>
              </div>
              <div
                onClick={() => setMode('shutdown')}
                className={`flex items-center gap-3 px-4 py-3 rounded-lg border cursor-pointer transition-colors ${
                  mode === 'shutdown'
                    ? 'border-vmm-accent/50 bg-vmm-accent/5'
                    : 'border-vmm-border bg-vmm-bg-alt hover:border-vmm-border-light'
                }`}
              >
                <Power size={16} className={mode === 'shutdown' ? 'text-vmm-accent' : 'text-vmm-text-muted'} />
                <div>
                  <div className="text-sm font-medium text-vmm-text">Shut down VMs</div>
                  <div className="text-xs text-vmm-text-muted">
                    All running VMs will be gracefully stopped. They remain assigned to this host and won't restart elsewhere.
                  </div>
                </div>
              </div>
            </div>
          </div>
        )}

        {/* Actions */}
        <div className="flex justify-end gap-3 pt-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button
            variant="primary"
            onClick={() => { onConfirm(mode); onClose() }}
          >
            <Wrench size={14} />
            {runningVms.length > 0
              ? mode === 'migrate'
                ? `Enter Maintenance & Migrate ${runningVms.length} VM${runningVms.length > 1 ? 's' : ''}`
                : `Enter Maintenance & Shut Down ${runningVms.length} VM${runningVms.length > 1 ? 's' : ''}`
              : 'Enter Maintenance'}
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
