import { useEffect, useState } from 'react'
import { Boxes, Plus, Trash2, RefreshCw, Shield, Zap, Wifi, WifiOff, HardDrive, Activity, Server } from 'lucide-react'
import api from '../api/client'
import type { CoreSanVolume, CoreSanBackend, CoreSanPeer, CoreSanStatus, CoreSanBenchmarkMatrix } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import SpecRow from '../components/SpecRow'
import Button from '../components/Button'
import ProgressBar from '../components/ProgressBar'
import Dialog from '../components/Dialog'
import FormField from '../components/FormField'
import TextInput from '../components/TextInput'
import Select from '../components/Select'
import ConfirmDialog from '../components/ConfirmDialog'
import { formatBytes } from '../utils/format'

type ResilienceMode = 'none' | 'mirror' | 'erasure'

const resilienceLabels: Record<ResilienceMode, string> = {
  none: 'No Protection',
  mirror: 'Mirror (RAID-1)',
  erasure: 'Erasure Coding (RAID-5/6)',
}

const resilienceColors: Record<ResilienceMode, string> = {
  none: 'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30',
  mirror: 'bg-vmm-success/20 text-vmm-success border-vmm-success/30',
  erasure: 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30',
}

const statusColors: Record<string, string> = {
  online: 'bg-vmm-success/20 text-vmm-success border-vmm-success/30',
  degraded: 'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30',
  offline: 'bg-vmm-danger/20 text-vmm-danger border-vmm-danger/30',
  creating: 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30',
  draining: 'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30',
  connecting: 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30',
}

