import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Boxes, Plus, Trash2, RefreshCw, Shield, Zap, WifiOff, HardDrive, Activity, Server, AlertTriangle, Disc, FolderOpen, Grid3x3, Pencil, X } from 'lucide-react'
import type { CoreSanVolume, CoreSanBackend, CoreSanPeer, CoreSanStatus, CoreSanBenchmarkMatrix, DiscoveredDisk } from '../api/types'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import SpecRow from '../components/SpecRow'
import Button from '../components/Button'
import ProgressBar from '../components/ProgressBar'
import ConfirmDialog from '../components/ConfirmDialog'
import VolumeBrowser from '../components/VolumeBrowser'
import { formatBytes } from '../utils/format'
import { Badge, fttLabels, fttColors, raidLabels, statusColors } from '../components/coresan/constants'
import CreateVolumeDialog from '../components/coresan/CreateVolumeDialog'
import AddHostDialog from '../components/coresan/AddHostDialog'
import ClaimDiskDialog from '../components/coresan/ClaimDiskDialog'
import AutoClaimDialog from '../components/coresan/AutoClaimDialog'
import SmartDetailDialog from '../components/coresan/SmartDetailDialog'
import EventFeed from '../components/EventFeed'

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
  const [sanHealth, setSanHealth] = useState<any>(null)

  // Disk claim dialog
  const [claimDisk, setClaimDisk] = useState<DiscoveredDisk | null>(null)
  const [claimConfirm, setClaimConfirm] = useState(false)
  const [claimError, setClaimError] = useState('')
  const [resetDisk, setResetDisk] = useState<DiscoveredDisk | null>(null)
  const [browseVolume, setBrowseVolume] = useState<CoreSanVolume | null>(null)
  const [backendsExpanded, setBackendsExpanded] = useState(false)

  // Auto-claim dialog
  const [autoClaimOpen, setAutoClaimOpen] = useState(false)
  const [autoClaimSelected, setAutoClaimSelected] = useState<Set<string>>(new Set())
  const [autoClaimRunning, setAutoClaimRunning] = useState(false)
  const [autoClaimError, setAutoClaimError] = useState('')

  // Dialogs
  const [createVolumeOpen, setCreateVolumeOpen] = useState(false)
  const [addHostOpen, setAddHostOpen] = useState(false)
  const [deleteVolume, setDeleteVolume] = useState<CoreSanVolume | null>(null)
  const [forceDeleteVolume, setForceDeleteVolume] = useState(false)
  const [deleteBackend, setDeleteBackend] = useState<CoreSanBackend | null>(null)

  // Repair
  const [repairRunning, setRepairRunning] = useState(false)
  const [repairResult, setRepairResult] = useState<any>(null)

  // Volume health
  const [volumeHealth, setVolumeHealth] = useState<any>(null)
  const [healthLoading, setHealthLoading] = useState(false)

  // Remove host from volume
  const [removeHostNode, setRemoveHostNode] = useState<string>('')

  // SMART detail dialog
  const [smartDisk, setSmartDisk] = useState<DiscoveredDisk | null>(null)

  // Edit volume dialog
  const [editVolumeOpen, setEditVolumeOpen] = useState(false)
  const [editFtt, setEditFtt] = useState(0)
  const [editRaid, setEditRaid] = useState('stripe')
  const [editError, setEditError] = useState('')
  const [editSaving, setEditSaving] = useState(false)

  // Tab navigation
  const [activeTab, setActiveTab] = useState<'volumes' | 'disks' | 'performance'>('volumes')

  // Create volume form
  const [newVolName, setNewVolName] = useState('')
  const [newVolSizeGb, setNewVolSizeGb] = useState(10)
  const [newVolFtt, setNewVolFtt] = useState(1)
  const [newVolRaid, setNewVolRaid] = useState('stripe')
  const [newVolSelectedHosts, setNewVolSelectedHosts] = useState<string[]>([])
  const [newVolProtocols, setNewVolProtocols] = useState<string[]>(['fuse'])
  const [newVolError, setNewVolError] = useState('')

  // Add host form
  const [addHostId, setAddHostId] = useState('')
  const [addHostError, setAddHostError] = useState('')

  // SAN API helpers
  const localSanBase = `${window.location.protocol}//${window.location.hostname}:7443`
  const sanApi = (path: string) => isCluster ? `/api/san${path.replace(/^\/api/, '')}` : `${localSanBase}${path}`
  const sanHosts = isCluster ? hosts.filter(h => h.san_enabled && h.san_address) : []

  const sanFetch = (url: string, init?: RequestInit) => {
    if (isCluster) {
      const token = localStorage.getItem('vmm_token')
      const headers = new Headers(init?.headers)
      if (token) headers.set('Authorization', `Bearer ${token}`)
      return fetch(url, { ...init, headers })
    }
    return fetch(url, init)
  }

  // ── Data fetching ────────────────────────────────────────────

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
      const statusData = await sRes.json()
      if (isCluster && Array.isArray(statusData)) {
        setStatus(statusData[0] || null)
      } else {
        setStatus(statusData)
      }
      const vols = vRes.ok ? await vRes.json() : []
      setVolumes(Array.isArray(vols) ? vols : [])
      const peersData = pRes.ok ? await pRes.json() : []
      setPeers(Array.isArray(peersData) ? peersData : [])
      if (Array.isArray(vols) && vols.length > 0 && !selectedVolume) setSelectedVolume(vols[0].id)
      sanFetch(sanApi('/api/disks')).then(r => r.ok ? r.json() : []).then(d => setDisks(Array.isArray(d) ? d : [])).catch(() => setDisks([]))
      if (isCluster) {
        sanFetch(sanApi('/api/health')).then(r => r.ok ? r.json() : null).then(setSanHealth).catch(() => {})
      }
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

  // ── Handlers ─────────────────────────────────────────────────

  const handleCreateVolume = async () => {
    setNewVolError('')
    const requiredHosts = newVolFtt + 1
    const selectedCount = newVolSelectedHosts.length + 1
    if (selectedCount < requiredHosts) {
      setNewVolError(`FTT=${newVolFtt} needs at least ${requiredHosts} hosts. Select ${requiredHosts - 1} more.`)
      return
    }
    // Create volume — backends are automatically the claimed disks on each node.
    // No manual backend creation needed.
    const resp = await sanFetch(sanApi('/api/volumes'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ name: newVolName, max_size_bytes: newVolSizeGb * 1024 * 1024 * 1024, ftt: newVolFtt, local_raid: newVolRaid, access_protocols: newVolProtocols }),
    })
    if (!resp.ok) { setNewVolError(await resp.text() || 'Failed to create volume'); return }
    setCreateVolumeOpen(false)
    setNewVolName('')
    setNewVolSelectedHosts([])
    setNewVolProtocols(['fuse'])
    setNewVolError('')
    refresh()
  }

  const handleAddHost = async () => {
    setAddHostError('')
    if (!addHostId || !selectedVolume || !sel) return
    try {
      // Peer registration is automatic — the volume sync happens when the SAN peer is online.
      // No manual backend creation needed (backends = claimed disks).
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
    const url = forceDeleteVolume
      ? sanApi(`/api/volumes/${deleteVolume.id}?force=true`)
      : sanApi(`/api/volumes/${deleteVolume.id}`)
    await sanFetch(url, { method: 'DELETE' })
    setDeleteVolume(null)
    setForceDeleteVolume(false)
    if (selectedVolume === deleteVolume.id) setSelectedVolume('')
    refresh()
  }

  const handleRepair = async () => {
    if (!selectedVolume) return
    setRepairRunning(true)
    setRepairResult(null)
    try {
      const resp = await sanFetch(sanApi(`/api/volumes/${selectedVolume}/repair`), { method: 'POST' })
      if (resp.ok) {
        const data = await resp.json()
        setRepairResult(data)
      }
    } catch { /* ignore */ }
    setRepairRunning(false)
    refresh()
  }

  const loadVolumeHealth = async (volId: string) => {
    setHealthLoading(true)
    try {
      const resp = await sanFetch(sanApi(`/api/volumes/${volId}/health`))
      if (resp.ok) setVolumeHealth(await resp.json())
    } catch { /* ignore */ }
    setHealthLoading(false)
  }

  const handleRemoveHost = async (nodeId: string) => {
    if (!selectedVolume) return
    const resp = await sanFetch(sanApi(`/api/volumes/${selectedVolume}/remove-host`), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ node_id: nodeId }),
    })
    if (!resp.ok) {
      const data = await resp.json().catch(() => ({}))
      alert(data.error || 'Failed to remove host')
    }
    refresh()
  }

  const openEditVolume = () => {
    if (!sel) return
    setEditFtt(sel.ftt)
    setEditRaid(sel.local_raid)
    setEditError('')
    setEditVolumeOpen(true)
  }

  const handleEditVolume = async () => {
    if (!sel) return
    setEditSaving(true)
    setEditError('')
    try {
      const resp = await sanFetch(sanApi(`/api/volumes/${sel.id}`), {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ ftt: editFtt, local_raid: editRaid }),
      })
      if (!resp.ok) {
        const data = await resp.text()
        setEditError(data || 'Failed to update volume')
        setEditSaving(false)
        return
      }
      setEditVolumeOpen(false)
      refresh()
    } catch (e: any) {
      setEditError(e.message || 'Failed to update volume')
    }
    setEditSaving(false)
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

  const diskKey = (d: DiscoveredDisk) => d._host_id ? `${d._host_id}:${d.path}` : d.path

  const openAutoClaim = () => {
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
    let ok = 0, fail = 0
    for (const key of keys) {
      const disk = disks.find(d => diskKey(d) === key)
      if (!disk) { fail++; continue }
      const resp = await sanFetch(sanApi('/api/disks/claim'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ host_id: disk._host_id || '', device_path: disk.path, confirm_format: true }),
      })
      if (resp.ok) { ok++ } else { fail++ }
    }
    setAutoClaimRunning(false)
    if (fail > 0) { setAutoClaimError(`${ok} claimed, ${fail} failed`) } else { setAutoClaimOpen(false) }
    refresh()
  }

  const handleClaimDisk = async () => {
    setClaimError('')
    if (!claimDisk) return
    const resp = await sanFetch(sanApi('/api/disks/claim'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        host_id: claimDisk._host_id || '',
        device_path: claimDisk.path,
        confirm_format: claimConfirm || claimDisk.status === 'available',
      }),
    })
    if (!resp.ok) { setClaimError(await resp.text() || 'Claim failed'); return }
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
    if (!resp.ok) alert(await resp.text() || 'Reset failed')
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

  // ── Derived state ────────────────────────────────────────────

  const sel = volumes.find(v => v.id === selectedVolume)
  const backendsByNode = backends.reduce<Record<string, CoreSanBackend[]>>((acc, b) => {
    if (!acc[b.node_id]) acc[b.node_id] = []
    acc[b.node_id].push(b)
    return acc
  }, {})
  const totalNodes = 1 + peers.length
  const onlineNodes = 1 + peers.filter(p => p.status === 'online').length
  const availableHosts = hosts.filter(h => h.status === 'online' && !h.san_enabled)

  // ── Render ───────────────────────────────────────────────────

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

      {/* Cluster Health Banner */}
      {(() => {
        const q = status?.quorum_status || 'unknown'
        const degradedVolumes = volumes.filter(v => v.status === 'degraded').length
        const offlineVolumes = volumes.filter(v => v.status === 'offline').length
        const totalStaleChunks = (status?.volumes || []).reduce((sum, v) => sum + (v.stale_chunks || 0), 0)
        const totalDegradedFiles = (status?.volumes || []).reduce((sum, v) => sum + (v.degraded_files || 0), 0)
        const isResyncing = totalStaleChunks > 0
        const hasProblem = q === 'fenced' || q === 'degraded' || degradedVolumes > 0 || offlineVolumes > 0 || onlineNodes < totalNodes || isResyncing || totalDegradedFiles > 0
        const isCritical = q === 'fenced' || offlineVolumes > 0

        if (hasProblem) {
          return (
            <div className={`flex items-center gap-3 px-4 py-3 rounded-lg border ${
              isCritical
                ? 'bg-vmm-danger/10 border-vmm-danger/30 text-vmm-danger'
                : isResyncing && !degradedVolumes && onlineNodes >= totalNodes
                  ? 'bg-vmm-accent/10 border-vmm-accent/30 text-vmm-accent'
                  : 'bg-vmm-warning/10 border-vmm-warning/30 text-vmm-warning'
            }`}>
              {isResyncing && !isCritical && !degradedVolumes
                ? <RefreshCw size={18} className="shrink-0 animate-spin" />
                : <AlertTriangle size={18} className="shrink-0" />}
              <div className="flex-1 text-sm">
                {q === 'fenced' && <strong>FENCED — </strong>}
                {q === 'degraded' && <strong>DEGRADED — </strong>}
                {isResyncing && !isCritical && <strong>RESYNCING — </strong>}
                {onlineNodes < totalNodes && `${totalNodes - onlineNodes} of ${totalNodes} node${totalNodes > 1 ? 's' : ''} offline. `}
                {degradedVolumes > 0 && `${degradedVolumes} volume${degradedVolumes > 1 ? 's' : ''} degraded. `}
                {offlineVolumes > 0 && `${offlineVolumes} volume${offlineVolumes > 1 ? 's' : ''} offline. `}
                {totalDegradedFiles > 0 && `${totalDegradedFiles} file${totalDegradedFiles > 1 ? 's' : ''} with reduced protection. `}
                {isResyncing && `${totalStaleChunks} chunk${totalStaleChunks > 1 ? 's' : ''} syncing. `}
                {q === 'fenced' && 'Writes are blocked — restore quorum to resume operations.'}
                {q === 'degraded' && onlineNodes >= totalNodes && 'Cluster is operating with reduced redundancy.'}
                {isResyncing && !isCritical && !degradedVolumes && 'Data is being redistributed across nodes.'}
              </div>
            </div>
          )
        }
        return null
      })()}

      {/* Status Overview */}
      <div className="grid grid-cols-2 md:grid-cols-5 gap-4">
        <Card>
          <div className="text-xs text-vmm-text-muted mb-1">Cluster Status</div>
          <div className={`text-lg font-bold capitalize ${
            status?.quorum_status === 'healthy' ? 'text-vmm-success' :
            status?.quorum_status === 'degraded' ? 'text-vmm-warning' :
            status?.quorum_status === 'fenced' ? 'text-vmm-danger' :
            'text-vmm-text'
          }`}>{status?.quorum_status || '—'}</div>
          <div className="text-xs text-vmm-text-muted mt-1">
            {status?.is_leader ? 'Leader' : 'Follower'}
          </div>
        </Card>
        <Card>
          <div className="text-xs text-vmm-text-muted mb-1">Total Capacity</div>
          <div className="text-lg font-bold text-vmm-text">{formatBytes(totalBytes)}</div>
          <ProgressBar value={usedPct} detail={`${formatBytes(usedBytes)} used`}
            color={usedPct > 80 ? 'bg-vmm-danger' : usedPct > 60 ? 'bg-vmm-warning' : 'bg-vmm-accent'} />
        </Card>
        <Card>
          <div className="text-xs text-vmm-text-muted mb-1">Volumes</div>
          <div className="text-lg font-bold text-vmm-text">{volumes.length}</div>
          <div className="text-xs text-vmm-text-muted mt-1">{volumes.filter(v => v.status === 'online').length} online</div>
        </Card>
        <Card>
          <div className="text-xs text-vmm-text-muted mb-1">Nodes</div>
          <div className={`text-lg font-bold ${onlineNodes < totalNodes ? 'text-vmm-warning' : 'text-vmm-text'}`}>
            {onlineNodes}/{totalNodes}
          </div>
          <div className="text-xs text-vmm-text-muted mt-1">
            {status?.claimed_disks || 0} disks claimed
          </div>
        </Card>
        <Card>
          <div className="text-xs text-vmm-text-muted mb-1">This Node</div>
          <div className="text-sm font-bold text-vmm-text truncate">{status?.hostname}</div>
          <div className="text-xs text-vmm-text-muted mt-1 font-mono">{status?.node_id?.slice(0, 8)}...</div>
        </Card>
      </div>

      {/* Tab Navigation */}
      <div className="flex items-center gap-1 border-b border-vmm-border">
        {([
          { key: 'volumes' as const, label: 'Volumes', icon: Boxes },
          { key: 'disks' as const, label: 'Disks & Nodes', icon: HardDrive },
          { key: 'performance' as const, label: 'Performance', icon: Activity },
        ]).map(tab => (
          <button key={tab.key} onClick={() => setActiveTab(tab.key)}
            className={`flex items-center gap-2 px-4 py-2.5 text-sm font-medium transition-colors border-b-2 -mb-px cursor-pointer ${
              activeTab === tab.key
                ? 'border-vmm-accent text-vmm-accent'
                : 'border-transparent text-vmm-text-muted hover:text-vmm-text hover:border-vmm-border'
            }`}>
            <tab.icon size={14} /> {tab.label}
          </button>
        ))}
      </div>

      {/* ── Tab: Disks & Nodes ──────────────────────────────────── */}
      {activeTab === 'disks' && <>

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
                  <th className="text-left py-2 px-2 text-vmm-text-muted">Health</th>
                  <th className="text-left py-2 px-2 text-vmm-text-muted">Temp</th>
                  <th className="text-left py-2 px-2 text-vmm-text-muted">Hours</th>
                  <th className="text-left py-2 px-2 text-vmm-text-muted">Status</th>
                  <th className="text-right py-2 px-2 text-vmm-text-muted">Actions</th>
                </tr>
              </thead>
              <tbody>
                {disks.map(d => (
                  <tr key={diskKey(d)} className="border-b border-vmm-border/50">
                    {isCluster && <td className="py-2 px-2 text-vmm-text-dim text-xs">{d._host_name || '—'}</td>}
                    <td className="py-2 px-2 text-vmm-text font-mono flex items-center gap-2">
                      <Disc size={13} className={d.status === 'claimed' ? 'text-vmm-accent' : 'text-vmm-text-muted'} />
                      {d.path}
                    </td>
                    <td className="py-2 px-2 text-vmm-text">{formatBytes(d.size_bytes)}</td>
                    <td className="py-2 px-2 text-vmm-text-dim">{d.model || '—'}</td>
                    {/* SMART Health */}
                    <td className="py-2 px-2">
                      {!d.smart ? (
                        <span className="text-vmm-text-muted">—</span>
                      ) : !d.smart.supported ? (
                        <span className="text-vmm-warning text-[10px] font-medium cursor-help" title="This disk does not support S.M.A.R.T. — proactive failure detection unavailable">
                          No SMART
                        </span>
                      ) : d.smart.health_passed === false ? (
                        <button onClick={() => { setSmartDisk(d) }}
                          className="text-vmm-danger font-bold text-[10px] cursor-pointer hover:underline">FAILED</button>
                      ) : d.smart.reallocated_sectors && d.smart.reallocated_sectors > 0 ? (
                        <button onClick={() => { setSmartDisk(d) }}
                          className="text-vmm-warning font-medium text-[10px] cursor-pointer hover:underline">Warning</button>
                      ) : (
                        <button onClick={() => { setSmartDisk(d) }}
                          className="text-vmm-success text-[10px] cursor-pointer hover:underline">OK</button>
                      )}
                    </td>
                    {/* Temperature */}
                    <td className="py-2 px-2">
                      {d.smart?.supported && d.smart?.temperature_celsius != null ? (
                        <span className={d.smart.temperature_celsius > 55 ? 'text-vmm-warning font-medium' : d.smart.temperature_celsius > 65 ? 'text-vmm-danger font-bold' : 'text-vmm-text'}>
                          {d.smart.temperature_celsius}&deg;C
                        </span>
                      ) : <span className="text-vmm-text-muted">—</span>}
                    </td>
                    {/* Power-On Hours */}
                    <td className="py-2 px-2 text-vmm-text-dim">
                      {d.smart?.supported && d.smart?.power_on_hours != null
                        ? `${Math.round(d.smart.power_on_hours / 24)}d`
                        : '—'}
                    </td>
                    <td className="py-2 px-2">
                      <Badge label={d.status.replace('_', ' ')} color={
                        d.status === 'available' ? statusColors.online :
                        d.status === 'claimed' ? 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30' :
                        d.status === 'has_data' ? statusColors.degraded :
                        statusColors.offline
                      } />
                    </td>
                    <td className="py-2 px-2 text-right">
                      {(d.status === 'available' || d.status === 'has_data') && volumes.length > 0 && (
                        <button onClick={() => { setClaimDisk(d); setClaimConfirm(d.status === 'available'); setClaimError('') }}
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
          <div className="flex items-center gap-3 p-3 rounded-lg bg-vmm-bg/50 border border-vmm-border">
            <Server size={16} className="text-vmm-success shrink-0" />
            <div className="flex-1">
              <div className="flex items-center gap-2">
                <span className="text-sm font-medium text-vmm-text">{status?.hostname}</span>
                <Badge label="online" color={statusColors.online} />
                {status?.is_leader && <Badge label="leader" color="bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30" />}
                <span className="text-[10px] text-vmm-text-muted">(this node)</span>
              </div>
              <div className="text-xs text-vmm-text-muted font-mono">{status?.node_id?.slice(0, 12)}...</div>
            </div>
            <div className="text-right">
              <div className="text-xs text-vmm-text-muted">
                {backendsByNode[status?.node_id || '']?.length || 0} backends
              </div>
              {(status?.volumes || []).some(v => v.stale_chunks > 0) && (
                <div className="flex items-center gap-1 text-[10px] text-vmm-accent mt-0.5">
                  <RefreshCw size={10} className="animate-spin" /> resyncing
                </div>
              )}
            </div>
          </div>
          {peers.map(p => {
            const hostHealth = sanHealth?.hosts?.find((h: any) => h.hostname === p.hostname)
            const peerResyncing = hostHealth?.resyncing
            const peerIsLeader = hostHealth?.is_leader
            return (
            <div key={p.node_id} className="flex items-center gap-3 p-3 rounded-lg bg-vmm-bg/50 border border-vmm-border">
              <Server size={16} className={p.status === 'online' ? 'text-vmm-success' : 'text-vmm-danger'} />
              <div className="flex-1">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium text-vmm-text">{p.hostname}</span>
                  <Badge label={p.status} color={statusColors[p.status] || statusColors.offline} />
                  {peerIsLeader && <Badge label="leader" color="bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30" />}
                  {hostHealth?.health === 'degraded' && <Badge label="degraded" color={statusColors.degraded} />}
                </div>
                <div className="text-xs text-vmm-text-muted">{p.address}</div>
              </div>
              <div className="text-right">
                <div className="text-xs text-vmm-text-muted">
                  {backendsByNode[p.node_id]?.length || 0} backends
                </div>
                {peerResyncing && (
                  <div className="flex items-center gap-1 text-[10px] text-vmm-accent mt-0.5">
                    <RefreshCw size={10} className="animate-spin" /> {hostHealth?.stale_chunks || 0} chunks
                  </div>
                )}
                {p.last_heartbeat && (
                  <div className="text-[10px] text-vmm-text-muted mt-0.5">{new Date(p.last_heartbeat).toLocaleTimeString()}</div>
                )}
              </div>
            </div>
            )
          })}
          {peers.length === 0 && (
            <p className="text-xs text-vmm-text-muted py-2 px-3">
              Single-node mode — {isCluster ? 'add cluster hosts above for redundancy.' : 'add peers for redundancy.'}
            </p>
          )}
        </div>
      </Card>

      </>}

      {/* ── Tab: Performance ───────────────────────────────────── */}
      {activeTab === 'performance' && <>

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

      </>}

      {/* ── Tab: Volumes ───────────────────────────────────────── */}
      {activeTab === 'volumes' && <>

      {/* Volume List */}
      <div>
        <SectionLabel>Volumes</SectionLabel>
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3 mt-3">
          {volumes.length === 0 ? (
            <Card><p className="text-sm text-vmm-text-dim text-center py-4">No volumes yet</p></Card>
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
      </div>

      {/* Volume Detail */}
      <div className="space-y-4">
          {sel ? (
            <>
              <Card>
                <div className="flex items-center justify-between mb-4">
                  <div className="flex items-center gap-2">
                    <SectionLabel>{`Volume: ${sel.name}`}</SectionLabel>
                    <button onClick={openEditVolume} className="p-1 rounded hover:bg-vmm-accent/10 text-vmm-text-muted hover:text-vmm-accent transition-colors cursor-pointer" title="Edit Volume">
                      <Pencil size={13} />
                    </button>
                  </div>
                  <div className="flex items-center gap-2">
                    <Button variant="outline" onClick={() => { loadVolumeHealth(sel.id) }}>
                      <Activity size={13} /> Health
                    </Button>
                    {sel.status === 'degraded' && (
                      <Button variant="outline" onClick={handleRepair} disabled={repairRunning}>
                        <RefreshCw size={13} className={repairRunning ? 'animate-spin' : ''} />
                        {repairRunning ? 'Repairing...' : 'Repair'}
                      </Button>
                    )}
                    <Button variant="outline" onClick={() => navigate(`/storage/coresan/volume/${sel.id}/chunks`)}>
                      <Grid3x3 size={13} /> Allocation Details
                    </Button>
                    <Button variant="outline" onClick={() => setBrowseVolume(sel)}>
                      <FolderOpen size={13} /> Browse
                    </Button>
                    <Button variant="danger" onClick={() => setDeleteVolume(sel)}>
                      <Trash2 size={13} /> Delete
                    </Button>
                  </div>
                </div>
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

              {/* Repair Result */}
              {repairResult && (
                <Card>
                  <SectionLabel>Repair Result</SectionLabel>
                  <div className="mt-2 text-sm space-y-1">
                    <div className="text-vmm-success">Repaired: {repairResult.repaired} chunks</div>
                    {repairResult.remaining > 0 && (
                      <div className="text-vmm-warning">Remaining: {repairResult.remaining} under-replicated</div>
                    )}
                    {repairResult.errors?.length > 0 && (
                      <div className="text-vmm-danger">Errors: {repairResult.errors.length}</div>
                    )}
                  </div>
                </Card>
              )}

              {/* Volume Health */}
              {volumeHealth && (
                <Card>
                  <div className="flex items-center justify-between mb-3">
                    <SectionLabel>Volume Health</SectionLabel>
                    <button onClick={() => setVolumeHealth(null)} className="text-xs text-vmm-text-muted hover:text-vmm-text cursor-pointer">Close</button>
                  </div>
                  {healthLoading ? (
                    <div className="text-sm text-vmm-text-muted">Analyzing...</div>
                  ) : (
                    <div className="space-y-3">
                      <div className="flex items-center gap-3">
                        <Badge label={volumeHealth.status} color={
                          volumeHealth.status === 'healthy' ? 'bg-vmm-success/20 text-vmm-success' :
                          volumeHealth.status === 'degraded' ? 'bg-vmm-warning/20 text-vmm-warning' :
                          'bg-vmm-danger/20 text-vmm-danger'
                        } />
                      </div>
                      <div className="grid grid-cols-2 md:grid-cols-4 gap-3 text-sm">
                        <div><span className="text-vmm-text-muted">Files:</span> {volumeHealth.summary?.total_files || 0}</div>
                        <div className="text-vmm-success">Protected: {volumeHealth.summary?.protected_files || 0}</div>
                        <div className="text-vmm-warning">Degraded: {volumeHealth.summary?.degraded_files || 0}</div>
                        <div className="text-vmm-danger">Lost chunks: {volumeHealth.summary?.error_chunks || 0}</div>
                      </div>
                      {volumeHealth.affected_files?.length > 0 && (
                        <div>
                          <div className="text-xs font-semibold text-vmm-text-muted uppercase tracking-wider mb-2">Affected Files</div>
                          <div className="space-y-1 max-h-40 overflow-y-auto">
                            {volumeHealth.affected_files.map((f: any, i: number) => (
                              <div key={i} className="flex items-center justify-between text-sm p-2 rounded bg-vmm-bg">
                                <span className="truncate">{f.rel_path}</span>
                                <Badge label={f.protection_status} color={
                                  f.protection_status === 'degraded' ? 'bg-vmm-warning/20 text-vmm-warning' : 'bg-vmm-danger/20 text-vmm-danger'
                                } />
                              </div>
                            ))}
                          </div>
                        </div>
                      )}
                      {volumeHealth.integrity_failures?.length > 0 && (
                        <div>
                          <div className="text-xs font-semibold text-vmm-danger uppercase tracking-wider mb-2">Integrity Failures</div>
                          <div className="space-y-1 max-h-32 overflow-y-auto text-xs">
                            {volumeHealth.integrity_failures.map((f: any, i: number) => (
                              <div key={i} className="p-2 rounded bg-vmm-danger/5 text-vmm-danger">
                                {f.rel_path} — chunk {f.chunk_index} — {f.checked_at}
                              </div>
                            ))}
                          </div>
                        </div>
                      )}
                    </div>
                  )}
                </Card>
              )}

              {/* Backends grouped by node — collapsible */}
              <Card>
                <button
                  onClick={() => setBackendsExpanded(prev => !prev)}
                  className="flex items-center justify-between w-full cursor-pointer">
                  <div className="flex items-center gap-2">
                    <SectionLabel>Storage Backends</SectionLabel>
                    <span className="text-xs text-vmm-text-muted">({backends.length})</span>
                  </div>
                  <span className={`text-vmm-text-muted transition-transform ${backendsExpanded ? 'rotate-180' : ''}`}>
                    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round"><polyline points="6 9 12 15 18 9"/></svg>
                  </span>
                </button>
                {backendsExpanded && (
                  <div className="mt-3">
                    {backends.length === 0 ? (
                      <div className="text-center py-4">
                        <p className="text-sm text-vmm-text-dim">No backends configured.</p>
                        <p className="text-xs text-vmm-text-muted mt-1">
                          {isCluster ? 'Add cluster hosts to contribute storage to this volume.' : 'Add a local mountpoint to provide storage.'}
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
                                {Object.keys(backendsByNode).length >= 2 && (
                                  <button
                                    onClick={() => { if (confirm(`Remove host "${nodeName}" from this volume? Its backends will be drained.`)) handleRemoveHost(nodeId) }}
                                    className="p-0.5 rounded hover:bg-vmm-danger/10 text-vmm-text-muted hover:text-vmm-danger transition-colors cursor-pointer ml-auto"
                                    title="Remove Host">
                                    <X size={12} />
                                  </button>
                                )}
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
                                    <button
                                      onClick={() => setDeleteBackend(b)}
                                      className="p-1.5 rounded hover:bg-vmm-danger/10 text-vmm-text-muted hover:text-vmm-danger transition-colors cursor-pointer shrink-0"
                                      title="Remove Backend">
                                      <Trash2 size={12} />
                                    </button>
                                  </div>
                                )
                              })}
                            </div>
                          )
                        })}
                      </div>
                    )}
                    {isCluster && availableHosts.length > 0 && (
                      <div className="mt-3 pt-3 border-t border-vmm-border">
                        <Button variant="outline" size="sm" onClick={() => { setAddHostOpen(true); setAddHostId(availableHosts[0]?.id || '') }}>
                          <Plus size={13} /> Add Host
                        </Button>
                      </div>
                    )}
                  </div>
                )}
              </Card>
            </>
          ) : (
            <Card>
              <p className="text-sm text-vmm-text-dim text-center py-8">Select a volume to view its details, or create a new one.</p>
            </Card>
          )}

      </div>

      </>}

      {/* ── Tab: Performance — Benchmark Matrix ────────────────── */}
      {activeTab === 'performance' && benchmarkMatrix && benchmarkMatrix.entries.length > 0 && (
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

      {/* ── Dialogs ───────────────────────────────────────────── */}

      <CreateVolumeDialog
        open={createVolumeOpen}
        onClose={() => { setCreateVolumeOpen(false); setNewVolError('') }}
        onSubmit={handleCreateVolume}
        status={status}
        sanHosts={sanHosts}
        availableHosts={availableHosts}
        newVolName={newVolName} setNewVolName={setNewVolName}
        newVolSizeGb={newVolSizeGb} setNewVolSizeGb={setNewVolSizeGb}
        newVolFtt={newVolFtt} setNewVolFtt={setNewVolFtt}
        newVolRaid={newVolRaid} setNewVolRaid={setNewVolRaid}
        newVolSelectedHosts={newVolSelectedHosts} setNewVolSelectedHosts={setNewVolSelectedHosts}
        newVolProtocols={newVolProtocols} setNewVolProtocols={setNewVolProtocols}
        newVolError={newVolError}
        volumes={volumes}
      />

      <AddHostDialog
        open={addHostOpen}
        onClose={() => { setAddHostOpen(false); setAddHostError('') }}
        onSubmit={handleAddHost}
        availableHosts={availableHosts}
        selectedVolume={sel}
        addHostId={addHostId} setAddHostId={setAddHostId}
        addHostError={addHostError}
      />

      <ClaimDiskDialog
        disk={claimDisk}
        onClose={() => { setClaimDisk(null); setClaimError('') }}
        onSubmit={handleClaimDisk}
        claimConfirm={claimConfirm} setClaimConfirm={setClaimConfirm}
        claimError={claimError}
      />

      <ConfirmDialog
        open={!!deleteVolume}
        title="Delete Volume"
        message={deleteVolume ? (
          <div className="space-y-3">
            <p>Are you sure you want to delete volume "{deleteVolume.name}"?</p>
            <label className="flex items-center gap-2 text-sm cursor-pointer">
              <input
                type="checkbox"
                checked={forceDeleteVolume}
                onChange={e => setForceDeleteVolume(e.target.checked)}
                className="rounded"
              />
              <span className="text-vmm-danger font-medium">Force delete — remove all files and chunks</span>
            </label>
            {!forceDeleteVolume && (
              <p className="text-xs text-vmm-text-muted">Without force delete, the volume must be empty.</p>
            )}
          </div>
        ) as any : ''}
        confirmLabel={forceDeleteVolume ? 'Force Delete' : 'Delete'}
        danger
        onConfirm={handleDeleteVolume}
        onCancel={() => { setDeleteVolume(null); setForceDeleteVolume(false) }}
      />

      <ConfirmDialog
        open={!!deleteBackend}
        title="Remove Backend"
        message={deleteBackend ? `Remove backend "${deleteBackend.path}"? If it contains data, it will be drained to other backends first.` : ''}
        confirmLabel="Remove"
        danger
        onConfirm={handleDeleteBackend}
        onCancel={() => setDeleteBackend(null)}
      />

      <ConfirmDialog
        open={!!resetDisk}
        title="Reset Disk"
        message={resetDisk ? `Reset disk "${resetDisk.path}" (${resetDisk.model || 'Unknown'}, ${formatBytes(resetDisk.size_bytes)})?\n\nThis will DESTROY all data, partition tables, and filesystem signatures. The disk will become available for CoreSAN.` : ''}
        confirmLabel="Reset & Wipe"
        danger
        onConfirm={handleResetDisk}
        onCancel={() => setResetDisk(null)}
      />

      <VolumeBrowser
        open={!!browseVolume}
        onClose={() => setBrowseVolume(null)}
        volumeId={browseVolume?.id || ''}
        volumeName={browseVolume?.name || ''}
        sanApi={sanApi}
        sanFetch={sanFetch}
      />

      <AutoClaimDialog
        open={autoClaimOpen}
        onClose={() => setAutoClaimOpen(false)}
        onSubmit={handleAutoClaim}
        disks={disks}
        diskKey={diskKey}
        status={status}
        autoClaimSelected={autoClaimSelected} setAutoClaimSelected={setAutoClaimSelected}
        autoClaimRunning={autoClaimRunning}
        autoClaimError={autoClaimError}
      />

      {/* SAN Events */}
      <EventFeed category="san" limit={10} title="CoreSAN Events" />
      <EventFeed category="disk" limit={5} title="Disk Events" />

      {/* SMART Detail Dialog */}
      <SmartDetailDialog
        open={!!smartDisk}
        onClose={() => setSmartDisk(null)}
        deviceName={smartDisk?.name || ''}
        hostId={smartDisk?._host_id}
        hostName={smartDisk?._host_name}
        sanAddress={smartDisk?._san_address}
      />

      {/* Edit Volume Dialog */}
      {editVolumeOpen && sel && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onClick={() => setEditVolumeOpen(false)}>
          <div className="bg-vmm-card border border-vmm-border rounded-xl shadow-xl w-full max-w-md p-6" onClick={e => e.stopPropagation()}>
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-base font-bold text-vmm-text">Edit Volume: {sel.name}</h3>
              <button onClick={() => setEditVolumeOpen(false)} className="p-1 rounded hover:bg-vmm-bg text-vmm-text-muted cursor-pointer">
                <X size={16} />
              </button>
            </div>
            <div className="space-y-4">
              <div>
                <label className="block text-xs font-medium text-vmm-text-muted mb-1">Failure Tolerance (FTT)</label>
                <select
                  value={editFtt}
                  onChange={e => setEditFtt(Number(e.target.value))}
                  className="w-full px-3 py-2 text-sm rounded-lg bg-vmm-bg border border-vmm-border text-vmm-text focus:outline-none focus:ring-1 focus:ring-vmm-accent">
                  <option value={0}>FTT=0 — No redundancy</option>
                  <option value={1}>FTT=1 — Tolerate 1 failure</option>
                  <option value={2}>FTT=2 — Tolerate 2 failures</option>
                </select>
              </div>
              <div>
                <label className="block text-xs font-medium text-vmm-text-muted mb-1">RAID Mode</label>
                <select
                  value={editRaid}
                  onChange={e => setEditRaid(e.target.value)}
                  className="w-full px-3 py-2 text-sm rounded-lg bg-vmm-bg border border-vmm-border text-vmm-text focus:outline-none focus:ring-1 focus:ring-vmm-accent">
                  <option value="stripe">Stripe — Distribute across disks</option>
                  <option value="mirror">Mirror — Mirror across disks</option>
                  <option value="stripe_mirror">Stripe+Mirror — Striped mirrors</option>
                </select>
              </div>
              {editError && (
                <div className="text-sm text-vmm-danger bg-vmm-danger/10 border border-vmm-danger/30 rounded-lg px-3 py-2">{editError}</div>
              )}
              <div className="flex justify-end gap-2 pt-2">
                <Button variant="ghost" onClick={() => setEditVolumeOpen(false)}>Cancel</Button>
                <Button variant="primary" onClick={handleEditVolume} disabled={editSaving}>
                  {editSaving ? 'Saving...' : 'Save Changes'}
                </Button>
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
