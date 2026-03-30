/** Machines list — all VMs with filtering by resource group. */
import { useEffect, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { Filter, Trash2 } from 'lucide-react'
import api from '../api/client'
import type { ResourceGroup } from '../api/types'
import { useVmStore } from '../stores/vmStore'
import VmPriorityCard from '../components/VmPriorityCard'
import Card from '../components/Card'
import Button from '../components/Button'
import { formatRam } from '../utils/format'

export default function MachinesList() {
  const { vms, fetchVms, startVm, stopVm, deleteVm } = useVmStore()
  const [deleteTarget, setDeleteTarget] = useState<{ id: string; name: string } | null>(null)
  const [deleting, setDeleting] = useState(false)
  const [deleteError, setDeleteError] = useState<string | null>(null)
  const [resourceGroups, setResourceGroups] = useState<ResourceGroup[]>([])
  const [searchParams, setSearchParams] = useSearchParams()
  const navigate = useNavigate()
  const filterRg = searchParams.get('rg') ? parseInt(searchParams.get('rg')!) : null

  useEffect(() => {
    fetchVms()
    api.get<ResourceGroup[]>('/api/resource-groups').then(({ data }) => setResourceGroups(data))
  }, [])

  const filteredVms = filterRg ? vms.filter(v => v.resource_group_id === filterRg) : vms
  const totalRamMb = 128 * 1024 // placeholder

  const handleDelete = async () => {
    if (!deleteTarget) return
    setDeleting(true)
    setDeleteError(null)
    try {
      await deleteVm(deleteTarget.id)
      setDeleteTarget(null)
    } catch (err: any) {
      setDeleteError(err?.response?.data?.error || err?.message || 'Failed to delete VM')
    } finally {
      setDeleting(false)
    }
  }

  return (
    <div className="space-y-5">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">All Machines</h1>
          <p className="text-sm text-vmm-text-muted mt-1">{filteredVms.length} virtual machines</p>
        </div>
      </div>

      {/* Resource group filter */}
      {resourceGroups.length > 1 && (
        <div className="flex items-center gap-2">
          <Filter size={14} className="text-vmm-text-muted" />
          <button
            onClick={() => setSearchParams({})}
            className={`px-3 py-1.5 text-xs font-medium rounded-lg transition-colors cursor-pointer
              ${!filterRg ? 'bg-vmm-accent text-white' : 'bg-vmm-surface text-vmm-text-muted hover:text-vmm-text'}`}
          >
            All ({vms.length})
          </button>
          {resourceGroups.map((rg) => (
            <button key={rg.id}
              onClick={() => setSearchParams({ rg: String(rg.id) })}
              className={`px-3 py-1.5 text-xs font-medium rounded-lg transition-colors cursor-pointer
                ${filterRg === rg.id ? 'bg-vmm-accent text-white' : 'bg-vmm-surface text-vmm-text-muted hover:text-vmm-text'}`}
            >
              {rg.name} ({rg.vm_count})
            </button>
          ))}
        </div>
      )}

      {filteredVms.length === 0 ? (
        <Card>
          <div className="text-vmm-text-muted text-sm py-8 text-center">
            {filterRg ? 'No VMs in this resource group.' : 'No virtual machines yet. Click "Create VM" to get started.'}
          </div>
        </Card>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
          {filteredVms.map((vm) => (
            <VmPriorityCard
              key={vm.id}
              name={vm.name}
              guestOs={vm.guest_os}
              state={vm.state}
              tag={`${vm.cpu_cores} vCPU • ${formatRam(vm.ram_mb)}`}
              cpuPercent={vm.state === 'running' ? Math.floor(Math.random() * 60 + 5) : 0}
              ramPercent={vm.state === 'running' ? Math.round((vm.ram_mb / totalRamMb) * 100) : 0}
              onClick={() => navigate(`/vms/${vm.id}`)}
              onConsole={() => navigate(`/vms/${vm.id}/console`)}
              onPower={() => vm.state === 'stopped' ? startVm(vm.id) : stopVm(vm.id)}
              onDelete={() => setDeleteTarget({ id: vm.id, name: vm.name })}
            />
          ))}
        </div>
      )}

      {/* ── Delete Confirmation Dialog ─────────────────────────────── */}
      {deleteTarget && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
          <div className="bg-vmm-surface border border-vmm-border rounded-xl p-6 w-full max-w-sm mx-4 space-y-4">
            <h3 className="text-lg font-semibold text-vmm-text">Delete Virtual Machine</h3>
            <p className="text-sm text-vmm-text-muted">
              Are you sure you want to permanently delete <span className="font-semibold text-vmm-text">{deleteTarget.name}</span>? This action cannot be undone and all associated disks will be removed.
            </p>
            {deleteError && (
              <div className="text-sm text-vmm-danger bg-vmm-danger/10 rounded-lg px-3 py-2">{deleteError}</div>
            )}
            <div className="flex justify-end gap-2 pt-2">
              <Button variant="outline" size="sm" onClick={() => { setDeleteTarget(null); setDeleteError(null) }} disabled={deleting}>
                Cancel
              </Button>
              <Button variant="danger" size="sm" icon={<Trash2 size={14} />} onClick={handleDelete} disabled={deleting}>
                {deleting ? 'Deleting...' : 'Delete VM'}
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