function Badge({ label, color }: { label: string; color: string }) {
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-[10px] font-bold border tracking-wider uppercase ${color}`}>
      {label}
    </span>
  )
}

export default function StorageCoresan() {
  const [status, setStatus] = useState<CoreSanStatus | null>(null)
  const [volumes, setVolumes] = useState<CoreSanVolume[]>([])
  const [selectedVolume, setSelectedVolume] = useState<string>('')
  const [backends, setBackends] = useState<CoreSanBackend[]>([])
  const [peers, setPeers] = useState<CoreSanPeer[]>([])
  const [benchmarkMatrix, setBenchmarkMatrix] = useState<CoreSanBenchmarkMatrix | null>(null)
  const [loading, setLoading] = useState(true)
  const [sanAvailable, setSanAvailable] = useState(true)

  // Dialogs
  const [createVolumeOpen, setCreateVolumeOpen] = useState(false)
  const [addBackendOpen, setAddBackendOpen] = useState(false)
  const [deleteVolume, setDeleteVolume] = useState<CoreSanVolume | null>(null)
  const [deleteBackend, setDeleteBackend] = useState<CoreSanBackend | null>(null)

  // Create volume form
  const [newVolName, setNewVolName] = useState('')
  const [newVolMode, setNewVolMode] = useState<ResilienceMode>('mirror')
  const [newVolReplicas, setNewVolReplicas] = useState(2)
  const [newVolSync, setNewVolSync] = useState('async')

  // Add backend form
  const [newBackendPath, setNewBackendPath] = useState('')

  const sanApi = (path: string) => `http://localhost:7443${path}`

  const refresh = async () => {
    try {
      const [sRes, vRes, pRes] = await Promise.all([
        fetch(sanApi('/api/status')),
        fetch(sanApi('/api/volumes')),
        fetch(sanApi('/api/peers')),
      ])
      if (!sRes.ok) { setSanAvailable(false); setLoading(false); return }
      setSanAvailable(true)
      setStatus(await sRes.json())
      const vols: CoreSanVolume[] = await vRes.json()
      setVolumes(vols)
      setPeers(await pRes.json())
      if (vols.length > 0 && !selectedVolume) setSelectedVolume(vols[0].id)
      setLoading(false)
    } catch {
      setSanAvailable(false)
      setLoading(false)
    }
  }

  useEffect(() => { refresh() }, [])

  // Load backends when volume is selected
  useEffect(() => {
    if (!selectedVolume) return
    fetch(sanApi(`/api/volumes/${selectedVolume}/backends`))
      .then(r => r.json()).then(setBackends).catch(() => setBackends([]))
  }, [selectedVolume])

  // Load benchmark matrix
  useEffect(() => {
    if (!sanAvailable) return
    fetch(sanApi('/api/benchmark/matrix'))
      .then(r => r.json()).then(setBenchmarkMatrix).catch(() => {})
  }, [sanAvailable])

  const handleCreateVolume = async () => {
    await fetch(sanApi('/api/volumes'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        name: newVolName,
        resilience_mode: newVolMode,
        replica_count: newVolMode === 'none' ? 1 : newVolReplicas,
        sync_mode: newVolSync,
      }),
    })
    setCreateVolumeOpen(false)
    setNewVolName('')
    refresh()
  }

  const handleAddBackend = async () => {
    await fetch(sanApi(`/api/volumes/${selectedVolume}/backends`), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path: newBackendPath }),
    })
    setAddBackendOpen(false)
    setNewBackendPath('')
    // Reload backends
    fetch(sanApi(`/api/volumes/${selectedVolume}/backends`))
      .then(r => r.json()).then(setBackends).catch(() => {})
    refresh()
  }

  const handleDeleteVolume = async () => {
    if (!deleteVolume) return
    await fetch(sanApi(`/api/volumes/${deleteVolume.id}`), { method: 'DELETE' })
    setDeleteVolume(null)
    if (selectedVolume === deleteVolume.id) setSelectedVolume('')
    refresh()
  }

  const handleDeleteBackend = async () => {
    if (!deleteBackend) return
    await fetch(sanApi(`/api/volumes/${selectedVolume}/backends/${deleteBackend.id}`), { method: 'DELETE' })
    setDeleteBackend(null)
    fetch(sanApi(`/api/volumes/${selectedVolume}/backends`))
      .then(r => r.json()).then(setBackends).catch(() => {})
    refresh()
  }

  const handleRunBenchmark = async () => {
    await fetch(sanApi('/api/benchmark/run'), { method: 'POST' })
    setTimeout(() => {
      fetch(sanApi('/api/benchmark/matrix'))
        .then(r => r.json()).then(setBenchmarkMatrix).catch(() => {})
    }, 3000)
  }

  const sel = volumes.find(v => v.id === selectedVolume)

  if (loading) {
    return <div className="p-6 text-vmm-text-dim">Loading CoreSAN status...</div>
  }

  if (!sanAvailable) {
    return (
      <div className="p-6 max-w-2xl mx-auto">
        <Card>
          <div className="flex flex-col items-center gap-4 py-8">
            <div className="w-16 h-16 rounded-2xl bg-vmm-danger/10 flex items-center justify-center">
              <WifiOff size={32} className="text-vmm-danger" />
            </div>
            <h2 className="text-lg font-bold text-vmm-text">CoreSAN Not Available</h2>
            <p className="text-sm text-vmm-text-dim text-center max-w-md">
              The CoreSAN daemon (vmm-san) is not running on this host.
              Start it with <code className="text-vmm-accent">./tools/build-vmm-san.sh --run</code> or
              enable it via the Cluster Storage Wizard.
            </p>
          </div>
        </Card>
      </div>
    )
  }

  const totalBytes = volumes.reduce((sum, v) => sum + v.total_bytes, 0)
  const freeBytes = volumes.reduce((sum, v) => sum + v.free_bytes, 0)
  const usedBytes = totalBytes - freeBytes
  const usedPct = totalBytes > 0 ? (usedBytes / totalBytes) * 100 : 0

  return (
    <div className="p-6 space-y-6 max-w-7xl">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className="w-10 h-10 rounded-xl bg-vmm-accent/10 flex items-center justify-center">
            <Boxes size={22} className="text-vmm-accent" />
          </div>
          <div>
            <h1 className="text-lg font-bold text-vmm-text">CoreSAN</h1>
            <p className="text-xs text-vmm-text-muted">Software-Defined Storage</p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button variant="ghost" onClick={refresh}><RefreshCw size={14} /></Button>
          <Button variant="primary" onClick={() => setCreateVolumeOpen(true)}>
            <Plus size={14} /> New Volume
          </Button>
        </div>
      </div>

      {/* Status Overview */}
      <div className="grid grid-cols-1 md:grid-cols-4 gap-4">
        <Card>
          <div className="text-xs text-vmm-text-muted mb-1">Total Capacity</div>
          <div className="text-xl font-bold text-vmm-text">{formatBytes(totalBytes)}</div>
          <ProgressBar value={usedPct} detail={`${formatBytes(usedBytes)} used`}
            color={usedPct > 80 ? 'bg-vmm-danger' : usedPct > 60 ? 'bg-vmm-warning' : 'bg-vmm-accent'} />
        </Card>
        <Card>
          <div className="text-xs text-vmm-text-muted mb-1">Volumes</div>
          <div className="text-xl font-bold text-vmm-text">{volumes.length}</div>
          <div className="text-xs text-vmm-text-muted mt-1">
            {volumes.filter(v => v.status === 'online').length} online
          </div>
        </Card>
        <Card>
          <div className="text-xs text-vmm-text-muted mb-1">Peers</div>
          <div className="text-xl font-bold text-vmm-text">{peers.length}</div>
          <div className="text-xs text-vmm-text-muted mt-1">
            {peers.filter(p => p.status === 'online').length} online
          </div>
        </Card>
        <Card>
          <div className="text-xs text-vmm-text-muted mb-1">Node</div>
          <div className="text-sm font-bold text-vmm-text truncate">{status?.hostname}</div>
          <div className="text-xs text-vmm-text-muted mt-1 font-mono">{status?.node_id?.slice(0, 8)}...</div>
        </Card>
      </div>

      {/* Benchmark Summary */}
      {status?.benchmark_summary && (
        <Card>
          <div className="flex items-center justify-between mb-3">
            <SectionLabel>Network Performance</SectionLabel>
            <Button variant="ghost" onClick={handleRunBenchmark}><Activity size={13} /> Run Benchmark</Button>
          </div>
          <div className="grid grid-cols-3 gap-4">
            <SpecRow label="Avg Bandwidth" value={`${status.benchmark_summary.avg_bandwidth_mbps.toFixed(0)} Mbit/s`} />
            <SpecRow label="Avg Latency" value={`${status.benchmark_summary.avg_latency_us.toFixed(0)} μs`} />
            <SpecRow label="Worst Peer" value={status.benchmark_summary.worst_peer || 'N/A'} />
          </div>
        </Card>
      )}

      {/* Volumes + Volume Detail */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Volume List */}
        <div className="space-y-3">
          <SectionLabel>Volumes</SectionLabel>
          {volumes.length === 0 ? (
            <Card>
              <p className="text-sm text-vmm-text-dim text-center py-4">No volumes yet</p>
            </Card>
          ) : volumes.map(vol => {
            const volUsed = vol.total_bytes - vol.free_bytes
            const volPct = vol.total_bytes > 0 ? (volUsed / vol.total_bytes) * 100 : 0
            const effectiveCapacity = vol.resilience_mode === 'none' ? vol.total_bytes : Math.floor(vol.total_bytes / vol.replica_count)
            return (
              <Card key={vol.id} className={`cursor-pointer transition-all ${selectedVolume === vol.id ? 'ring-1 ring-vmm-accent' : 'hover:border-vmm-accent/30'}`}
                padding={false}>
                <div className="p-4" onClick={() => setSelectedVolume(vol.id)}>
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-sm font-bold text-vmm-text">{vol.name}</span>
                    <div className="flex items-center gap-1.5">
                      <Badge label={vol.status} color={statusColors[vol.status] || statusColors.offline} />
                      <Badge label={vol.resilience_mode} color={resilienceColors[vol.resilience_mode as ResilienceMode] || resilienceColors.none} />
                    </div>
                  </div>
                  <ProgressBar value={volPct} detail={`${formatBytes(volUsed)} / ${formatBytes(vol.total_bytes)}`}
                    color={volPct > 80 ? 'bg-vmm-danger' : 'bg-vmm-accent'} />
                  <div className="flex items-center justify-between mt-2 text-xs text-vmm-text-muted">
                    <span>{vol.backend_count} backend{vol.backend_count !== 1 ? 's' : ''}</span>
                    <span>Effective: {formatBytes(effectiveCapacity)}</span>
                  </div>
                </div>
              </Card>
            )
          })}
        </div>

        {/* Volume Detail */}
        <div className="lg:col-span-2 space-y-4">
          {sel ? (
            <>
              {/* Volume Info */}
              <Card>
                <div className="flex items-center justify-between mb-4">
                  <SectionLabel>Volume: {sel.name}</SectionLabel>
                  <Button variant="danger" onClick={() => setDeleteVolume(sel)}>
                    <Trash2 size={13} /> Delete
                  </Button>
                </div>
                <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                  <SpecRow label="Resilience" value={resilienceLabels[sel.resilience_mode as ResilienceMode] || sel.resilience_mode} icon={Shield} />
                  <SpecRow label="Replicas" value={`${sel.replica_count}x`} />
                  <SpecRow label="Sync Mode" value={sel.sync_mode === 'sync' ? 'Synchronous' : 'Asynchronous'} icon={Zap} />
                  <SpecRow label="Backends" value={`${sel.backend_count}`} icon={HardDrive} />
                </div>
              </Card>

              {/* Backends */}
              <Card>
                <div className="flex items-center justify-between mb-3">
                  <SectionLabel>Backends (Mountpoints)</SectionLabel>
                  <Button variant="primary" onClick={() => setAddBackendOpen(true)}>
                    <Plus size={13} /> Add Backend
                  </Button>
                </div>
                {backends.length === 0 ? (
                  <p className="text-sm text-vmm-text-dim text-center py-4">
                    No backends configured. Add a local mountpoint to provide storage.
                  </p>
                ) : (
                  <div className="space-y-2">
                    {backends.map(b => {
                      const bUsed = b.total_bytes - b.free_bytes
                      const bPct = b.total_bytes > 0 ? (bUsed / b.total_bytes) * 100 : 0
                      return (
                        <div key={b.id} className="flex items-center gap-3 p-3 rounded-lg bg-vmm-bg/50 border border-vmm-border">
                          <HardDrive size={16} className="text-vmm-text-muted shrink-0" />
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2 mb-1">
                              <span className="text-sm font-medium text-vmm-text truncate">{b.path}</span>
                              <Badge label={b.status} color={statusColors[b.status] || statusColors.offline} />
                            </div>
                            <ProgressBar value={bPct} detail={`${formatBytes(bUsed)} / ${formatBytes(b.total_bytes)}`}
                              color={bPct > 80 ? 'bg-vmm-danger' : 'bg-vmm-accent'} />
                          </div>
                          <button onClick={() => setDeleteBackend(b)}
                            className="p-1.5 rounded hover:bg-vmm-danger/10 text-vmm-text-muted hover:text-vmm-danger transition-colors">
                            <Trash2 size={14} />
                          </button>
                        </div>
                      )
                    })}
                  </div>
                )}
              </Card>
            </>
          ) : (
            <Card>
              <p className="text-sm text-vmm-text-dim text-center py-8">
                Select a volume to view its details, or create a new one.
              </p>
            </Card>
          )}

          {/* Peers */}
          <Card>
            <SectionLabel>Peers</SectionLabel>
            {peers.length === 0 ? (
              <p className="text-sm text-vmm-text-dim text-center py-4">
                No peers connected. CoreSAN is running in single-node mode.
              </p>
            ) : (
              <div className="space-y-2 mt-3">
                {peers.map(p => (
                  <div key={p.node_id} className="flex items-center gap-3 p-3 rounded-lg bg-vmm-bg/50 border border-vmm-border">
                    <Server size={16} className={p.status === 'online' ? 'text-vmm-success' : 'text-vmm-danger'} />
                    <div className="flex-1">
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium text-vmm-text">{p.hostname}</span>
                        <Badge label={p.status} color={statusColors[p.status] || statusColors.offline} />
                      </div>
                      <div className="text-xs text-vmm-text-muted">{p.address}</div>
                    </div>
                    {p.last_heartbeat && (
                      <span className="text-xs text-vmm-text-muted">
                        {new Date(p.last_heartbeat).toLocaleTimeString()}
                      </span>
                    )}
                  </div>
                ))}
              </div>
            )}
          </Card>

          {/* Benchmark Matrix */}
          {benchmarkMatrix && benchmarkMatrix.entries.length > 0 && (
            <Card>
              <div className="flex items-center justify-between mb-3">
                <SectionLabel>Network Performance Matrix</SectionLabel>
                <Button variant="ghost" onClick={handleRunBenchmark}><Activity size={13} /> Retest</Button>
              </div>
              <div className="overflow-x-auto">
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b border-vmm-border">
                      <th className="text-left py-2 px-2 text-vmm-text-muted">From → To</th>
                      <th className="text-right py-2 px-2 text-vmm-text-muted">Bandwidth</th>
                      <th className="text-right py-2 px-2 text-vmm-text-muted">Latency</th>
                      <th className="text-right py-2 px-2 text-vmm-text-muted">Jitter</th>
                      <th className="text-right py-2 px-2 text-vmm-text-muted">Loss</th>
                    </tr>
                  </thead>
                  <tbody>
                    {benchmarkMatrix.entries.map((e, i) => (
                      <tr key={i} className="border-b border-vmm-border/50">
                        <td className="py-2 px-2 text-vmm-text font-mono">
                          {e.from_node_id.slice(0, 8)} → {e.to_node_id.slice(0, 8)}
                        </td>
                        <td className="text-right py-2 px-2 text-vmm-text font-medium">{e.bandwidth_mbps.toFixed(0)} Mbit/s</td>
                        <td className="text-right py-2 px-2 text-vmm-text">{e.latency_us.toFixed(0)} μs</td>
                        <td className="text-right py-2 px-2 text-vmm-text">{e.jitter_us.toFixed(1)} μs</td>
                        <td className="text-right py-2 px-2">
                          <span className={e.packet_loss_pct > 0 ? 'text-vmm-danger' : 'text-vmm-success'}>
                            {e.packet_loss_pct.toFixed(1)}%
                          </span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            </Card>
          )}
        </div>
      </div>

      {/* Create Volume Dialog */}
      {createVolumeOpen && (
        <Dialog title="Create Volume" onClose={() => setCreateVolumeOpen(false)}>
          <div className="space-y-4">
            <FormField label="Volume Name">
              <TextInput value={newVolName} onChange={setNewVolName} placeholder="e.g. pool-a" />
            </FormField>
            <FormField label="Resilience Mode">
              <Select value={newVolMode} onChange={(v) => {
                setNewVolMode(v as ResilienceMode)
                if (v === 'none') setNewVolReplicas(1)
                else if (newVolReplicas < 2) setNewVolReplicas(2)
              }} options={[
                { value: 'none', label: 'No Protection (RAID-0) — 1 copy, maximum space' },
                { value: 'mirror', label: 'Mirror (RAID-1) — N copies, maximum safety' },
                { value: 'erasure', label: 'Erasure Coding (RAID-5/6) — balanced (coming soon)' },
              ]} />
            </FormField>
            {newVolMode === 'mirror' && (
              <FormField label="Replica Count">
                <Select value={String(newVolReplicas)} onChange={(v) => setNewVolReplicas(Number(v))} options={[
                  { value: '2', label: '2 copies — tolerates 1 node failure' },
                  { value: '3', label: '3 copies — tolerates 2 node failures' },
                  { value: '4', label: '4 copies — maximum redundancy' },
                ]} />
              </FormField>
            )}
            <FormField label="Sync Mode">
              <Select value={newVolSync} onChange={setNewVolSync} options={[
                { value: 'async', label: 'Asynchronous — fast writes, background replication' },
                { value: 'sync', label: 'Synchronous — wait for all replicas (slower, safer)' },
              ]} />
            </FormField>
            <div className="flex justify-end gap-2 pt-2">
              <Button variant="ghost" onClick={() => setCreateVolumeOpen(false)}>Cancel</Button>
              <Button variant="primary" onClick={handleCreateVolume} disabled={!newVolName.trim()}>
                Create Volume
              </Button>
            </div>
          </div>
        </Dialog>
      )}

      {/* Add Backend Dialog */}
      {addBackendOpen && (
        <Dialog title="Add Backend" onClose={() => setAddBackendOpen(false)}>
          <div className="space-y-4">
            <p className="text-sm text-vmm-text-dim">
              Add a local directory as storage backend for this volume.
              The directory must exist and be writable.
            </p>
            <FormField label="Directory Path">
              <TextInput value={newBackendPath} onChange={setNewBackendPath}
                placeholder="e.g. /mnt/data1" />
            </FormField>
            <div className="flex justify-end gap-2 pt-2">
              <Button variant="ghost" onClick={() => setAddBackendOpen(false)}>Cancel</Button>
              <Button variant="primary" onClick={handleAddBackend} disabled={!newBackendPath.trim()}>
                Add Backend
              </Button>
            </div>
          </div>
        </Dialog>
      )}

      {/* Delete Volume Confirm */}
      {deleteVolume && (
        <ConfirmDialog
          title="Delete Volume"
          message={`Are you sure you want to delete volume "${deleteVolume.name}"? This cannot be undone. The volume must be empty.`}
          confirmLabel="Delete"
          variant="danger"
          onConfirm={handleDeleteVolume}
          onCancel={() => setDeleteVolume(null)}
        />
      )}

      {/* Delete Backend Confirm */}
      {deleteBackend && (
        <ConfirmDialog
          title="Remove Backend"
          message={`Remove backend "${deleteBackend.path}"? If it contains data, it will be drained to other backends first.`}
          confirmLabel="Remove"
          variant="danger"
          onConfirm={handleDeleteBackend}
          onCancel={() => setDeleteBackend(null)}
        />
      )}
    </div>
  )
}
