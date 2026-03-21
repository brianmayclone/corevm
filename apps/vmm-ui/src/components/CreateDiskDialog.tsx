import { useEffect, useState } from 'react'
import api from '../api/client'
import type { StoragePool } from '../api/types'
import Dialog from './Dialog'
import FormField from './FormField'
import TextInput from './TextInput'
import Select from './Select'
import Button from './Button'

interface Props {
  open: boolean
  onClose: () => void
  vmName: string
  vmId: string
  /** Called with the created disk path */
  onCreated: (path: string) => void
  /** In cluster mode: only show pools accessible by all hosts in this cluster */
  clusterId?: string
}

export default function CreateDiskDialog({ open, onClose, vmName, vmId, onCreated, clusterId }: Props) {
  const [pools, setPools] = useState<StoragePool[]>([])
  const [poolId, setPoolId] = useState<number | null>(null)
  const [sizeGb, setSizeGb] = useState(32)
  const [error, setError] = useState('')
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (open) {
      const params = clusterId ? `?cluster_id=${encodeURIComponent(clusterId)}` : ''
      api.get<StoragePool[]>(`/api/storage/pools${params}`).then(({ data }) => {
        setPools(data.filter((p) => p.total_bytes > 0)) // only online pools
        if (data.length > 0 && !poolId) setPoolId(data[0].id)
      })
    }
  }, [open, clusterId])

  const selectedPool = pools.find((p) => p.id === poolId)
  const safeName = vmName.replace(/\s/g, '_').replace(/\//g, '_').toLowerCase()

  const handleCreate = async () => {
    if (!poolId) { setError('Select a storage pool'); return }
    if (sizeGb < 1) { setError('Size must be at least 1 GB'); return }
    setSaving(true)
    setError('')
    try {
      const { data } = await api.post('/api/storage/vm-disk', {
        vm_name: vmName, vm_id: vmId, size_gb: sizeGb, pool_id: poolId,
      })
      onCreated(data.path)
      onClose()
    } catch (e: any) {
      setError(e.response?.data?.error || 'Failed to create disk')
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} title="Create VM Disk">
      <div className="space-y-4">
        <FormField label="Storage Pool">
          <Select
            options={pools.map((p) => ({ value: String(p.id), label: `${p.name} (${p.pool_type})` }))}
            value={poolId ? String(poolId) : ''}
            onChange={(e) => setPoolId(parseInt(e.target.value))}
          />
        </FormField>
        <FormField label="Disk Size (GB)">
          <TextInput type="number" min={1} max={2048} value={sizeGb}
            onChange={(e) => setSizeGb(parseInt(e.target.value) || 1)} />
        </FormField>
        <div className="bg-vmm-bg-alt border border-vmm-border rounded-lg px-4 py-3">
          <div className="text-xs text-vmm-text-muted">Disk will be created at:</div>
          <div className="text-sm text-vmm-text font-mono mt-1">
            {selectedPool ? `${selectedPool.path}/${safeName}/disk.raw` : '—'}
          </div>
        </div>
        {error && <div className="text-xs text-vmm-danger">{error}</div>}
        <div className="flex justify-end gap-3 pt-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleCreate} disabled={saving}>
            {saving ? 'Creating...' : `Create ${sizeGb} GB Disk`}
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
