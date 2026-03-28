import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { HardDrive, Plus, Settings, CheckCircle, AlertTriangle, Trash2, Link, Workflow, Boxes, ArrowRight } from 'lucide-react'
import api from '../api/client'
import type { StoragePool, StorageStats, DiskImage, Iso, Cluster, CoreSanStatus } from '../api/types'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import SpecRow from '../components/SpecRow'
import Button from '../components/Button'
import Select from '../components/Select'
import StoragePoolRow from '../components/StoragePoolRow'
import AddPoolDialog from '../components/AddPoolDialog'
import ConfirmDialog from '../components/ConfirmDialog'
import ContextMenu from '../components/ContextMenu'
import { formatBytes } from '../utils/format'

export default function Storage() {
  const navigate = useNavigate()
  const { backendMode } = useClusterStore()
  const isCluster = backendMode === 'cluster'
  const [pools, setPools] = useState<StoragePool[]>([])
  const [stats, setStats] = useState<StorageStats | null>(null)
  const [images, setImages] = useState<DiskImage[]>([])
  const [isos, setIsos] = useState<Iso[]>([])
  const [addPoolOpen, setAddPoolOpen] = useState(false)
  const [deletePool, setDeletePool] = useState<StoragePool | null>(null)
  const [deleteImage, setDeleteImage] = useState<DiskImage | null>(null)
  const [orphanedOpen, setOrphanedOpen] = useState(false)
  const [filter, setFilter] = useState<'all' | 'orphaned'>('all')
  const [sanStatus, setSanStatus] = useState<CoreSanStatus | null>(null)

  // Cluster-mode: cluster selector
  const [clusters, setClusters] = useState<Cluster[]>([])
  const [selectedClusterId, setSelectedClusterId] = useState('')

  useEffect(() => {
    if (isCluster) {
      api.get<Cluster[]>('/api/clusters').then(({ data }) => {
        setClusters(data)
        if (data.length > 0 && !selectedClusterId) setSelectedClusterId(data[0].id)
      })
    }
  }, [isCluster])

  const refresh = () => {
    const clusterParam = isCluster && selectedClusterId ? `?cluster_id=${encodeURIComponent(selectedClusterId)}` : ''
    api.get<StoragePool[]>(`/api/storage/pools${clusterParam}`)
      .then(({ data }) => { setPools(data) })
      .catch(() => setPools([]))
    api.get<StorageStats>('/api/storage/stats').then(({ data }) => setStats(data)).catch(() => {})
    api.get<DiskImage[]>('/api/storage/images').then(({ data }) => setImages(data)).catch(() => {})
    api.get<Iso[]>('/api/storage/isos').then(({ data }) => setIsos(data)).catch(() => {})
    // Fetch CoreSAN status — use cluster proxy in cluster mode, direct in standalone
    const sanUrl = isCluster ? '/api/san/status' : `${window.location.protocol}//${window.location.hostname}:7443/api/status`
    const sanHeaders: HeadersInit = isCluster ? { Authorization: `Bearer ${localStorage.getItem('vmm_token') || ''}` } : {}
    fetch(sanUrl, { headers: sanHeaders })
      .then(r => r.json())
      .then(d => {
        // Cluster mode returns array of statuses — pick first or merge
        if (Array.isArray(d) && d.length > 0) {
          const merged = d[0]
          // Aggregate peer count and volumes from all hosts
          if (d.length > 1) {
            merged.peer_count = Math.max(...d.map((s: any) => s.peer_count || 0))
            merged.claimed_disks = d.reduce((sum: number, s: any) => sum + (s.claimed_disks || 0), 0)
          }
          setSanStatus(merged)
        } else if (d && d.running !== undefined) {
          setSanStatus(d)
        } else {
          setSanStatus(null)
        }
      })
      .catch(() => setSanStatus(null))
  }
  // Refresh when cluster selection changes, or on mount in standalone mode
  useEffect(() => {
    // In cluster mode, wait until a cluster is selected before loading
    if (isCluster && !selectedClusterId && clusters.length === 0) return
    refresh()
  }, [selectedClusterId, isCluster])

  // CoreSAN capacity from volume summaries
  const sanTotalBytes = sanStatus?.volumes?.reduce((s: number, v: any) => s + (v.total_bytes || 0), 0) || 0
  const sanFreeBytes = sanStatus?.volumes?.reduce((s: number, v: any) => s + (v.free_bytes || 0), 0) || 0
  const sanUsedBytes = sanTotalBytes - sanFreeBytes

  // Aggregate: local pools + CoreSAN
  const poolUsedBytes = stats?.used_bytes || 0
  const poolTotalBytes = stats?.total_bytes || 0
  const poolFreeBytes = stats?.free_bytes || 0
  const usedBytes = poolUsedBytes + sanUsedBytes
  const totalBytes = (poolTotalBytes + sanTotalBytes) || 1
  const freeBytes = poolFreeBytes + sanFreeBytes
  const vmDiskBytes = stats?.vm_disk_bytes || 0
  const usagePercent = Math.round((usedBytes / totalBytes) * 100)
  const orphanedCount = stats?.orphaned_images || 0

  const filteredImages = filter === 'orphaned' ? images.filter((i) => !i.vm_id) : images

  const handleDeletePool = async () => {
    if (!deletePool) return
    try {
      await api.delete(`/api/storage/pools/${deletePool.id}`)
      setDeletePool(null)
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed to delete pool')
    }
  }

  const handleDeleteImage = async () => {
    if (!deleteImage) return
    try {
      await api.delete(`/api/storage/images/${deleteImage.id}`)
      setDeleteImage(null)
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed to delete image')
    }
  }

  return (
    <div className="space-y-6">
      {/* ── Header ────────────────────────────────────────────────── */}
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Storage Management</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            {isCluster ? 'Cluster-wide datastores and disk images' : 'Manage storage pools, disk images and ISO files'}
          </p>
        </div>
        <div className="flex items-center gap-3">
          {isCluster && (
            <Button variant="outline" icon={<HardDrive size={14} />} onClick={() => navigate('/storage/wizard')}>
              Create Cluster Storage
            </Button>
          )}
          <Button variant="primary" icon={<Plus size={14} />} onClick={() => setAddPoolOpen(true)}>
            {isCluster ? 'Add Datastore' : 'Add Storage Pool'}
          </Button>
        </div>
      </div>

      {/* Cluster selector — only in cluster mode */}
      {isCluster && clusters.length > 0 && (
        <div className="flex items-center gap-3 bg-vmm-surface border border-vmm-border rounded-xl px-4 py-3">
          <Workflow size={16} className="text-vmm-accent flex-shrink-0" />
          <span className="text-sm text-vmm-text-muted flex-shrink-0">Cluster:</span>
          <select
            value={selectedClusterId}
            onChange={(e) => setSelectedClusterId(e.target.value)}
            className="bg-vmm-bg border border-vmm-border rounded-lg px-3 py-1.5 text-sm text-vmm-text flex-1 max-w-xs"
          >
            {clusters.map(c => (
              <option key={c.id} value={c.id}>{c.name} ({c.host_count} hosts)</option>
            ))}
          </select>
          <span className="text-xs text-vmm-text-muted">
            Showing only datastores accessible by all hosts in this cluster
          </span>
        </div>
      )}

      {/* ── Aggregate Capacity + Health ────────────────────────────── */}
      <div className="grid grid-cols-1 lg:grid-cols-[1fr_300px] gap-5">
        <Card>
          <SectionLabel className="mb-4">Aggregate Capacity</SectionLabel>
          <div className="flex items-baseline gap-2 mb-4">
            <span className="text-4xl font-bold text-vmm-text">{formatBytes(usedBytes)}</span>
            <span className="text-lg text-vmm-text-muted">Used</span>
            <span className="text-lg text-vmm-text-muted mx-1">/</span>
            <span className="text-lg text-vmm-text-muted">{formatBytes(totalBytes)} Total</span>
          </div>
          <div className="w-full h-4 bg-vmm-border rounded-full overflow-hidden flex mb-3">
            <div className="bg-vmm-accent-dim h-full" style={{ width: `${Math.round((vmDiskBytes / totalBytes) * 100)}%` }} />
            <div className="bg-vmm-accent h-full" style={{ width: `${Math.max(0, Math.round((poolUsedBytes / totalBytes) * 100) - Math.round((vmDiskBytes / totalBytes) * 100))}%` }} />
            {sanUsedBytes > 0 && (
              <div className="h-full" style={{ width: `${Math.round((sanUsedBytes / totalBytes) * 100)}%`, background: '#8b5cf6' }} />
            )}
          </div>
          <div className="flex items-center gap-5 text-xs text-vmm-text-muted flex-wrap">
            <span className="flex items-center gap-1.5">
              <span className="w-2.5 h-2.5 rounded-full bg-vmm-accent-dim" /> VM Disks ({formatBytes(vmDiskBytes)})
            </span>
            {poolUsedBytes > 0 && (
              <span className="flex items-center gap-1.5">
                <span className="w-2.5 h-2.5 rounded-full bg-vmm-accent" /> Local Pools ({formatBytes(poolUsedBytes)})
              </span>
            )}
            {sanUsedBytes > 0 && (
              <span className="flex items-center gap-1.5">
                <span className="w-2.5 h-2.5 rounded-full" style={{ background: '#8b5cf6' }} /> CoreSAN ({formatBytes(sanUsedBytes)})
              </span>
            )}
            <span className="flex items-center gap-1.5">
              <span className="w-2.5 h-2.5 rounded-full bg-vmm-border" /> Available ({formatBytes(freeBytes)})
            </span>
          </div>
        </Card>

        <Card>
          <SectionLabel className="mb-4">Health Status</SectionLabel>
          <SpecRow icon={<CheckCircle size={16} className="text-vmm-success" />}
            label="Storage Pools" value={`${stats?.online_pools || 0} / ${stats?.total_pools || 0}`} />
          <SpecRow icon={<CheckCircle size={16} className="text-vmm-success" />}
            label="Disk Images" value={`${stats?.total_images || 0} Active`} />
          {sanStatus?.running && (
            <>
              <SpecRow
                icon={<Boxes size={16} className={
                  sanStatus.quorum_status === 'active' || sanStatus.quorum_status === 'solo' ? 'text-vmm-success' :
                  sanStatus.quorum_status === 'degraded' ? 'text-vmm-warning' : 'text-vmm-danger'
                } />}
                label="CoreSAN"
                value={`${sanStatus.quorum_status} — ${sanStatus.volumes?.length || 0} vol${(sanStatus.volumes?.length || 0) !== 1 ? 's' : ''}, ${(sanStatus.peer_count || 0) + 1} nodes`}
              />
              {sanTotalBytes > 0 && (
                <SpecRow
                  icon={<HardDrive size={16} className="text-vmm-accent" />}
                  label="SAN Capacity"
                  value={`${formatBytes(sanUsedBytes)} / ${formatBytes(sanTotalBytes)} (${Math.round(sanUsedBytes / sanTotalBytes * 100)}%)`}
                />
              )}
            </>
          )}
          <div
            className={`mt-1 cursor-pointer ${orphanedCount > 0 ? 'hover:bg-vmm-surface-hover/30' : ''} rounded-lg transition-colors`}
            onClick={() => { if (orphanedCount > 0) setFilter(filter === 'orphaned' ? 'all' : 'orphaned') }}
          >
            <SpecRow
              icon={<AlertTriangle size={16} className={orphanedCount > 0 ? 'text-vmm-warning' : 'text-vmm-success'} />}
              label="Orphaned Images"
              value={orphanedCount > 0 ? `${orphanedCount} Detected — Click to view` : 'None'}
            />
          </div>
        </Card>
      </div>

      {/* ── CoreSAN Status ────────────────────────────────────────── */}
      {sanStatus && sanStatus.running && (
        <Card>
          <div className="flex items-center justify-between mb-3">
            <div className="flex items-center gap-2.5">
              <div className="w-8 h-8 rounded-lg bg-vmm-accent/10 flex items-center justify-center">
                <Boxes size={16} className="text-vmm-accent" />
              </div>
              <div>
                <div className="flex items-center gap-2">
                  <h2 className="text-sm font-bold text-vmm-text">CoreSAN</h2>
                  <span className={`px-1.5 py-0.5 rounded text-[9px] font-bold border tracking-wider uppercase ${
                    sanStatus.quorum_status === 'active' || sanStatus.quorum_status === 'solo' ? 'bg-vmm-success/20 text-vmm-success border-vmm-success/30' :
                    sanStatus.quorum_status === 'degraded' ? 'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30' :
                    sanStatus.quorum_status === 'fenced' || sanStatus.quorum_status === 'sanitizing' ? 'bg-vmm-danger/20 text-vmm-danger border-vmm-danger/30' :
                    'bg-vmm-surface text-vmm-text-muted border-vmm-border'
                  }`}>{sanStatus.quorum_status || 'unknown'}</span>
                </div>
                <p className="text-[10px] text-vmm-text-muted">
                  {sanStatus.volumes.length} volume{sanStatus.volumes.length !== 1 ? 's' : ''} &middot; {sanStatus.peer_count + 1} node{sanStatus.peer_count !== 0 ? 's' : ''} &middot; {sanStatus.hostname}{sanStatus.is_leader ? ' (leader)' : ''}
                </p>
              </div>
            </div>
            <button onClick={() => navigate('/storage/coresan')}
              className="flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium text-vmm-accent hover:text-vmm-accent-hover transition-colors cursor-pointer">
              Manage <ArrowRight size={12} />
            </button>
          </div>
          {sanStatus.volumes.length > 0 && (
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2">
              {sanStatus.volumes.map(vol => {
                const used = vol.total_bytes - vol.free_bytes
                const pct = vol.total_bytes > 0 ? Math.round((used / vol.total_bytes) * 100) : 0
                return (
                  <div key={vol.volume_id} className="flex items-center gap-3 p-3 rounded-lg bg-vmm-bg/50 border border-vmm-border">
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 mb-1">
                        <span className="text-xs font-medium text-vmm-text truncate">{vol.volume_name}</span>
                        <span className={`px-1.5 py-0.5 rounded text-[9px] font-bold border tracking-wider uppercase ${
                          vol.status === 'online' ? 'bg-vmm-success/20 text-vmm-success border-vmm-success/30' :
                          vol.status === 'degraded' ? 'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30' :
                          'bg-vmm-danger/20 text-vmm-danger border-vmm-danger/30'
                        }`}>{vol.status}</span>
                        <span className="px-1.5 py-0.5 rounded text-[9px] font-bold border tracking-wider uppercase bg-vmm-surface text-vmm-text-muted border-vmm-border">
                          FTT={vol.ftt}
                        </span>
                      </div>
                      <div className="w-full h-1 bg-vmm-border rounded-full overflow-hidden">
                        <div className={`h-full rounded-full ${pct > 80 ? 'bg-vmm-danger' : 'bg-vmm-accent'}`}
                          style={{ width: `${pct}%` }} />
                      </div>
                      <div className="flex justify-between mt-1 text-[10px] text-vmm-text-muted">
                        <span>{formatBytes(used)} / {formatBytes(vol.total_bytes)}</span>
                        <span>{vol.stale_chunks > 0 ? `${vol.stale_chunks} syncing` : `${vol.synced_chunks} synced`}</span>
                      </div>
                    </div>
                  </div>
                )
              })}
            </div>
          )}
        </Card>
      )}

      {/* ── Storage Pools ─────────────────────────────────────────── */}
      <div>
        <h2 className="text-lg font-bold text-vmm-text mb-3">Storage Pools</h2>
        {pools.length === 0 ? (
          <Card>
            <div className="text-vmm-text-muted text-sm py-8 text-center">
              No storage pools configured. Click "Add Storage Pool" to get started.
            </div>
          </Card>
        ) : (
          <div className="space-y-3">
            {pools.map((pool) => (
              <StoragePoolRow
                key={pool.id}
                pool={pool}
                onEdit={() => {
                  const name = prompt('Pool name:', pool.name)
                  if (name === null) return
                  api.put(`/api/storage/pools/${pool.id}`, { name })
                    .then(refresh)
                    .catch(e => alert(e.response?.data?.error || 'Failed to update'))
                }}
                onDelete={() => setDeletePool(pool)}
              />
            ))}
          </div>
        )}
      </div>

      {/* ── Disk Images ───────────────────────────────────────────── */}
      <div>
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-3">
            <h2 className="text-lg font-bold text-vmm-text">Disk Images</h2>
            {filter === 'orphaned' && (
              <span className="px-2 py-0.5 text-[10px] font-bold tracking-wider rounded bg-vmm-warning/20 text-vmm-warning border border-vmm-warning/30">
                SHOWING ORPHANED ONLY
              </span>
            )}
          </div>
          {filter === 'orphaned' && (
            <Button variant="ghost" size="sm" onClick={() => setFilter('all')}>Show All</Button>
          )}
        </div>
        {filteredImages.length === 0 ? (
          <Card>
            <div className="text-vmm-text-muted text-sm py-6 text-center">
              {filter === 'orphaned' ? 'No orphaned images found.' : 'No disk images yet.'}
            </div>
          </Card>
        ) : (
          <Card padding={false}>
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-vmm-border text-xs text-vmm-text-muted uppercase tracking-wider">
                  <th className="text-left px-5 py-3">Name</th>
                  <th className="text-left px-5 py-3">Size</th>
                  <th className="text-left px-5 py-3">Format</th>
                  <th className="text-left px-5 py-3">VM</th>
                  <th className="text-right px-5 py-3 w-12"></th>
                </tr>
              </thead>
              <tbody>
                {filteredImages.map((img) => (
                  <tr key={img.id} className="border-b border-vmm-border last:border-b-0 hover:bg-vmm-surface-hover/50">
                    <td className="px-5 py-3 text-vmm-text font-medium">{img.name}</td>
                    <td className="px-5 py-3 text-vmm-text-dim">{formatBytes(img.size_bytes)}</td>
                    <td className="px-5 py-3 text-vmm-text-dim uppercase">{img.format}</td>
                    <td className="px-5 py-3">
                      {img.vm_name ? (
                        <span
                          className="text-vmm-accent hover:underline cursor-pointer flex items-center gap-1"
                          onClick={() => navigate(`/vms/${img.vm_id}`)}
                        >
                          <Link size={11} /> {img.vm_name}
                        </span>
                      ) : (
                        <span className="text-vmm-warning text-xs">Orphaned</span>
                      )}
                    </td>
                    <td className="px-5 py-3 text-right">
                      <ContextMenu items={[
                        { label: 'Delete Image', icon: <Trash2 size={14} />, danger: true, onClick: () => setDeleteImage(img) },
                      ]} />
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </Card>
        )}
      </div>

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
      <AddPoolDialog open={addPoolOpen} onClose={() => setAddPoolOpen(false)} onCreated={refresh}
        clusterId={isCluster ? selectedClusterId : undefined} />

      <ConfirmDialog
        open={!!deletePool}
        title="Delete Storage Pool"
        message={`Are you sure you want to delete the pool "${deletePool?.name}"? This will NOT delete the files on disk, only the pool configuration.`}
        confirmLabel="Delete Pool"
        danger
        onConfirm={handleDeletePool}
        onCancel={() => setDeletePool(null)}
      />

      <ConfirmDialog
        open={!!deleteImage}
        title="Delete Disk Image"
        message={`Are you sure you want to delete "${deleteImage?.name}"? This will permanently remove the disk image file from disk.`}
        confirmLabel="Delete Image"
        danger
        onConfirm={handleDeleteImage}
        onCancel={() => setDeleteImage(null)}
      />
    </div>
  )
}
