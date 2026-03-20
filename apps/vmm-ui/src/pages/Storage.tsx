import { useEffect, useState } from 'react'
import { HardDrive, Plus, Settings, CheckCircle, AlertTriangle } from 'lucide-react'
import api from '../api/client'
import type { StoragePool, StorageStats, DiskImage, Iso } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import SpecRow from '../components/SpecRow'
import Button from '../components/Button'
import StoragePoolRow from '../components/StoragePoolRow'
import AddPoolDialog from '../components/AddPoolDialog'
import { formatBytes } from '../utils/format'

export default function Storage() {
  const [pools, setPools] = useState<StoragePool[]>([])
  const [stats, setStats] = useState<StorageStats | null>(null)
  const [images, setImages] = useState<DiskImage[]>([])
  const [isos, setIsos] = useState<Iso[]>([])
  const [addPoolOpen, setAddPoolOpen] = useState(false)

  const refresh = () => {
    api.get<StoragePool[]>('/api/storage/pools').then(({ data }) => setPools(data))
    api.get<StorageStats>('/api/storage/stats').then(({ data }) => setStats(data))
    api.get<DiskImage[]>('/api/storage/images').then(({ data }) => setImages(data))
    api.get<Iso[]>('/api/storage/isos').then(({ data }) => setIsos(data))
  }
  useEffect(() => { refresh() }, [])

  const usedBytes = stats?.used_bytes || 0
  const totalBytes = stats?.total_bytes || 1
  const freeBytes = stats?.free_bytes || 0
  const vmDiskBytes = stats?.vm_disk_bytes || 0
  const usagePercent = Math.round((usedBytes / totalBytes) * 100)

  return (
    <div className="space-y-6">
      {/* ── Header ────────────────────────────────────────────────── */}
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Local Storage</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            Directly attached physical and logical storage pools
          </p>
        </div>
        <div className="flex items-center gap-3">
          <Button variant="outline" icon={<Settings size={14} />}>Disk Management</Button>
          <Button variant="primary" icon={<Plus size={14} />} onClick={() => setAddPoolOpen(true)}>Add Storage Pool</Button>
        </div>
      </div>

      {/* ── Aggregate Capacity + Health ────────────────────────────── */}
      <div className="grid grid-cols-[1fr_300px] gap-5">
        <Card>
          <SectionLabel className="mb-4">Aggregate Capacity</SectionLabel>
          <div className="flex items-baseline gap-2 mb-4">
            <span className="text-4xl font-bold text-vmm-text">{formatBytes(usedBytes)}</span>
            <span className="text-lg text-vmm-text-muted">Used</span>
            <span className="text-lg text-vmm-text-muted mx-1">/</span>
            <span className="text-lg text-vmm-text-muted">{formatBytes(totalBytes)} Total</span>
          </div>
          {/* Segmented bar */}
          <div className="w-full h-4 bg-vmm-border rounded-full overflow-hidden flex mb-3">
            <div className="bg-vmm-accent-dim h-full" style={{ width: `${Math.round((vmDiskBytes / totalBytes) * 100)}%` }} />
            <div className="bg-vmm-accent h-full" style={{ width: `${Math.max(0, usagePercent - Math.round((vmDiskBytes / totalBytes) * 100))}%` }} />
          </div>
          <div className="flex items-center gap-6 text-xs text-vmm-text-muted">
            <span className="flex items-center gap-1.5">
              <span className="w-2.5 h-2.5 rounded-full bg-vmm-accent-dim" /> VM Disks ({formatBytes(vmDiskBytes)})
            </span>
            <span className="flex items-center gap-1.5">
              <span className="w-2.5 h-2.5 rounded-full bg-vmm-border" /> Available ({formatBytes(freeBytes)})
            </span>
          </div>
        </Card>

        <Card>
          <SectionLabel className="mb-4">Healthy States</SectionLabel>
          <SpecRow icon={<CheckCircle size={16} className="text-vmm-success" />}
            label="Storage Nodes" value={`${stats?.online_pools || 0} / ${stats?.total_pools || 0}`} />
          <SpecRow icon={<CheckCircle size={16} className="text-vmm-success" />}
            label="Disk Images" value={`${stats?.total_images || 0} Active`} />
          <SpecRow icon={<AlertTriangle size={16} className="text-vmm-warning" />}
            label="Orphaned Images" value={`${stats?.orphaned_images || 0} Detected`} />
        </Card>
      </div>

      {/* ── Storage Pools ─────────────────────────────────────────── */}
      <div>
        <div className="flex items-center justify-between mb-3">
          <h2 className="text-lg font-bold text-vmm-text">Storage Pools</h2>
        </div>
        {pools.length === 0 ? (
          <Card>
            <div className="text-vmm-text-muted text-sm py-8 text-center">
              No storage pools configured. Click "Add Storage Pool" to get started.
            </div>
          </Card>
        ) : (
          <div className="space-y-3">
            {pools.map((pool) => (
              <StoragePoolRow key={pool.id} pool={pool} />
            ))}
          </div>
        )}
      </div>

      {/* ── Disk Images ───────────────────────────────────────────── */}
      {images.length > 0 && (
        <div>
          <h2 className="text-lg font-bold text-vmm-text mb-3">Disk Images</h2>
          <Card padding={false}>
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-vmm-border text-xs text-vmm-text-muted uppercase tracking-wider">
                  <th className="text-left px-5 py-3">Name</th>
                  <th className="text-left px-5 py-3">Size</th>
                  <th className="text-left px-5 py-3">Format</th>
                  <th className="text-left px-5 py-3">VM</th>
                </tr>
              </thead>
              <tbody>
                {images.map((img) => (
                  <tr key={img.id} className="border-b border-vmm-border last:border-b-0 hover:bg-vmm-surface-hover/50">
                    <td className="px-5 py-3 text-vmm-text font-medium">{img.name}</td>
                    <td className="px-5 py-3 text-vmm-text-dim">{formatBytes(img.size_bytes)}</td>
                    <td className="px-5 py-3 text-vmm-text-dim uppercase">{img.format}</td>
                    <td className="px-5 py-3 text-vmm-text-muted">{img.vm_id || '—'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Card>
        </div>
      )}

      {/* ── ISOs ──────────────────────────────────────────────────── */}
      {isos.length > 0 && (
        <div>
          <h2 className="text-lg font-bold text-vmm-text mb-3">ISO Library</h2>
          <Card padding={false}>
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-vmm-border text-xs text-vmm-text-muted uppercase tracking-wider">
                  <th className="text-left px-5 py-3">Name</th>
                  <th className="text-left px-5 py-3">Size</th>
                  <th className="text-left px-5 py-3">Uploaded</th>
                </tr>
              </thead>
              <tbody>
                {isos.map((iso) => (
                  <tr key={iso.id} className="border-b border-vmm-border last:border-b-0 hover:bg-vmm-surface-hover/50">
                    <td className="px-5 py-3 text-vmm-text font-medium">{iso.name}</td>
                    <td className="px-5 py-3 text-vmm-text-dim">{formatBytes(iso.size_bytes)}</td>
                    <td className="px-5 py-3 text-vmm-text-muted">{iso.uploaded_at}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Card>
        </div>
      )}

      {/* Dialogs */}
      <AddPoolDialog open={addPoolOpen} onClose={() => setAddPoolOpen(false)} onCreated={refresh} />
    </div>
  )
}
