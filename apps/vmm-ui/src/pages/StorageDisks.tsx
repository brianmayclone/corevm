/** Disk Management — create, delete, resize disk images. */
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Plus, HardDrive, Trash2, Link, Maximize2 } from 'lucide-react'
import api from '../api/client'
import type { DiskImage, StoragePool, CoreSanStatus } from '../api/types'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import Button from '../components/Button'
import ContextMenu from '../components/ContextMenu'
import ConfirmDialog from '../components/ConfirmDialog'
import { formatBytes } from '../utils/format'

export default function StorageDisks() {
  const navigate = useNavigate()
  const { backendMode } = useClusterStore()
  const isCluster = backendMode === 'cluster'
  const [images, setImages] = useState<DiskImage[]>([])
  const [pools, setPools] = useState<StoragePool[]>([])
  const [sanVolumes, setSanVolumes] = useState<{ id: string; name: string }[]>([])
  const [deleteImage, setDeleteImage] = useState<DiskImage | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [filter, setFilter] = useState<'all' | 'orphaned' | 'attached'>('all')

  // Create form
  const [createName, setCreateName] = useState('')
  const [createSize, setCreateSize] = useState('20')
  const [createPool, setCreatePool] = useState<number>(0)

  const refresh = () => {
    api.get<DiskImage[]>('/api/storage/images').then(({ data }) => setImages(data))
    api.get<StoragePool[]>('/api/storage/pools').then(({ data }) => setPools(data))
    // Fetch CoreSAN volumes for pool dropdown
    const sanUrl = isCluster ? '/api/san/status' : `${window.location.protocol}//${window.location.hostname}:7443/api/status`
    const sanHeaders: HeadersInit = isCluster ? { Authorization: `Bearer ${localStorage.getItem('vmm_token') || ''}` } : {}
    fetch(sanUrl, { headers: sanHeaders }).then(r => r.json()).then(d => {
      const status = Array.isArray(d) ? d[0] : d
      if (status?.volumes) {
        setSanVolumes(status.volumes.map((v: any) => ({ id: v.volume_id, name: v.volume_name })))
      }
    }).catch(() => {})
  }
  useEffect(() => { refresh() }, [])

  const filteredImages = filter === 'orphaned' ? images.filter(i => !i.vm_id)
    : filter === 'attached' ? images.filter(i => !!i.vm_id)
    : images

  const handleDelete = async () => {
    if (!deleteImage) return
    try {
      await api.delete(`/api/storage/images/${deleteImage.id}`)
      setDeleteImage(null)
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed to delete')
    }
  }

  const handleCreate = async () => {
    if (!createName.trim() || !createPool) return
    try {
      await api.post('/api/storage/images', { name: createName, size_gb: parseInt(createSize), pool_id: createPool })
      setShowCreate(false)
      setCreateName('')
      setCreateSize('20')
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed to create disk')
    }
  }

  const handleResize = async (img: DiskImage) => {
    const newSize = prompt(`New size in GB (current: ${Math.round(img.size_bytes / 1024 / 1024 / 1024)} GB):`)
    if (!newSize) return
    try {
      await api.post(`/api/storage/images/${img.id}/resize`, { new_size_gb: parseInt(newSize) })
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed to resize')
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Disk Management</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            Create, resize, and manage virtual disk images
          </p>
        </div>
        <Button variant="primary" icon={<Plus size={14} />} onClick={() => setShowCreate(true)}>
          Create Disk
        </Button>
      </div>

      {/* Create form */}
      {showCreate && (
        <Card>
          <SectionLabel className="mb-4">New Disk Image</SectionLabel>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Name</label>
              <input value={createName} onChange={(e) => setCreateName(e.target.value)}
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none"
                placeholder="e.g. data-disk" />
            </div>
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Size (GB)</label>
              <input value={createSize} onChange={(e) => setCreateSize(e.target.value)}
                type="number" min="1"
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none" />
            </div>
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Storage Pool</label>
              <select value={createPool} onChange={(e) => setCreatePool(parseInt(e.target.value))}
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none">
                <option value={0}>Select storage...</option>
                {pools.length > 0 && (
                  <optgroup label="Local Pools">
                    {pools.map(p => <option key={p.id} value={p.id}>{p.name} ({p.shared ? 'Shared' : 'Local'})</option>)}
                  </optgroup>
                )}
                {sanVolumes.length > 0 && (
                  <optgroup label="CoreSAN Volumes">
                    {sanVolumes.map(v => <option key={v.id} value={-1}>{v.name} (CoreSAN)</option>)}
                  </optgroup>
                )}
              </select>
            </div>
          </div>
          <div className="flex items-center justify-end gap-3 mt-4">
            <Button variant="ghost" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button variant="primary" onClick={handleCreate}>Create</Button>
          </div>
        </Card>
      )}

      {/* Filter tabs */}
      <div className="flex items-center gap-2">
        {(['all', 'attached', 'orphaned'] as const).map((f) => (
          <button key={f} onClick={() => setFilter(f)}
            className={`px-3 py-1.5 text-xs font-medium rounded-lg transition-colors cursor-pointer
              ${filter === f ? 'bg-vmm-accent text-white' : 'bg-vmm-surface text-vmm-text-muted hover:text-vmm-text'}`}>
            {f === 'all' ? `All (${images.length})` : f === 'attached' ? `Attached (${images.filter(i => !!i.vm_id).length})` : `Orphaned (${images.filter(i => !i.vm_id).length})`}
          </button>
        ))}
      </div>

      {/* Image table */}
      {filteredImages.length === 0 ? (
        <Card>
          <div className="flex flex-col items-center justify-center py-12 text-center">
            <HardDrive size={28} className="text-vmm-text-muted mb-3" />
            <p className="text-sm text-vmm-text-muted">
              {filter === 'orphaned' ? 'No orphaned disk images.' : 'No disk images found.'}
            </p>
          </div>
        </Card>
      ) : (
        <Card padding={false}>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-vmm-border text-[10px] text-vmm-text-muted uppercase tracking-wider">
                <th className="text-left px-5 py-3">Name</th>
                <th className="text-left px-5 py-3">Size</th>
                <th className="text-left px-5 py-3">Format</th>
                <th className="text-left px-5 py-3">Assigned VM</th>
                <th className="text-left px-5 py-3">Created</th>
                <th className="text-right px-5 py-3 w-12"></th>
              </tr>
            </thead>
            <tbody>
              {filteredImages.map((img) => (
                <tr key={img.id} className="border-b border-vmm-border last:border-b-0 hover:bg-vmm-surface-hover/50">
                  <td className="px-5 py-3 text-vmm-text font-medium flex items-center gap-2">
                    <HardDrive size={14} className="text-vmm-text-muted" />
                    {img.name}
                  </td>
                  <td className="px-5 py-3 text-vmm-text-dim">{formatBytes(img.size_bytes)}</td>
                  <td className="px-5 py-3 text-vmm-text-dim uppercase">{img.format}</td>
                  <td className="px-5 py-3">
                    {img.vm_name ? (
                      <span className="text-vmm-accent hover:underline cursor-pointer flex items-center gap-1"
                        onClick={() => navigate(`/vms/${img.vm_id}`)}>
                        <Link size={11} /> {img.vm_name}
                      </span>
                    ) : (
                      <span className="text-vmm-warning text-xs">Unattached</span>
                    )}
                  </td>
                  <td className="px-5 py-3 text-vmm-text-muted text-xs">{img.created_at}</td>
                  <td className="px-5 py-3 text-right">
                    <ContextMenu items={[
                      { label: 'Resize', icon: <Maximize2 size={14} />, onClick: () => handleResize(img) },
                      { label: 'Delete', icon: <Trash2 size={14} />, danger: true, onClick: () => setDeleteImage(img) },
                    ]} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}

      <ConfirmDialog
        open={!!deleteImage}
        title="Delete Disk Image"
        message={`Permanently delete "${deleteImage?.name}"? This will remove the file from disk.`}
        confirmLabel="Delete"
        danger
        onConfirm={handleDelete}
        onCancel={() => setDeleteImage(null)}
      />
    </div>
  )
}
