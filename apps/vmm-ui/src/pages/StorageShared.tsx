/** Shared Storage — NFS, GlusterFS, Ceph shared pools. */
import { useEffect, useState } from 'react'
import { Plus, Server, Share2 } from 'lucide-react'
import api from '../api/client'
import type { StoragePool } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import Button from '../components/Button'
import StoragePoolRow from '../components/StoragePoolRow'
import AddPoolDialog from '../components/AddPoolDialog'
import ConfirmDialog from '../components/ConfirmDialog'
import { formatBytes } from '../utils/format'

export default function StorageShared() {
  const [pools, setPools] = useState<StoragePool[]>([])
  const [addOpen, setAddOpen] = useState(false)
  const [deletePool, setDeletePool] = useState<StoragePool | null>(null)

  const refresh = () => {
    api.get<StoragePool[]>('/api/storage/pools').then(({ data }) =>
      setPools(data.filter(p => p.shared))
    )
  }
  useEffect(() => { refresh() }, [])

  const handleDelete = async () => {
    if (!deletePool) return
    try {
      await api.delete(`/api/storage/pools/${deletePool.id}`)
      setDeletePool(null)
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed to delete pool')
    }
  }

  const totalBytes = pools.reduce((s, p) => s + p.total_bytes, 0)
  const freeBytes = pools.reduce((s, p) => s + p.free_bytes, 0)

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Shared Storage</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            Network-attached storage pools (NFS, GlusterFS, Ceph) accessible across cluster nodes
          </p>
        </div>
        <Button variant="primary" icon={<Plus size={14} />} onClick={() => setAddOpen(true)}>
          Add Shared Pool
        </Button>
      </div>

      {/* Shared summary */}
      {pools.length > 0 && (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          <Card>
            <div className="flex items-center gap-3 mb-2">
              <Share2 size={18} className="text-vmm-accent" />
              <SectionLabel>Shared Pools</SectionLabel>
            </div>
            <div className="text-3xl font-bold text-vmm-text">{pools.length}</div>
            <div className="text-xs text-vmm-text-muted mt-1">
              {pools.filter(p => p.total_bytes > 0).length} online
            </div>
          </Card>
          <Card>
            <div className="flex items-center gap-3 mb-2">
              <Server size={18} className="text-vmm-accent" />
              <SectionLabel>Total Capacity</SectionLabel>
            </div>
            <div className="text-3xl font-bold text-vmm-text">{formatBytes(totalBytes)}</div>
            <div className="text-xs text-vmm-text-muted mt-1">{formatBytes(freeBytes)} available</div>
          </Card>
          <Card>
            <div className="flex items-center gap-3 mb-2">
              <SectionLabel>Supported Protocols</SectionLabel>
            </div>
            <div className="flex flex-wrap gap-2 mt-2">
              {['NFS', 'GlusterFS', 'Ceph RBD', 'iSCSI'].map((proto) => (
                <span key={proto} className="px-2 py-1 text-[10px] font-bold tracking-wider rounded bg-vmm-accent/10 text-vmm-accent border border-vmm-accent/20">
                  {proto}
                </span>
              ))}
            </div>
          </Card>
        </div>
      )}

      {/* Pool list */}
      {pools.length === 0 ? (
        <Card>
          <div className="flex flex-col items-center justify-center py-12 text-center">
            <div className="w-16 h-16 rounded-2xl bg-vmm-bg-alt flex items-center justify-center mb-4">
              <Share2 size={28} className="text-vmm-text-muted" />
            </div>
            <h3 className="text-lg font-semibold text-vmm-text mb-2">No Shared Storage</h3>
            <p className="text-sm text-vmm-text-muted max-w-md mb-4">
              Connect NFS exports, GlusterFS volumes, or Ceph pools to enable live migration
              and high availability across cluster nodes.
            </p>
            <Button variant="primary" icon={<Plus size={14} />} onClick={() => setAddOpen(true)}>
              Add Shared Pool
            </Button>
          </div>
        </Card>
      ) : (
        <div className="space-y-3">
          {pools.map((pool) => (
            <StoragePoolRow
              key={pool.id}
              pool={pool}
              onEdit={() => {}}
              onDelete={() => setDeletePool(pool)}
            />
          ))}
        </div>
      )}

      <AddPoolDialog open={addOpen} onClose={() => setAddOpen(false)} onCreated={refresh} />
      <ConfirmDialog
        open={!!deletePool}
        title="Remove Shared Pool"
        message={`Remove shared pool "${deletePool?.name}"? Remote data will NOT be deleted.`}
        confirmLabel="Remove Pool"
        danger
        onConfirm={handleDelete}
        onCancel={() => setDeletePool(null)}
      />
    </div>
  )
}
