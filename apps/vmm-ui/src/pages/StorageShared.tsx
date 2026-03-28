/** Shared Storage — NFS, GlusterFS, Ceph, and CoreSAN shared pools. */
import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Plus, Server, Share2, Boxes } from 'lucide-react'
import api from '../api/client'
import type { StoragePool, CoreSanStatus } from '../api/types'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import Button from '../components/Button'
import StoragePoolRow from '../components/StoragePoolRow'
import AddPoolDialog from '../components/AddPoolDialog'
import ConfirmDialog from '../components/ConfirmDialog'
import { formatBytes } from '../utils/format'

export default function StorageShared() {
  const navigate = useNavigate()
  const { backendMode } = useClusterStore()
  const isCluster = backendMode === 'cluster'
  const [pools, setPools] = useState<StoragePool[]>([])
  const [sanStatus, setSanStatus] = useState<CoreSanStatus | null>(null)
  const [addOpen, setAddOpen] = useState(false)
  const [deletePool, setDeletePool] = useState<StoragePool | null>(null)

  const refresh = () => {
    api.get<StoragePool[]>('/api/storage/pools').then(({ data }) =>
      setPools(data.filter(p => p.shared))
    )
    // Fetch CoreSAN status
    const sanUrl = isCluster ? '/api/san/status' : `${window.location.protocol}//${window.location.hostname}:7443/api/status`
    const sanHeaders: HeadersInit = isCluster ? { Authorization: `Bearer ${localStorage.getItem('vmm_token') || ''}` } : {}
    fetch(sanUrl, { headers: sanHeaders }).then(r => r.json()).then(d => {
      const status = Array.isArray(d) ? d[0] : d
      setSanStatus(status?.running ? status : null)
    }).catch(() => setSanStatus(null))
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

  const sanTotalBytes = sanStatus?.volumes?.reduce((s: number, v: any) => s + (v.total_bytes || 0), 0) || 0
  const sanFreeBytes = sanStatus?.volumes?.reduce((s: number, v: any) => s + (v.free_bytes || 0), 0) || 0
  const poolTotalBytes = pools.reduce((s, p) => s + p.total_bytes, 0)
  const poolFreeBytes = pools.reduce((s, p) => s + p.free_bytes, 0)
  const totalBytes = poolTotalBytes + sanTotalBytes
  const freeBytes = poolFreeBytes + sanFreeBytes
  const sharedCount = pools.length + (sanStatus?.volumes?.length || 0)

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Shared Storage</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            Shared storage accessible across cluster nodes — NFS, GlusterFS, Ceph, and CoreSAN
          </p>
        </div>
        <Button variant="primary" icon={<Plus size={14} />} onClick={() => setAddOpen(true)}>
          Add Shared Pool
        </Button>
      </div>

      {/* Shared summary */}
      {(pools.length > 0 || sanStatus) && (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
          <Card>
            <div className="flex items-center gap-3 mb-2">
              <Share2 size={18} className="text-vmm-accent" />
              <SectionLabel>Shared Pools</SectionLabel>
            </div>
            <div className="text-3xl font-bold text-vmm-text">{sharedCount}</div>
            <div className="text-xs text-vmm-text-muted mt-1">
              {pools.filter(p => p.total_bytes > 0).length + (sanStatus?.volumes?.length || 0)} online
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
              {['NFS', 'GlusterFS', 'Ceph RBD', 'iSCSI', 'CoreSAN'].map((proto) => (
                <span key={proto} className="px-2 py-1 text-[10px] font-bold tracking-wider rounded bg-vmm-accent/10 text-vmm-accent border border-vmm-accent/20">
                  {proto}
                </span>
              ))}
            </div>
          </Card>
        </div>
      )}

      {/* Pool list */}
      {pools.length === 0 && !sanStatus ? (
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

      {/* CoreSAN Volumes */}
      {sanStatus && sanStatus.volumes && sanStatus.volumes.length > 0 && (
        <div>
          <div className="flex items-center justify-between mb-3">
            <div className="flex items-center gap-2">
              <Boxes size={18} className="text-vmm-accent" />
              <h2 className="text-lg font-bold text-vmm-text">CoreSAN Volumes</h2>
              <span className={`px-1.5 py-0.5 rounded text-[9px] font-bold border tracking-wider uppercase ${
                sanStatus.quorum_status === 'active' || sanStatus.quorum_status === 'solo'
                  ? 'bg-vmm-success/20 text-vmm-success border-vmm-success/30'
                  : sanStatus.quorum_status === 'degraded'
                  ? 'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30'
                  : 'bg-vmm-danger/20 text-vmm-danger border-vmm-danger/30'
              }`}>{sanStatus.quorum_status}</span>
            </div>
            <button onClick={() => navigate('/storage/coresan')}
              className="text-xs font-medium text-vmm-accent hover:text-vmm-accent-hover transition-colors cursor-pointer">
              Manage CoreSAN &rarr;
            </button>
          </div>
          <div className="space-y-3">
            {sanStatus.volumes.map((vol: any) => {
              const used = vol.total_bytes - vol.free_bytes
              const pct = vol.total_bytes > 0 ? Math.round((used / vol.total_bytes) * 100) : 0
              return (
                <Card key={vol.volume_id}>
                  <div className="flex items-center justify-between">
                    <div className="flex items-center gap-3">
                      <div className="w-10 h-10 rounded-lg bg-vmm-accent/10 flex items-center justify-center">
                        <Boxes size={18} className="text-vmm-accent" />
                      </div>
                      <div>
                        <div className="flex items-center gap-2">
                          <span className="text-sm font-medium text-vmm-text">{vol.volume_name}</span>
                          <span className={`px-1.5 py-0.5 rounded text-[9px] font-bold border tracking-wider uppercase ${
                            vol.status === 'online' ? 'bg-vmm-success/20 text-vmm-success border-vmm-success/30' :
                            'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30'
                          }`}>{vol.status}</span>
                          <span className="px-1.5 py-0.5 rounded text-[9px] font-bold border tracking-wider uppercase bg-vmm-surface text-vmm-text-muted border-vmm-border">
                            FTT={vol.ftt} &middot; {vol.local_raid}
                          </span>
                        </div>
                        <div className="text-xs text-vmm-text-muted mt-0.5">
                          {formatBytes(used)} / {formatBytes(vol.total_bytes)} ({pct}%)
                          &middot; {vol.synced_chunks || 0} chunks synced
                        </div>
                      </div>
                    </div>
                    <div className="w-32">
                      <div className="w-full h-2 bg-vmm-border rounded-full overflow-hidden">
                        <div className={`h-full rounded-full ${pct > 80 ? 'bg-vmm-danger' : 'bg-vmm-accent'}`}
                          style={{ width: `${pct}%` }} />
                      </div>
                    </div>
                  </div>
                </Card>
              )
            })}
          </div>
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
