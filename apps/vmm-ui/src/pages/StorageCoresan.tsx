import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Boxes, Plus, Trash2, RefreshCw, Shield, Zap, WifiOff, HardDrive, Activity, Server, AlertTriangle, Check, Disc, RotateCcw, FolderOpen } from 'lucide-react'
import type { CoreSanVolume, CoreSanBackend, CoreSanPeer, CoreSanStatus, CoreSanBenchmarkMatrix, Host, DiscoveredDisk } from '../api/types'
import { useClusterStore } from '../stores/clusterStore'
import api from '../api/client'
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
import VolumeBrowser from '../components/VolumeBrowser'
import { formatBytes } from '../utils/format'

const fttLabels: Record<number, string> = {
  0: 'FTT=0 (No Protection)',
  1: 'FTT=1 (1 Failure)',
  2: 'FTT=2 (2 Failures)',
}

const fttColors: Record<number, string> = {
  0: 'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30',
  1: 'bg-vmm-success/20 text-vmm-success border-vmm-success/30',
  2: 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30',
}

const raidLabels: Record<string, string> = {
  stripe: 'Stripe (RAID-0)',
  mirror: 'Mirror (RAID-1)',
  stripe_mirror: 'Stripe+Mirror (RAID-10)',
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
  const navigate = useNavigate()
  const { backendMode, hosts, fetchHosts } = useClusterStore()
  const isCluster = backendMode === 'cluster'

  const [status, setStatus] = useState<CoreSanStatus | null>(null)
  const [volumes, setVolumes] = useState<CoreSanVolume[]>([])
  const [selectedVolume, setSelectedVolume] = useState<string>('')
  const [backends, setBackends] = useState<CoreSanBackend[]>([])
  const [peers, setPeers] = useState<CoreSanPeer[]>([])
  const [benchmarkMatrix, setBenchmarkMatrix] = useState<CoreSanBenchmarkMatrix | null>(null)
  const [disks, setDisks] = useState<DiscoveredDisk[]>([])
  const [loading, setLoading] = useState(true)
  const [sanAvailable, setSanAvailable] = useState(true)

  // Disk claim dialog
  const [claimDisk, setClaimDisk] = useState<DiscoveredDisk | null>(null)
  const [claimVolumeId, setClaimVolumeId] = useState('')
  const [claimConfirm, setClaimConfirm] = useState(false)
  const [claimError, setClaimError] = useState('')
  const [resetDisk, setResetDisk] = useState<DiscoveredDisk | null>(null)
  const [browseVolume, setBrowseVolume] = useState<CoreSanVolume | null>(null)

  // Auto-claim dialog
  const [autoClaimOpen, setAutoClaimOpen] = useState(false)
  const [autoClaimSelected, setAutoClaimSelected] = useState<Set<string>>(new Set())
  const [autoClaimRunning, setAutoClaimRunning] = useState(false)
  const [autoClaimError, setAutoClaimError] = useState('')

  // Dialogs
  const [createVolumeOpen, setCreateVolumeOpen] = useState(false)
  const [addHostOpen, setAddHostOpen] = useState(false)
  const [deleteVolume, setDeleteVolume] = useState<CoreSanVolume | null>(null)
  const [deleteBackend, setDeleteBackend] = useState<CoreSanBackend | null>(null)

  // Create volume form
  const [newVolName, setNewVolName] = useState('')
  const [newVolFtt, setNewVolFtt] = useState(1)
  const [newVolRaid, setNewVolRaid] = useState('stripe')
  const [newVolBackendPath, setNewVolBackendPath] = useState('/vmm/san-data')
  const [newVolSelectedHosts, setNewVolSelectedHosts] = useState<string[]>([])
  const [newVolError, setNewVolError] = useState('')

  // Add host form
  const [addHostId, setAddHostId] = useState('')
  const [addHostBackendPath, setAddHostBackendPath] = useState('/vmm/san-data')
  const [addHostError, setAddHostError] = useState('')

  // In cluster mode, all SAN operations go through the cluster proxy (/api/san/*).
  // In standalone mode, talk directly to the local vmm-san instance.
  const localSanBase = `${window.location.protocol}//${window.location.hostname}:7443`
  const sanApi = (path: string) => isCluster ? `/api/san${path}` : `${localSanBase}${path}`
  // SAN-enabled hosts (for host selection in dialogs)
  const sanHosts = isCluster ? hosts.filter(h => h.san_enabled && h.san_address) : []

  /** Fetch wrapper that adds JWT auth when going through the cluster proxy. */
  const sanFetch = (url: string, init?: RequestInit) => {
    if (isCluster) {
      const token = localStorage.getItem('vmm_token')
      const headers = new Headers(init?.headers)
      if (token) headers.set('Authorization', `Bearer ${token}`)
      return fetch(url, { ...init, headers })
    }
    return fetch(url, init)
  }

  const refresh = async () => {
    if (isCluster) fetchHosts()
    try {
      const [sRes, vRes, pRes] = await Promise.all([
        sanFetch(sanApi('/api/status')),
        sanFetch(sanApi('/api/volumes')),
        sanFetch(sanApi('/api/peers')),
      ])
      if (!sRes.ok) { setSanAvailable(false); setLoading(false); return }
      setSanAvailable(true)
      // In cluster mode, /api/san/status returns an array of host statuses
      const statusData = await sRes.json()
      if (isCluster && Array.isArray(statusData)) {
        // Use the first host's status as the "primary" status display
        setStatus(statusData[0] || null)
      } else {
        setStatus(statusData)
      }
      const vols: CoreSanVolume[] = await vRes.json()
      setVolumes(vols)
      setPeers(await pRes.json())
      if (vols.length > 0 && !selectedVolume) setSelectedVolume(vols[0].id)

      // Disks: in cluster mode, /api/san/disks already aggregates from all hosts
      sanFetch(sanApi('/api/disks')).then(r => r.json()).then(setDisks).catch(() => setDisks([]))

      setLoading(false)
    } catch {
      setSanAvailable(false)
      setLoading(false)
    }
  }

  useEffect(() => { refresh() }, [isCluster, sanHosts.length])

  useEffect(() => {
    if (!selectedVolume) return
    sanFetch(sanApi(`/api/volumes/${selectedVolume}/backends`))
      .then(r => r.json()).then(setBackends).catch(() => setBackends([]))
  }, [selectedVolume])

  useEffect(() => {
    if (!sanAvailable) return
    sanFetch(sanApi('/api/benchmark/matrix'))
      .then(r => r.json()).then(setBenchmarkMatrix).catch(() => {})
  }, [sanAvailable])

  const handleCreateVolume = async () => {
    setNewVolError('')

    // Validate host selection
    const requiredHosts = newVolFtt + 1
    const selectedCount = newVolSelectedHosts.length + 1 // +1 for local node
    if (selectedCount < requiredHosts) {
      setNewVolError(`FTT=${newVolFtt} needs at least ${requiredHosts} hosts. Select ${requiredHosts - 1} more.`)
      return
    }

    // 1. Create volume
    const resp = await sanFetch(sanApi('/api/volumes'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        name: newVolName,
        ftt: newVolFtt,
        local_raid: newVolRaid,
      }),
    })
    if (!resp.ok) {
      setNewVolError(await resp.text() || 'Failed to create volume')
      return
    }
    const volData = await resp.json()
    const volumeId = volData.id

    // Auto backend path: /vmm/san-data/<volume-name>
    const backendPath = `/vmm/san-data/${newVolName}`

    // 2. Add backends on all selected hosts (cluster routes to correct host)
    // In cluster mode, the first SAN host gets a backend automatically via the volume create.
    // Add backends on additional selected hosts.
    for (const hostId of newVolSelectedHosts) {
      await sanFetch(sanApi(`/api/volumes/${volumeId}/backends`), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ host_id: hostId, path: backendPath }),
      }).catch(() => {})
    }
    // Peer registration is handled automatically by the cluster heartbeat engine.

    setCreateVolumeOpen(false)
    setNewVolName('')
    setNewVolSelectedHosts([])
    setNewVolError('')
    refresh()
  }

  const handleAddHost = async () => {
    setAddHostError('')
    if (!addHostId || !selectedVolume || !sel) return

    const backendPath = `/vmm/san-data/${sel.name}`

    try {
      // Add backend on the selected host — cluster routes to the correct SAN host.
      // Peer registration is handled automatically by the cluster heartbeat engine.
      const backendResp = await sanFetch(sanApi(`/api/volumes/${selectedVolume}/backends`), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ host_id: addHostId, path: backendPath }),
      })

      if (!backendResp.ok) {
        const text = await backendResp.text()
        setAddHostError(`Failed to add backend: ${text}`)
        return
      }

      setAddHostOpen(false)
      setAddHostId('')
      refresh()
      sanFetch(sanApi(`/api/volumes/${selectedVolume}/backends`))
        .then(r => r.json()).then(setBackends).catch(() => {})
    } catch (e: any) {
      setAddHostError(e.message || 'Failed to add host')
    }
  }

  const handleDeleteVolume = async () => {
    if (!deleteVolume) return
    await sanFetch(sanApi(`/api/volumes/${deleteVolume.id}`), { method: 'DELETE' })
    setDeleteVolume(null)
    if (selectedVolume === deleteVolume.id) setSelectedVolume('')
    refresh()
  }

  const handleDeleteBackend = async () => {
    if (!deleteBackend) return
    await sanFetch(sanApi(`/api/volumes/${selectedVolume}/backends/${deleteBackend.id}`), {
      method: 'DELETE',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ host_id: (deleteBackend as any)._host_id || '' }),
    })
    setDeleteBackend(null)
    sanFetch(sanApi(`/api/volumes/${selectedVolume}/backends`))
      .then(r => r.json()).then(setBackends).catch(() => {})
    refresh()
  }

  const handleRunBenchmark = async () => {
    await sanFetch(sanApi('/api/benchmark/run'), { method: 'POST' })
    setTimeout(() => {
      sanFetch(sanApi('/api/benchmark/matrix'))
        .then(r => r.json()).then(setBenchmarkMatrix).catch(() => {})
    }, 3000)
  }

  /** Unique key for a disk across hosts. */
  const diskKey = (d: DiscoveredDisk) => d._host_id ? `${d._host_id}:${d.path}` : d.path

  const openAutoClaim = () => {
    // Pre-select all empty (available) disks, leave has_data unchecked
    const claimable = disks.filter(d => d.status === 'available' || d.status === 'has_data')
    const preSelected = new Set(claimable.filter(d => d.status === 'available').map(diskKey))
    setAutoClaimSelected(preSelected)
    setAutoClaimError('')
    setAutoClaimOpen(true)
  }

  const handleAutoClaim = async () => {
    if (autoClaimSelected.size === 0) return
    setAutoClaimRunning(true)
    setAutoClaimError('')

    const keys = Array.from(autoClaimSelected)
    let ok = 0
    let fail = 0

    for (const key of keys) {
      const disk = disks.find(d => diskKey(d) === key)
      if (!disk) { fail++; continue }
      const resp = await sanFetch(sanApi('/api/disks/claim'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          host_id: disk._host_id || '',
          device_path: disk.path,
          confirm_format: true,
        }),
      })
      if (resp.ok) { ok++ } else { fail++ }
    }

    setAutoClaimRunning(false)
    if (fail > 0) {
      setAutoClaimError(`${ok} claimed, ${fail} failed`)
    } else {
      setAutoClaimOpen(false)
    }
    refresh()
  }

  const handleClaimDisk = async () => {
    setClaimError('')
    if (!claimDisk || !claimVolumeId) return
    const resp = await sanFetch(sanApi('/api/disks/claim'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        host_id: claimDisk._host_id || '',
        device_path: claimDisk.path,
        confirm_format: claimConfirm || claimDisk.status === 'available',
      }),
    })
    if (!resp.ok) {
      setClaimError(await resp.text() || 'Claim failed')
      return
    }
    setClaimDisk(null)
    setClaimConfirm(false)
    refresh()
  }

  const handleResetDisk = async () => {
    if (!resetDisk) return
    const resp = await sanFetch(sanApi('/api/disks/reset'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ host_id: resetDisk._host_id || '', device_path: resetDisk.path }),
    })
    if (!resp.ok) {
      alert(await resp.text() || 'Reset failed')
    }
    setResetDisk(null)
    refresh()
  }

  const handleReleaseDisk = async (disk: DiscoveredDisk) => {
    if (!confirm('Release this disk? Data will be drained to other backends.')) return
    await sanFetch(sanApi('/api/disks/release'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ host_id: disk._host_id || '', device_path: disk.path }),
    })
    refresh()
  }

  const sel = volumes.find(v => v.id === selectedVolume)

  // Group backends by node
  const backendsByNode = backends.reduce<Record<string, CoreSanBackend[]>>((acc, b) => {
    const key = b.node_id
    if (!acc[key]) acc[key] = []
    acc[key].push(b)
    return acc
  }, {})

  // Total node count = 1 (self) + peers
  const totalNodes = 1 + peers.length
  const onlineNodes = 1 + peers.filter(p => p.status === 'online').length

  // Cluster hosts without CoreSAN (available to add)
  const availableHosts = hosts.filter(h => h.status === 'online' && !h.san_enabled)

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
              {isCluster
                ? ' Use the Storage Wizard to set up CoreSAN across your cluster.'
                : ' Start it with the build script or enable it via the Cluster Storage Wizard.'}
            </p>
            {isCluster && (
              <Button variant="primary" onClick={() => navigate('/storage/wizard')}>
                Open Storage Wizard
              </Button>
            )}
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
          <Button variant="outline" onClick={openAutoClaim}>
            <Disc size={14} /> Auto-Claim
          </Button>
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
          <div className="text-xs text-vmm-text-muted mb-1">Nodes</div>
          <div className="text-xl font-bold text-vmm-text">{totalNodes}</div>
          <div className="text-xs text-vmm-text-muted mt-1">
            {onlineNodes} online
          </div>
        </Card>
        <Card>
          <div className="text-xs text-vmm-text-muted mb-1">This Node</div>
          <div className="text-sm font-bold text-vmm-text truncate">{status?.hostname}</div>
          <div className="text-xs text-vmm-text-muted mt-1 font-mono">{status?.node_id?.slice(0, 8)}...</div>
        </Card>
      </div>

      {/* Physical Disks */}
      {disks.length > 0 && (
        <Card>
          <div className="flex items-center justify-between mb-3">
            <SectionLabel>Physical Disks</SectionLabel>
            <span className="text-xs text-vmm-text-muted">
              {disks.filter(d => d.status === 'available' || d.status === 'has_data').length} available,{' '}
              {disks.filter(d => d.status === 'claimed').length} claimed
            </span>
          </div>
          <div className="overflow-x-auto">
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b border-vmm-border">
                  {isCluster && <th className="text-left py-2 px-2 text-vmm-text-muted">Host</th>}
                  <th className="text-left py-2 px-2 text-vmm-text-muted">Device</th>
                  <th className="text-left py-2 px-2 text-vmm-text-muted">Size</th>
                  <th className="text-left py-2 px-2 text-vmm-text-muted">Model</th>
                  <th className="text-left py-2 px-2 text-vmm-text-muted">Status</th>
                  <th className="text-right py-2 px-2 text-vmm-text-muted">Actions</th>
                </tr>
              </thead>
              <tbody>
                {disks.map(d => (
                  <tr key={diskKey(d)} className="border-b border-vmm-border/50">
                    {isCluster && <td className="py-2 px-2 text-vmm-text-dim text-xs">{d._host_name || '—'}</td>}
                    <td className="py-2 px-2 text-vmm-text font-mono flex items-center gap-2">
                      <Disc size={13} className={d.status === 'claimed' ? 'text-vmm-accent' : d.status === 'os_disk' ? 'text-vmm-danger' : 'text-vmm-text-muted'} />
                      {d.path}
                    </td>
                    <td className="py-2 px-2 text-vmm-text">{formatBytes(d.size_bytes)}</td>
                    <td className="py-2 px-2 text-vmm-text-dim">{d.model || '—'}</td>
                    <td className="py-2 px-2">
                      <Badge label={d.status.replace('_', ' ')} color={
                        d.status === 'available' ? statusColors.online :
                        d.status === 'claimed' ? 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30' :
                        d.status === 'has_data' ? statusColors.degraded :
                        d.status === 'os_disk' ? 'bg-vmm-danger/20 text-vmm-danger border-vmm-danger/30' :
                        statusColors.offline
                      } />
                    </td>
                    <td className="py-2 px-2 text-right">
                      {(d.status === 'available' || d.status === 'has_data') && volumes.length > 0 && (
                        <button onClick={() => { setClaimDisk(d); setClaimVolumeId(volumes[0]?.id || ''); setClaimConfirm(d.status === 'available'); setClaimError('') }}
                          className="px-2 py-1 text-[10px] font-bold text-vmm-accent hover:bg-vmm-accent/10 rounded transition-colors cursor-pointer">
                          CLAIM
                        </button>
                      )}
                      {d.status === 'has_data' && (
                        <button onClick={() => setResetDisk(d)}
                          className="px-2 py-1 text-[10px] font-bold text-vmm-warning hover:bg-vmm-warning/10 rounded transition-colors cursor-pointer ml-1">
                          RESET
                        </button>
                      )}
                      {d.status === 'claimed' && (
                        <button onClick={() => handleReleaseDisk(d)}
                          className="px-2 py-1 text-[10px] font-bold text-vmm-danger hover:bg-vmm-danger/10 rounded transition-colors cursor-pointer">
                          RELEASE
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </Card>
      )}

      {/* Participating Nodes */}
      <Card>
        <div className="flex items-center justify-between mb-3">
          <SectionLabel>Participating Nodes</SectionLabel>
          {isCluster && availableHosts.length > 0 && (
            <Button variant="primary" onClick={() => { setAddHostOpen(true); setAddHostId(availableHosts[0]?.id || '') }}>
              <Plus size={13} /> Add Host to CoreSAN
            </Button>
          )}
        </div>
        <div className="space-y-2">
          {/* This node */}
          <div className="flex items-center gap-3 p-3 rounded-lg bg-vmm-bg/50 border border-vmm-border">
            <Server size={16} className="text-vmm-success shrink-0" />
            <div className="flex-1">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-vmm-text">{status?.hostname}</span>
                <Badge label="online" color={statusColors.online} />
                <span className="text-[10px] text-vmm-text-muted">(this node)</span>
              </div>
              <div className="text-xs text-vmm-text-muted font-mono">{status?.node_id?.slice(0, 12)}...</div>
            </div>
            <span className="text-xs text-vmm-text-muted">
              {backendsByNode[status?.node_id || '']?.length || 0} backend{(backendsByNode[status?.node_id || '']?.length || 0) !== 1 ? 's' : ''}
            </span>
          </div>
          {/* Peers */}
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
              <span className="text-xs text-vmm-text-muted">
                {backendsByNode[p.node_id]?.length || 0} backend{(backendsByNode[p.node_id]?.length || 0) !== 1 ? 's' : ''}
              </span>
              {p.last_heartbeat && (
                <span className="text-xs text-vmm-text-muted">
                  {new Date(p.last_heartbeat).toLocaleTimeString()}
                </span>
              )}
            </div>
          ))}
          {peers.length === 0 && (
            <p className="text-xs text-vmm-text-muted py-2 px-3">
              Single-node mode — {isCluster ? 'add cluster hosts above for redundancy.' : 'add peers for redundancy.'}
            </p>
          )}
        </div>
      </Card>

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
            <SpecRow label="Worst Peer" value={status.benchmark_summary.worst_peer?.slice(0, 8) || 'N/A'} />
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
            const effectiveCapacity = vol.ftt === 0 ? vol.total_bytes : Math.floor(vol.total_bytes / (vol.ftt + 1))
            const needsWarning = vol.backend_count === 0
            return (
              <Card key={vol.id} className={`cursor-pointer transition-all ${selectedVolume === vol.id ? 'ring-1 ring-vmm-accent' : 'hover:border-vmm-accent/30'}`}
                padding={false}>
                <div className="p-4" onClick={() => setSelectedVolume(vol.id)}>
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-sm font-bold text-vmm-text">{vol.name}</span>
                    <div className="flex items-center gap-1.5">
                      <Badge label={vol.status} color={statusColors[vol.status] || statusColors.offline} />
                      <Badge label={`FTT=${vol.ftt}`} color={fttColors[vol.ftt] || fttColors[0]} />
                    </div>
                  </div>
                  {needsWarning ? (
                    <div className="flex items-center gap-2 py-1 text-xs text-vmm-warning">
                      <AlertTriangle size={12} /> No backends — add hosts to provide storage
                    </div>
                  ) : (
                    <>
                      <ProgressBar value={volPct} detail={`${formatBytes(volUsed)} / ${formatBytes(vol.total_bytes)}`}
                        color={volPct > 80 ? 'bg-vmm-danger' : 'bg-vmm-accent'} />
                      <div className="flex items-center justify-between mt-2 text-xs text-vmm-text-muted">
                        <span>{vol.backend_count} backend{vol.backend_count !== 1 ? 's' : ''}</span>
                        <span>Effective: {formatBytes(effectiveCapacity)}</span>
                      </div>
                    </>
                  )}
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
                  <div className="flex items-center gap-2">
                    <Button variant="outline" onClick={() => setBrowseVolume(sel)}>
                      <FolderOpen size={13} /> Browse
                    </Button>
                    <Button variant="danger" onClick={() => setDeleteVolume(sel)}>
                      <Trash2 size={13} /> Delete
                    </Button>
                  </div>
                </div>

                {/* Warning for mirror volumes without enough nodes/backends */}
                {sel.ftt > 0 && totalNodes < (sel.ftt + 1) && (
                  <div className="flex items-center gap-2 p-3 mb-4 rounded-lg bg-vmm-warning/10 border border-vmm-warning/30 text-sm text-vmm-warning">
                    <AlertTriangle size={16} />
                    FTT={sel.ftt} requires {sel.ftt + 1} nodes, but only {totalNodes} available.
                    {isCluster ? ' Add more cluster hosts to CoreSAN.' : ' Add more peers.'}
                  </div>
                )}

                <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
                  <SpecRow label="FTT" value={fttLabels[sel.ftt] || `FTT=${sel.ftt}`} icon={<Shield size={14} />} />
                  <SpecRow label="Local RAID" value={raidLabels[sel.local_raid] || sel.local_raid} />
                  <SpecRow label="Chunk Size" value={`${sel.chunk_size_bytes / (1024 * 1024)} MB`} icon={<Zap size={14} />} />
                  <SpecRow label="Backends" value={`${sel.backend_count}`} icon={<HardDrive size={14} />} />
                </div>
              </Card>

              {/* Backends grouped by node */}
              <Card>
                <div className="flex items-center justify-between mb-3">
                  <SectionLabel>Storage Backends</SectionLabel>
                  {isCluster && availableHosts.length > 0 && (
                    <Button variant="primary" onClick={() => { setAddHostOpen(true); setAddHostId(availableHosts[0]?.id || '') }}>
                      <Plus size={13} /> Add Host
                    </Button>
                  )}
                </div>
                {backends.length === 0 ? (
                  <div className="text-center py-6">
                    <p className="text-sm text-vmm-text-dim">
                      No backends configured.
                    </p>
                    <p className="text-xs text-vmm-text-muted mt-1">
                      {isCluster
                        ? 'Add cluster hosts to contribute storage to this volume.'
                        : 'Add a local mountpoint to provide storage.'}
                    </p>
                  </div>
                ) : (
                  <div className="space-y-3">
                    {Object.entries(backendsByNode).map(([nodeId, nodeBackends]) => {
                      const isLocal = nodeId === status?.node_id
                      const peer = peers.find(p => p.node_id === nodeId)
                      const nodeName = isLocal ? status?.hostname : peer?.hostname || nodeId.slice(0, 8)
                      return (
                        <div key={nodeId}>
                          <div className="flex items-center gap-2 mb-1.5">
                            <Server size={12} className={isLocal || peer?.status === 'online' ? 'text-vmm-success' : 'text-vmm-danger'} />
                            <span className="text-xs font-semibold text-vmm-text-muted uppercase tracking-wider">{nodeName}</span>
                            {isLocal && <span className="text-[9px] text-vmm-text-muted">(local)</span>}
                          </div>
                          {nodeBackends.map(b => {
                            const bUsed = b.total_bytes - b.free_bytes
                            const bPct = b.total_bytes > 0 ? (bUsed / b.total_bytes) * 100 : 0
                            return (
                              <div key={b.id} className="flex items-center gap-3 p-3 ml-4 rounded-lg bg-vmm-bg/50 border border-vmm-border mb-1.5">
                                <HardDrive size={14} className="text-vmm-text-muted shrink-0" />
                                <div className="flex-1 min-w-0">
                                  <div className="flex items-center gap-2 mb-1">
                                    <span className="text-xs font-medium text-vmm-text truncate">{b.path}</span>
                                    <Badge label={b.status} color={statusColors[b.status] || statusColors.offline} />
                                  </div>
                                  <ProgressBar value={bPct} detail={`${formatBytes(bUsed)} / ${formatBytes(b.total_bytes)}`}
                                    color={bPct > 80 ? 'bg-vmm-danger' : 'bg-vmm-accent'} />
                                </div>
                                <button onClick={() => setDeleteBackend(b)}
                                  className="p-1.5 rounded hover:bg-vmm-danger/10 text-vmm-text-muted hover:text-vmm-danger transition-colors cursor-pointer">
                                  <Trash2 size={14} />
                                </button>
                              </div>
                            )
                          })}
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
      <Dialog open={createVolumeOpen} title="Create Volume" onClose={() => { setCreateVolumeOpen(false); setNewVolError('') }} width="max-w-xl">
        <div className="space-y-4">
          {newVolError && (
            <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">
              {newVolError}
            </div>
          )}
          <FormField label="Volume Name">
            <TextInput value={newVolName} onChange={(e) => setNewVolName(e.target.value)} placeholder="e.g. pool-a" />
          </FormField>
          <FormField label="Failures To Tolerate (FTT)">
            <Select value={String(newVolFtt)} onChange={(e) => setNewVolFtt(Number(e.target.value))} options={[
              { value: '0', label: 'FTT=0 — No protection (data on 1 host only)' },
              { value: '1', label: 'FTT=1 — Tolerates 1 host failure (2 copies)' },
              { value: '2', label: 'FTT=2 — Tolerates 2 host failures (3 copies)' },
            ]} />
          </FormField>
          <FormField label="Local RAID (per host)">
            <Select value={newVolRaid} onChange={(e) => setNewVolRaid(e.target.value)} options={[
              { value: 'stripe', label: 'Stripe (RAID-0) — chunks distributed across local disks' },
              { value: 'mirror', label: 'Mirror (RAID-1) — every chunk on every local disk' },
              { value: 'stripe_mirror', label: 'Stripe+Mirror (RAID-10) — striped with local mirror' },
            ]} />
          </FormField>

          {/* Host Selection */}
          <div>
            <label className="block text-[11px] font-semibold tracking-widest text-vmm-text-muted uppercase mb-2">
              Hosts ({1 + newVolSelectedHosts.length} selected — {newVolFtt + 1} required for FTT={newVolFtt})
            </label>

            {/* This node (always included) */}
            <div className="space-y-1.5">
              <div className="flex items-center gap-3 p-2.5 rounded-lg bg-vmm-accent/5 border border-vmm-accent/30">
                <input type="checkbox" checked disabled className="accent-vmm-accent" />
                <Server size={14} className="text-vmm-success" />
                <span className="text-sm text-vmm-text">{status?.hostname}</span>
                <span className="text-[10px] text-vmm-text-muted">(this node — always included)</span>
              </div>

              {/* Other cluster hosts with CoreSAN */}
              {sanHosts.filter(h => h.san_node_id && h.san_node_id !== status?.node_id).map(h => {
                const checked = newVolSelectedHosts.includes(h.id)
                return (
                  <label key={h.id} className={`flex items-center gap-3 p-2.5 rounded-lg border cursor-pointer transition-colors
                    ${checked ? 'bg-vmm-accent/5 border-vmm-accent/30' : 'border-vmm-border hover:border-vmm-accent/20'}`}>
                    <input type="checkbox" checked={checked} onChange={() => {
                      setNewVolSelectedHosts(prev => checked ? prev.filter(id => id !== h.id) : [...prev, h.id])
                    }} className="accent-vmm-accent" />
                    <Server size={14} className={h.status === 'online' ? 'text-vmm-success' : 'text-vmm-text-muted'} />
                    <span className="text-sm text-vmm-text">{h.hostname}</span>
                    <span className="text-[10px] text-vmm-text-muted">{h.address}</span>
                  </label>
                )
              })}

              {/* Cluster hosts without CoreSAN */}
              {availableHosts.length > 0 && (
                <p className="text-[10px] text-vmm-text-muted pt-1">
                  {availableHosts.length} host{availableHosts.length !== 1 ? 's' : ''} without CoreSAN not shown.
                  Enable CoreSAN on them first.
                </p>
              )}

              {newVolFtt > 0 && (1 + newVolSelectedHosts.length) < (newVolFtt + 1) && (
                <div className="flex items-center gap-2 p-2.5 rounded-lg bg-vmm-warning/10 border border-vmm-warning/30 text-xs text-vmm-warning">
                  <AlertTriangle size={14} />
                  FTT={newVolFtt} requires {newVolFtt + 1} hosts. Select {newVolFtt - newVolSelectedHosts.length} more.
                </div>
              )}
            </div>
          </div>

          <p className="text-[10px] text-vmm-text-muted">
            Storage will be automatically provisioned at <code className="text-vmm-accent">/vmm/san-data/{newVolName || '<name>'}</code> on each host.
          </p>

          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" onClick={() => { setCreateVolumeOpen(false); setNewVolError('') }}>Cancel</Button>
            <Button variant="primary" onClick={handleCreateVolume}
              disabled={!newVolName.trim() || (newVolFtt > 0 && (1 + newVolSelectedHosts.length) < (newVolFtt + 1))}>
              Create Volume
            </Button>
          </div>
        </div>
      </Dialog>

      {/* Add Host to CoreSAN Dialog */}
      <Dialog open={addHostOpen} title="Add Host to CoreSAN" onClose={() => { setAddHostOpen(false); setAddHostError('') }}>
        <div className="space-y-4">
          <p className="text-sm text-vmm-text-dim">
            Select a cluster host to add to this volume. Storage will be provisioned automatically.
          </p>
          {addHostError && (
            <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">
              {addHostError}
            </div>
          )}
          <FormField label="Host">
            <Select value={addHostId} onChange={(e) => setAddHostId(e.target.value)}
              options={availableHosts.map(h => ({ value: h.id, label: `${h.hostname} (${h.address})` }))} />
          </FormField>
          {sel && (
            <p className="text-[10px] text-vmm-text-muted">
              Backend will be created at <code className="text-vmm-accent">/vmm/san-data/{sel.name}</code> on the selected host.
            </p>
          )}
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" onClick={() => { setAddHostOpen(false); setAddHostError('') }}>Cancel</Button>
            <Button variant="primary" onClick={handleAddHost} disabled={!addHostId}>
              Add Host
            </Button>
          </div>
        </div>
      </Dialog>

      {/* Delete Volume Confirm */}
      <ConfirmDialog
        open={!!deleteVolume}
        title="Delete Volume"
        message={deleteVolume ? `Are you sure you want to delete volume "${deleteVolume.name}"? This cannot be undone. The volume must be empty.` : ''}
        confirmLabel="Delete"
        danger
        onConfirm={handleDeleteVolume}
        onCancel={() => setDeleteVolume(null)}
      />

      {/* Delete Backend Confirm */}
      <ConfirmDialog
        open={!!deleteBackend}
        title="Remove Backend"
        message={deleteBackend ? `Remove backend "${deleteBackend.path}"? If it contains data, it will be drained to other backends first.` : ''}
        confirmLabel="Remove"
        danger
        onConfirm={handleDeleteBackend}
        onCancel={() => setDeleteBackend(null)}
      />

      {/* Claim Disk Dialog */}
      <Dialog open={!!claimDisk} title="Claim Disk" onClose={() => { setClaimDisk(null); setClaimError('') }}>
        {claimDisk && (
          <div className="space-y-4">
            <div className="flex items-center gap-3 p-3 rounded-lg bg-vmm-bg/50 border border-vmm-border">
              <Disc size={20} className="text-vmm-accent" />
              <div>
                <div className="text-sm font-bold text-vmm-text">{claimDisk.path}</div>
                <div className="text-xs text-vmm-text-muted">{formatBytes(claimDisk.size_bytes)} — {claimDisk.model || 'Unknown model'}</div>
              </div>
            </div>

            {claimDisk.status === 'has_data' && (
              <div className="flex items-center gap-2 p-3 rounded-lg bg-vmm-danger/10 border border-vmm-danger/30 text-sm text-vmm-danger">
                <AlertTriangle size={16} />
                This disk has existing data ({claimDisk.fs_type || 'unknown'}). It will be wiped and reformatted. All data will be lost!
              </div>
            )}

            {claimError && (
              <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">{claimError}</div>
            )}

            {claimDisk.status === 'has_data' && (
              <label className="flex items-center gap-2 cursor-pointer">
                <input type="checkbox" checked={claimConfirm} onChange={e => setClaimConfirm(e.target.checked)} className="accent-vmm-danger" />
                <span className="text-sm text-vmm-text">I confirm all data on this disk will be destroyed</span>
              </label>
            )}

            <div className="flex justify-end gap-2 pt-2">
              <Button variant="ghost" onClick={() => { setClaimDisk(null); setClaimError('') }}>Cancel</Button>
              <Button variant="primary" onClick={handleClaimDisk}
                disabled={!claimVolumeId || (claimDisk.status === 'has_data' && !claimConfirm)}>
                Claim & Format Disk
              </Button>
            </div>
          </div>
        )}
      </Dialog>

      {/* Reset Disk Confirm */}
      <ConfirmDialog
        open={!!resetDisk}
        title="Reset Disk"
        message={resetDisk ? `Reset disk "${resetDisk.path}" (${resetDisk.model || 'Unknown'}, ${formatBytes(resetDisk.size_bytes)})?\n\nThis will DESTROY all data, partition tables, and filesystem signatures. The disk will become available for CoreSAN.` : ''}
        confirmLabel="Reset & Wipe"
        danger
        onConfirm={handleResetDisk}
        onCancel={() => setResetDisk(null)}
      />

      {/* Volume Browser */}
      <VolumeBrowser
        open={!!browseVolume}
        onClose={() => setBrowseVolume(null)}
        volumeId={browseVolume?.id || ''}
        volumeName={browseVolume?.name || ''}
        sanApi={sanApi}
      />

      {/* Auto-Claim Dialog */}
      <Dialog open={autoClaimOpen} title="Auto-Claim Disks" onClose={() => setAutoClaimOpen(false)} width="max-w-4xl">
        <div className="space-y-4">
          <p className="text-sm text-vmm-text-dim">
            Select disks to claim for CoreSAN. Empty disks are pre-selected.
            Disks with existing data must be explicitly selected (they will be formatted).
            OS disks cannot be selected.
          </p>

          {autoClaimError && (
            <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">{autoClaimError}</div>
          )}

          {/* Disks grouped by host */}
          <div className="space-y-4 max-h-[50vh] overflow-y-auto">
            {/* Group disks by host */}
            {(() => {
              const unclaimedDisks = disks.filter(d => d.status !== 'claimed')
              const groups: Record<string, { label: string; disks: DiscoveredDisk[] }> = {}
              for (const d of unclaimedDisks) {
                const groupKey = d._host_id || '__local__'
                if (!groups[groupKey]) {
                  groups[groupKey] = {
                    label: d._host_name || status?.hostname || 'This node',
                    disks: [],
                  }
                }
                groups[groupKey].disks.push(d)
              }
              return Object.entries(groups).map(([groupKey, group]) => (
                <div key={groupKey}>
                  <div className="flex items-center gap-2 mb-2">
                    <Server size={14} className="text-vmm-success" />
                    <span className="text-xs font-bold text-vmm-text uppercase tracking-wider">{group.label}</span>
                    {groupKey === '__local__' && <span className="text-[10px] text-vmm-text-muted">(this node)</span>}
                  </div>
                  <div className="space-y-1.5 ml-5">
                    {group.disks.map(d => {
                      const key = diskKey(d)
                      const isOsDisk = d.status === 'os_disk' || d.status === 'in_use'
                      const hasData = d.status === 'has_data'
                      const checked = autoClaimSelected.has(key)
                      return (
                        <label key={key} className={`flex items-center gap-3 p-3 rounded-lg border transition-colors
                          ${isOsDisk ? 'opacity-40 cursor-not-allowed border-vmm-border' :
                            checked ? 'bg-vmm-accent/5 border-vmm-accent/30 cursor-pointer' :
                            'border-vmm-border hover:border-vmm-accent/20 cursor-pointer'}`}>
                          <input
                            type="checkbox"
                            checked={checked}
                            disabled={isOsDisk}
                            onChange={() => {
                              setAutoClaimSelected(prev => {
                                const next = new Set(prev)
                                if (next.has(key)) next.delete(key)
                                else next.add(key)
                                return next
                              })
                            }}
                            className="accent-vmm-accent"
                          />
                          <Disc size={16} className={isOsDisk ? 'text-vmm-danger' : checked ? 'text-vmm-accent' : 'text-vmm-text-muted'} />
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2">
                              <span className="text-sm font-mono font-medium text-vmm-text">{d.path}</span>
                              <span className="text-xs text-vmm-text-dim">{formatBytes(d.size_bytes)}</span>
                              {d.model && <span className="text-xs text-vmm-text-muted">{d.model}</span>}
                            </div>
                          </div>
                          <div className="shrink-0">
                            {isOsDisk && (
                              <Badge label="OS DISK" color="bg-vmm-danger/20 text-vmm-danger border-vmm-danger/30" />
                            )}
                            {d.status === 'in_use' && (
                              <Badge label="IN USE" color={statusColors.offline} />
                            )}
                            {d.status === 'available' && (
                              <Badge label="EMPTY" color={statusColors.online} />
                            )}
                            {hasData && (
                              <Badge label={`HAS DATA (${d.fs_type || '?'})`} color={statusColors.degraded} />
                            )}
                          </div>
                        </label>
                      )
                    })}
                    {group.disks.length === 0 && (
                      <p className="text-xs text-vmm-text-muted py-2">No unclaimed disks on this node.</p>
                    )}
                  </div>
                </div>
              ))
            })()}
          </div>

          {autoClaimSelected.size > 0 && disks.some(d => autoClaimSelected.has(diskKey(d)) && d.status === 'has_data') && (
            <div className="flex items-center gap-2 p-3 rounded-lg bg-vmm-warning/10 border border-vmm-warning/30 text-xs text-vmm-warning">
              <AlertTriangle size={14} />
              {disks.filter(d => autoClaimSelected.has(diskKey(d)) && d.status === 'has_data').length} disk(s)
              with existing data selected. They will be wiped and formatted!
            </div>
          )}

          <div className="flex items-center justify-between pt-2 border-t border-vmm-border">
            <span className="text-sm text-vmm-text-dim">
              {autoClaimSelected.size} disk{autoClaimSelected.size !== 1 ? 's' : ''} selected
              ({formatBytes(disks.filter(d => autoClaimSelected.has(diskKey(d))).reduce((s, d) => s + d.size_bytes, 0))} total)
            </span>
            <div className="flex items-center gap-2">
              <Button variant="ghost" onClick={() => setAutoClaimOpen(false)}>Cancel</Button>
              <Button variant="primary" onClick={handleAutoClaim}
                disabled={autoClaimSelected.size === 0 || autoClaimRunning}>
                {autoClaimRunning ? 'Claiming...' : `Claim ${autoClaimSelected.size} Disk${autoClaimSelected.size !== 1 ? 's' : ''}`}
              </Button>
            </div>
          </div>
        </div>
      </Dialog>
    </div>
  )
}
