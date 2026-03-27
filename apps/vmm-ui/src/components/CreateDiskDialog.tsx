import { useEffect, useState } from 'react'
import { Boxes, HardDrive } from 'lucide-react'
import api from '../api/client'
import type { StoragePool, CoreSanVolume } from '../api/types'
import { formatBytes } from '../utils/format'
import Dialog from './Dialog'
import FormField from './FormField'
import TextInput from './TextInput'
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

type StorageTarget =
  | { type: 'pool'; pool: StoragePool }
  | { type: 'san'; volume: CoreSanVolume }

export default function CreateDiskDialog({ open, onClose, vmName, vmId, onCreated, clusterId }: Props) {
  const [targets, setTargets] = useState<StorageTarget[]>([])
  const [selectedIdx, setSelectedIdx] = useState(0)
  const [sizeGb, setSizeGb] = useState(32)
  const [error, setError] = useState('')
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (!open) return
    const results: StorageTarget[] = []

    const fetchPools = async () => {
      try {
        const params = clusterId ? `?cluster_id=${encodeURIComponent(clusterId)}` : ''
        const { data } = await api.get<StoragePool[]>(`/api/storage/pools${params}`)
        for (const p of data.filter(p => p.total_bytes > 0)) {
          results.push({ type: 'pool', pool: p })
        }
      } catch { /* no pools available */ }
    }

    const fetchSanVolumes = async () => {
      try {
        const sanBase = clusterId ? '/api/san' : `${window.location.protocol}//${window.location.hostname}:7443/api`
        const resp = await (clusterId
          ? api.get<CoreSanVolume[]>(`${sanBase}/volumes`)
          : fetch(`${sanBase}/volumes`).then(r => r.json()))
        const vols: CoreSanVolume[] = Array.isArray(resp) ? resp : (resp as any).data || []
        for (const v of vols.filter(v => v.status === 'online')) {
          results.push({ type: 'san', volume: v })
        }
      } catch { /* SAN not available */ }
    }

    Promise.all([fetchPools(), fetchSanVolumes()]).then(() => {
      setTargets(results)
      if (results.length > 0 && selectedIdx >= results.length) setSelectedIdx(0)
    })
  }, [open, clusterId])

  const selected = targets[selectedIdx]
  const safeName = vmName.replace(/\s/g, '_').replace(/\//g, '_').toLowerCase()

  const previewPath = selected
    ? selected.type === 'pool'
      ? `${selected.pool.path}/${safeName}/disk.raw`
      : `/vmm/san/${selected.volume.name}/${safeName}/disk.raw`
    : '—'

  const freeBytes = selected
    ? selected.type === 'pool'
      ? selected.pool.free_bytes
      : selected.volume.free_bytes
    : 0

  const handleCreate = async () => {
    if (!selected) { setError('Select a storage target'); return }
    if (sizeGb < 1) { setError('Size must be at least 1 GB'); return }
    setSaving(true)
    setError('')

    try {
      if (selected.type === 'pool') {
        const { data } = await api.post('/api/storage/vm-disk', {
          vm_name: vmName, vm_id: vmId, size_gb: sizeGb, pool_id: selected.pool.id,
        })
        onCreated(data.path)
      } else {
        // Create disk on CoreSAN volume via FUSE path
        const sanBase = clusterId ? '/api/san' : `${window.location.protocol}//${window.location.hostname}:7443/api`
        const dirPath = `${safeName}`
        // Create directory first
        if (clusterId) {
          await api.post(`${sanBase}/volumes/${selected.volume.id}/mkdir`, { path: dirPath })
        } else {
          await fetch(`${sanBase}/volumes/${selected.volume.id}/mkdir`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path: dirPath }),
          })
        }
        // Create sparse raw disk image (just write a minimal header — the VM will expand it)
        const diskPath = `${dirPath}/disk.raw`
        const sparseHeader = new Uint8Array(512) // minimal empty disk
        if (clusterId) {
          await api.put(`${sanBase}/volumes/${selected.volume.id}/files/${encodeURIComponent(diskPath)}`, sparseHeader, {
            headers: { 'Content-Type': 'application/octet-stream' },
          })
        } else {
          await fetch(`${sanBase}/volumes/${selected.volume.id}/files/${encodeURIComponent(diskPath)}`, {
            method: 'PUT',
            headers: { 'Content-Type': 'application/octet-stream' },
            body: sparseHeader,
          })
        }
        onCreated(`/vmm/san/${selected.volume.name}/${diskPath}`)
      }
      onClose()
    } catch (e: any) {
      setError(e.response?.data?.error || e.message || 'Failed to create disk')
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} title="Create VM Disk" width="max-w-lg">
      <div className="space-y-4">
        <FormField label="Storage Target">
          <div className="space-y-1.5 max-h-48 overflow-y-auto">
            {targets.length === 0 && (
              <p className="text-sm text-vmm-text-muted py-3 text-center">No storage targets available</p>
            )}
            {targets.map((t, i) => {
              const isSan = t.type === 'san'
              const name = isSan ? t.volume.name : t.pool.name
              const free = isSan ? t.volume.free_bytes : t.pool.free_bytes
              const total = isSan ? t.volume.total_bytes : t.pool.total_bytes
              const label = isSan ? 'CoreSAN' : t.pool.pool_type
              return (
                <label key={i} className={`flex items-center gap-3 p-2.5 rounded-lg border cursor-pointer transition-colors
                  ${selectedIdx === i ? 'bg-vmm-accent/5 border-vmm-accent/30' : 'border-vmm-border hover:border-vmm-accent/20'}`}>
                  <input type="radio" name="storage-target" checked={selectedIdx === i}
                    onChange={() => setSelectedIdx(i)} className="accent-vmm-accent" />
                  {isSan
                    ? <Boxes size={14} className="text-vmm-accent flex-shrink-0" />
                    : <HardDrive size={14} className="text-vmm-text-muted flex-shrink-0" />}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-sm text-vmm-text font-medium truncate">{name}</span>
                      <span className="text-[10px] px-1.5 py-0.5 rounded bg-vmm-surface border border-vmm-border text-vmm-text-muted uppercase font-bold">
                        {label}
                      </span>
                    </div>
                    <span className="text-[10px] text-vmm-text-muted">
                      {formatBytes(free)} free of {formatBytes(total)}
                    </span>
                  </div>
                </label>
              )
            })}
          </div>
        </FormField>

        <FormField label="Disk Size (GB)">
          <TextInput type="number" min={1} max={2048} value={sizeGb}
            onChange={(e) => setSizeGb(parseInt(e.target.value) || 1)} />
          {freeBytes > 0 && sizeGb * 1024 * 1024 * 1024 > freeBytes && (
            <p className="text-[10px] text-vmm-warning mt-1">
              Warning: requested size exceeds available space ({formatBytes(freeBytes)} free)
            </p>
          )}
        </FormField>

        <div className="bg-vmm-bg-alt border border-vmm-border rounded-lg px-4 py-3">
          <div className="text-xs text-vmm-text-muted">Disk will be created at:</div>
          <div className="text-sm text-vmm-text font-mono mt-1">{previewPath}</div>
        </div>

        {error && <div className="text-xs text-vmm-danger">{error}</div>}

        <div className="flex justify-end gap-3 pt-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleCreate} disabled={saving || targets.length === 0}>
            {saving ? 'Creating...' : `Create ${sizeGb} GB Disk`}
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
