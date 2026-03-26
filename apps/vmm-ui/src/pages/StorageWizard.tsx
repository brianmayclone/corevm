import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { HardDrive, Server, Database, ArrowRight, ArrowLeft, Check, X, Circle, Loader, ChevronDown, ChevronUp, Boxes, Star } from 'lucide-react'
import api from '../api/client'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'
import Button from '../components/Button'

interface HostPkgStatus {
  host_id: string; hostname: string; installed: string[]; missing: string[]; distro: string; is_root: boolean
}
interface WizardStep {
  label: string; status: string; error?: string
}

type FsType = 'coresan' | 'nfs' | 'glusterfs' | 'cephfs' | null

export default function StorageWizard() {
  const navigate = useNavigate()
  const { clusters, fetchClusters, hosts, fetchHosts } = useClusterStore()
  const [step, setStep] = useState(1)
  const [fsType, setFsType] = useState<FsType>(null)
  const [clusterId, setClusterId] = useState('')
  const [expert, setExpert] = useState(false)

  // Step 2
  const [pkgStatus, setPkgStatus] = useState<HostPkgStatus[]>([])
  const [pkgLoading, setPkgLoading] = useState(false)
  const [installLoading, setInstallLoading] = useState(false)
  const [sudoPasswords, setSudoPasswords] = useState<Record<string, string>>({})

  // Step 3 — config
  const [datastoreName, setDatastoreName] = useState('cluster-storage')
  const [mountPath, setMountPath] = useState('')
  const [selectedHostIds, setSelectedHostIds] = useState<string[]>([])
  // NFS
  const [nfsMode, setNfsMode] = useState<'create' | 'existing'>('create') // create = setup NFS server on a host
  const [nfsServerHostId, setNfsServerHostId] = useState('') // host that becomes the NFS server
  const [nfsServer, setNfsServer] = useState('') // external server address (existing mode)
  const [nfsExport, setNfsExport] = useState('/vmm/nfs-export')
  const [nfsOpts, setNfsOpts] = useState('vers=4,noatime')
  // CephFS
  const [cephMode, setCephMode] = useState<'create' | 'existing'>('create')
  // GlusterFS
  const [glusterVolume, setGlusterVolume] = useState('')
  const [glusterBrick, setGlusterBrick] = useState('')
  const [glusterReplica, setGlusterReplica] = useState(2)
  // CephFS
  const [cephMonitors, setCephMonitors] = useState('')
  const [cephPath, setCephPath] = useState('/')
  const [cephSecret, setCephSecret] = useState('')
  // CoreSAN
  const [coresanResilience, setCoresanResilience] = useState('mirror')
  const [coresanReplicas, setCoresanReplicas] = useState(2)
  const [coresanBackendPath, setCoresanBackendPath] = useState('/vmm/san-data')

  // Step 4
  const [setupSteps, setSetupSteps] = useState<WizardStep[]>([])
  const [setupRunning, setSetupRunning] = useState(false)
  const [setupError, setSetupError] = useState('')

  useEffect(() => { fetchClusters(); fetchHosts() }, [])
  useEffect(() => { if (clusters.length > 0 && !clusterId) setClusterId(clusters[0].id) }, [clusters])

  // Auto-set defaults when fsType changes
  useEffect(() => {
    if (!fsType) return
    const name = datastoreName || 'cluster-storage'
    setMountPath(`/vmm/datastores/${name}`)
    setGlusterVolume(name.replace(/[^a-zA-Z0-9-]/g, '-'))
    setGlusterBrick(`/data/gluster/${name}`)
    const clusterHosts = hosts.filter(h => h.cluster_id === clusterId && h.status === 'online')
    setSelectedHostIds(clusterHosts.map(h => h.id))
    setGlusterReplica(Math.min(3, clusterHosts.length))
  }, [fsType, clusterId])

  const clusterHosts = hosts.filter(h => h.cluster_id === clusterId && h.status === 'online')
  const allPkgsInstalled = pkgStatus.length > 0 && pkgStatus.every(h => h.missing.length === 0)

  // Step 2: Check packages
  const checkPackages = async () => {
    setPkgLoading(true)
    try {
      const { data } = await api.post('/api/storage/wizard/check', { cluster_id: clusterId, fs_type: fsType })
      setPkgStatus(data)
    } finally { setPkgLoading(false) }
  }

  const installPkgs = async () => {
    const hostsWithMissing = pkgStatus.filter(h => h.missing.length > 0).map(h => h.host_id)
    if (hostsWithMissing.length === 0) return
    // Check if non-root hosts have sudo passwords
    const needsSudo = pkgStatus.filter(h => !h.is_root && h.missing.length > 0 && !sudoPasswords[h.host_id])
    if (needsSudo.length > 0) {
      alert(`Please enter sudo passwords for: ${needsSudo.map(h => h.hostname).join(', ')}`)
      return
    }
    setInstallLoading(true)
    try {
      await api.post('/api/storage/wizard/install', { host_ids: hostsWithMissing, fs_type: fsType, sudo_passwords: sudoPasswords })
      await checkPackages()
    } catch (e: any) {
      alert(e.response?.data?.error || 'Installation failed')
    } finally { setInstallLoading(false) }
  }

  // Step 4: Run setup
  const runSetup = async () => {
    setSetupRunning(true); setSetupError('')
    try {
      // For NFS create mode: use the selected host's IP as NFS server
      let effectiveNfsServer = nfsServer
      if (fsType === 'nfs' && nfsMode === 'create' && nfsServerHostId) {
        const serverHost = clusterHosts.find(h => h.id === nfsServerHostId)
        if (serverHost) {
          effectiveNfsServer = serverHost.address.replace('https://', '').replace('http://', '').split(':')[0]
        }
      }

      const config = {
        fs_type: fsType, cluster_id: clusterId, datastore_name: datastoreName,
        host_ids: selectedHostIds, mount_path: mountPath,
        nfs_server: effectiveNfsServer || undefined, nfs_export: nfsExport || undefined, nfs_opts: nfsOpts || undefined,
        nfs_server_host_id: (fsType === 'nfs' && nfsMode === 'create') ? nfsServerHostId : undefined,
        gluster_volume: glusterVolume || undefined, gluster_brick_path: glusterBrick || undefined, gluster_replica: glusterReplica,
        ceph_monitors: cephMonitors || undefined, ceph_path: cephPath || undefined, ceph_secret: cephSecret || undefined,
        ceph_create_new: fsType === 'cephfs' && cephMode === 'create',
        coresan_volume_name: fsType === 'coresan' ? datastoreName : undefined,
        coresan_resilience_mode: fsType === 'coresan' ? coresanResilience : undefined,
        coresan_replica_count: fsType === 'coresan' ? coresanReplicas : undefined,
        coresan_backend_paths: fsType === 'coresan' ? [coresanBackendPath] : undefined,
        sudo_passwords: sudoPasswords,
      }
      const { data } = await api.post('/api/storage/wizard/setup', config)
      setSetupSteps(data.steps || [])
      setStep(5)
    } catch (e: any) {
      setSetupError(e.response?.data?.error || 'Setup failed')
    } finally { setSetupRunning(false) }
  }

  return (
    <div className="space-y-5 max-w-3xl mx-auto">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">Create Cluster Storage</h1>
        <p className="text-sm text-vmm-text-muted mt-1">Guided setup for shared cluster filesystems</p>
      </div>

      {/* Progress bar */}
      <div className="flex items-center gap-2">
        {[1,2,3,4,5].map(s => (
          <div key={s} className={`flex-1 h-1.5 rounded-full ${s <= step ? 'bg-vmm-accent' : 'bg-vmm-border'}`} />
        ))}
      </div>

      {/* ── Step 1: Choose Type ──────────────────────────────────── */}
      {step === 1 && (
        <div className="space-y-4">
          <h2 className="text-lg font-semibold text-vmm-text">Choose Storage Type</h2>

          {/* CoreSAN — featured recommendation */}
          <Card>
            <div onClick={() => setFsType('coresan')}
              className={`p-6 cursor-pointer rounded-xl transition-colors relative ${fsType === 'coresan' ? 'ring-2 ring-vmm-accent bg-vmm-accent/5' : 'hover:bg-vmm-surface-hover'}`}>
              <span className="absolute top-3 right-3 inline-flex items-center gap-1 px-2.5 py-1 rounded text-[10px] font-bold bg-vmm-success/20 text-vmm-success border border-vmm-success/30">
                <Star size={10} /> RECOMMENDED
              </span>
              <div className="flex items-center gap-5">
                <div className="w-14 h-14 rounded-2xl bg-vmm-accent/10 flex items-center justify-center shrink-0">
                  <Boxes size={28} className={fsType === 'coresan' ? 'text-vmm-accent' : 'text-vmm-success'} />
                </div>
                <div>
                  <h3 className="text-base font-bold text-vmm-text">CoreSAN — Software-Defined Storage</h3>
                  <p className="text-sm text-vmm-text-dim mt-1">
                    Built-in distributed storage with per-volume RAID levels, automatic replication,
                    self-healing, write ownership, and network benchmarking. Works with 1 or more nodes.
                    No external software required.
                  </p>
                  <div className="flex items-center gap-4 mt-2.5 text-xs text-vmm-text-muted">
                    <span className="flex items-center gap-1"><Check size={12} className="text-vmm-success" /> RAID-0/1 per volume</span>
                    <span className="flex items-center gap-1"><Check size={12} className="text-vmm-success" /> Auto-replication</span>
                    <span className="flex items-center gap-1"><Check size={12} className="text-vmm-success" /> Self-healing</span>
                    <span className="flex items-center gap-1"><Check size={12} className="text-vmm-success" /> FUSE-based</span>
                  </div>
                </div>
              </div>
            </div>
          </Card>

          {/* Separator */}
          <div className="flex items-center gap-3 pt-1">
            <div className="flex-1 h-px bg-vmm-border" />
            <span className="text-[10px] font-semibold tracking-widest text-vmm-text-muted uppercase">Other Options</span>
            <div className="flex-1 h-px bg-vmm-border" />
          </div>

          {/* Other filesystem options */}
          <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
            {([
              { id: 'nfs', icon: Server, title: 'NFS', desc: 'Sets up one host as NFS server and mounts the share on all others. Simple and reliable.' },
              { id: 'glusterfs', icon: Database, title: 'GlusterFS', desc: 'Installs GlusterFS on all hosts and creates a replicated volume. No external server needed.' },
              { id: 'cephfs', icon: HardDrive, title: 'CephFS', desc: 'Installs Ceph across your hosts with CephFS. Maximum scalability and fault tolerance.' },
            ] as const).map(opt => (
              <Card key={opt.id}>
                <div onClick={() => setFsType(opt.id)}
                  className={`p-4 cursor-pointer rounded-xl transition-colors text-center ${fsType === opt.id ? 'ring-2 ring-vmm-accent bg-vmm-accent/5' : 'hover:bg-vmm-surface-hover'}`}>
                  <opt.icon size={24} className={`mx-auto mb-2 ${fsType === opt.id ? 'text-vmm-accent' : 'text-vmm-text-muted'}`} />
                  <h3 className="text-sm font-semibold text-vmm-text">{opt.title}</h3>
                  <p className="text-xs text-vmm-text-muted mt-1.5">{opt.desc}</p>
                </div>
              </Card>
            ))}
          </div>
          {clusters.length > 1 && (
            <div>
              <label className="block text-xs text-vmm-text-muted mb-1">Cluster</label>
              <select value={clusterId} onChange={e => setClusterId(e.target.value)}
                className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                {clusters.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
              </select>
            </div>
          )}
          <div className="flex justify-end">
            <Button variant="primary" disabled={!fsType} onClick={() => { setStep(2); checkPackages() }}>
              Continue <ArrowRight size={14} />
            </Button>
          </div>
        </div>
      )}

      {/* ── Step 2: Host Preparation ─────────────────────────────── */}
      {step === 2 && (
        <div className="space-y-4">
          <h2 className="text-lg font-semibold text-vmm-text">Prepare Hosts</h2>
          <p className="text-sm text-vmm-text-muted">Checking required packages on {clusterHosts.length} hosts...</p>

          <Card>
            <div className="divide-y divide-vmm-border">
              {pkgLoading ? (
                <div className="p-6 text-center text-vmm-text-muted"><Loader size={20} className="inline animate-spin mr-2" /> Checking packages...</div>
              ) : pkgStatus.map(h => (
                <div key={h.host_id} className="px-4 py-3 space-y-2">
                  <div className="flex items-center gap-3">
                    <Circle size={8} className={h.missing.length === 0 ? 'fill-vmm-success text-vmm-success' : 'fill-vmm-danger text-vmm-danger'} />
                    <div className="flex-1">
                      <span className="text-sm font-medium text-vmm-text">{h.hostname}</span>
                      <span className="text-xs text-vmm-text-muted ml-2">({h.distro})</span>
                      {h.is_root ? (
                        <span className="ml-2 text-[10px] px-1.5 py-0.5 rounded bg-vmm-success/10 text-vmm-success">root</span>
                      ) : (
                        <span className="ml-2 text-[10px] px-1.5 py-0.5 rounded bg-yellow-500/10 text-yellow-400">non-root</span>
                      )}
                    </div>
                    {h.missing.length > 0 ? (
                      <span className="text-xs text-vmm-danger">Missing: {h.missing.join(', ')}</span>
                    ) : (
                      <span className="text-xs text-vmm-success">Ready</span>
                    )}
                  </div>
                  {/* Show sudo password field if agent is not root and has missing packages */}
                  {!h.is_root && h.missing.length > 0 && (
                    <div className="ml-8 flex items-center gap-2">
                      <label className="text-xs text-vmm-text-muted whitespace-nowrap">sudo password:</label>
                      <input type="password" placeholder="Enter sudo password for this host"
                        value={sudoPasswords[h.host_id] || ''}
                        onChange={e => setSudoPasswords({ ...sudoPasswords, [h.host_id]: e.target.value })}
                        className="flex-1 px-2 py-1 bg-vmm-bg border border-vmm-border rounded text-vmm-text text-xs" />
                    </div>
                  )}
                </div>
              ))}
            </div>
          </Card>

          {!allPkgsInstalled && pkgStatus.length > 0 && (
            <Button variant="primary" onClick={installPkgs} disabled={installLoading}>
              {installLoading ? <><Loader size={14} className="animate-spin" /> Installing...</> : 'Prepare All Hosts'}
            </Button>
          )}

          <div className="flex justify-between">
            <Button variant="ghost" onClick={() => setStep(1)}><ArrowLeft size={14} /> Back</Button>
            <Button variant="primary" disabled={!allPkgsInstalled} onClick={() => setStep(3)}>
              Continue <ArrowRight size={14} />
            </Button>
          </div>
        </div>
      )}

      {/* ── Step 3: Configuration ────────────────────────────────── */}
      {step === 3 && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <h2 className="text-lg font-semibold text-vmm-text">Configuration</h2>
            <button onClick={() => setExpert(!expert)} className="text-xs text-vmm-accent flex items-center gap-1">
              {expert ? <ChevronUp size={12} /> : <ChevronDown size={12} />}
              {expert ? 'Hide Advanced' : 'Show Advanced'}
            </button>
          </div>

          <Card>
            <div className="p-5 space-y-4">
              {/* Common */}
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Datastore Name</label>
                <input type="text" value={datastoreName} onChange={e => {
                  setDatastoreName(e.target.value)
                  setMountPath(`/vmm/datastores/${e.target.value}`)
                  setGlusterVolume(e.target.value.replace(/[^a-zA-Z0-9-]/g, '-'))
                  setGlusterBrick(`/data/gluster/${e.target.value}`)
                }} className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              </div>

              {/* CoreSAN */}
              {fsType === 'coresan' && (
                <div className="space-y-4">
                  <div className="bg-vmm-success/5 border border-vmm-success/20 rounded-lg p-4 space-y-2">
                    <div className="text-sm font-medium text-vmm-text">CoreSAN — Software-Defined Storage</div>
                    <div className="text-xs text-vmm-text-muted">
                      CoreSAN will be installed and configured on <strong>{selectedHostIds.length} host{selectedHostIds.length !== 1 ? 's' : ''}</strong>.
                      Each host runs its own CoreSAN daemon and contributes local storage to a shared volume.
                      Data is replicated automatically between nodes based on the resilience policy.
                    </div>
                  </div>
                  <div className="grid grid-cols-2 gap-4">
                    <div>
                      <label className="block text-xs text-vmm-text-muted mb-1">Resilience Mode</label>
                      <select value={coresanResilience} onChange={e => {
                        setCoresanResilience(e.target.value)
                        if (e.target.value === 'none') setCoresanReplicas(1)
                        else if (coresanReplicas < 2) setCoresanReplicas(2)
                      }}
                        className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                        <option value="none">No Protection (RAID-0) — maximum space</option>
                        <option value="mirror">Mirror (RAID-1) — maximum safety</option>
                      </select>
                    </div>
                    {coresanResilience === 'mirror' && (
                      <div>
                        <label className="block text-xs text-vmm-text-muted mb-1">Replica Count</label>
                        <select value={String(coresanReplicas)} onChange={e => setCoresanReplicas(Number(e.target.value))}
                          className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                          <option value="2">2 copies — tolerates 1 node failure</option>
                          <option value="3">3 copies — tolerates 2 node failures</option>
                        </select>
                      </div>
                    )}
                  </div>
                  <div>
                    <label className="block text-xs text-vmm-text-muted mb-1">Backend Storage Path (per host)</label>
                    <input type="text" value={coresanBackendPath} onChange={e => setCoresanBackendPath(e.target.value)}
                      className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                    <p className="text-[10px] text-vmm-text-muted mt-1">Local directory on each host that provides storage to the volume. Must be a mounted filesystem (ext4, xfs, etc.).</p>
                  </div>
                </div>
              )}

              {/* NFS */}
              {fsType === 'nfs' && (
                <div className="space-y-4">
                  <div className="flex gap-3">
                    <button onClick={() => setNfsMode('create')}
                      className={`flex-1 px-4 py-3 rounded-lg border text-sm text-left ${nfsMode === 'create' ? 'border-vmm-accent bg-vmm-accent/5' : 'border-vmm-border hover:border-vmm-border-light'}`}>
                      <div className="font-medium text-vmm-text">Set up a new NFS server</div>
                      <div className="text-xs text-vmm-text-muted mt-0.5">One host becomes the NFS server, others mount from it</div>
                    </button>
                    <button onClick={() => setNfsMode('existing')}
                      className={`flex-1 px-4 py-3 rounded-lg border text-sm text-left ${nfsMode === 'existing' ? 'border-vmm-accent bg-vmm-accent/5' : 'border-vmm-border hover:border-vmm-border-light'}`}>
                      <div className="font-medium text-vmm-text">Use existing NFS server</div>
                      <div className="text-xs text-vmm-text-muted mt-0.5">Connect to a server that's already set up</div>
                    </button>
                  </div>
                  {nfsMode === 'create' ? (
                    <div className="grid grid-cols-2 gap-4">
                      <div>
                        <label className="block text-xs text-vmm-text-muted mb-1">NFS Server Host</label>
                        <select value={nfsServerHostId} onChange={e => setNfsServerHostId(e.target.value)}
                          className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                          <option value="">Select a host...</option>
                          {clusterHosts.map(h => <option key={h.id} value={h.id}>{h.hostname}</option>)}
                        </select>
                        <p className="text-[10px] text-vmm-text-muted mt-1">This host will run the NFS server. nfs-kernel-server will be installed automatically.</p>
                      </div>
                      <div>
                        <label className="block text-xs text-vmm-text-muted mb-1">Export Directory</label>
                        <input type="text" value={nfsExport} onChange={e => setNfsExport(e.target.value)}
                          className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                      </div>
                    </div>
                  ) : (
                    <div className="grid grid-cols-2 gap-4">
                      <div>
                        <label className="block text-xs text-vmm-text-muted mb-1">NFS Server Address</label>
                        <input type="text" value={nfsServer} onChange={e => setNfsServer(e.target.value)}
                          placeholder="192.168.1.100" className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                      </div>
                      <div>
                        <label className="block text-xs text-vmm-text-muted mb-1">Export Path</label>
                        <input type="text" value={nfsExport} onChange={e => setNfsExport(e.target.value)}
                          className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                      </div>
                    </div>
                  )}
                </div>
              )}

              {/* GlusterFS — Easy mode: just confirm, everything auto-configured */}
              {fsType === 'glusterfs' && (
                <div className="bg-vmm-accent/5 border border-vmm-accent/20 rounded-lg p-4 space-y-2">
                  <div className="text-sm font-medium text-vmm-text">Ready to create replicated storage</div>
                  <div className="text-xs text-vmm-text-muted">
                    A GlusterFS volume will be created across <strong>{selectedHostIds.length} hosts</strong> with
                    <strong> {glusterReplica}-way replication</strong>. Every file is stored on {glusterReplica} hosts simultaneously —
                    if a host fails, your data remains available.
                  </div>
                  <div className="text-xs text-vmm-text-muted mt-1">
                    No external storage server needed — your cluster hosts provide the storage.
                  </div>
                </div>
              )}

              {/* CephFS */}
              {fsType === 'cephfs' && (
                <div className="space-y-4">
                  <div className="flex gap-3">
                    <button onClick={() => setCephMode('create')}
                      className={`flex-1 px-4 py-3 rounded-lg border text-sm text-left ${cephMode === 'create' ? 'border-vmm-accent bg-vmm-accent/5' : 'border-vmm-border hover:border-vmm-border-light'}`}>
                      <div className="font-medium text-vmm-text">Set up new Ceph cluster</div>
                      <div className="text-xs text-vmm-text-muted mt-0.5">Install and configure Ceph on your hosts from scratch</div>
                    </button>
                    <button onClick={() => setCephMode('existing')}
                      className={`flex-1 px-4 py-3 rounded-lg border text-sm text-left ${cephMode === 'existing' ? 'border-vmm-accent bg-vmm-accent/5' : 'border-vmm-border hover:border-vmm-border-light'}`}>
                      <div className="font-medium text-vmm-text">Connect to existing Ceph</div>
                      <div className="text-xs text-vmm-text-muted mt-0.5">Use monitors and credentials from an existing cluster</div>
                    </button>
                  </div>
                  {cephMode === 'create' ? (
                    <div className="bg-vmm-accent/5 border border-vmm-accent/20 rounded-lg p-4 space-y-2">
                      <div className="text-sm font-medium text-vmm-text">Ceph will be installed on all selected hosts</div>
                      <div className="text-xs text-vmm-text-muted">
                        A Ceph monitor + OSD will be set up on each host. CephFS will be created automatically.
                        Requires at least 3 hosts for a production deployment. All packages are installed automatically.
                      </div>
                    </div>
                  ) : (
                    <div className="grid grid-cols-2 gap-4">
                      <div>
                        <label className="block text-xs text-vmm-text-muted mb-1">Monitor Addresses</label>
                        <input type="text" value={cephMonitors} onChange={e => setCephMonitors(e.target.value)}
                          placeholder="mon1,mon2,mon3" className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                      </div>
                      <div>
                        <label className="block text-xs text-vmm-text-muted mb-1">Ceph Secret / Keyring</label>
                        <input type="password" value={cephSecret} onChange={e => setCephSecret(e.target.value)}
                          className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                      </div>
                    </div>
                  )}
                </div>
              )}

              {/* Expert mode */}
              {expert && (
                <div className="border-t border-vmm-border pt-4 space-y-3">
                  <h4 className="text-xs font-semibold text-vmm-text-muted uppercase">Advanced Settings</h4>
                  <div className="grid grid-cols-2 gap-3">
                    <div>
                      <label className="block text-xs text-vmm-text-muted mb-1">Mount Path</label>
                      <input type="text" value={mountPath} onChange={e => setMountPath(e.target.value)}
                        className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                    </div>
                    {fsType === 'nfs' && (
                      <div>
                        <label className="block text-xs text-vmm-text-muted mb-1">Mount Options</label>
                        <input type="text" value={nfsOpts} onChange={e => setNfsOpts(e.target.value)}
                          className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                      </div>
                    )}
                    {fsType === 'glusterfs' && (
                      <>
                        <div>
                          <label className="block text-xs text-vmm-text-muted mb-1">Volume Name</label>
                          <input type="text" value={glusterVolume} onChange={e => setGlusterVolume(e.target.value)}
                            className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                        </div>
                        <div>
                          <label className="block text-xs text-vmm-text-muted mb-1">Brick Path</label>
                          <input type="text" value={glusterBrick} onChange={e => setGlusterBrick(e.target.value)}
                            className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                        </div>
                        <div>
                          <label className="block text-xs text-vmm-text-muted mb-1">Replica Count</label>
                          <select value={glusterReplica} onChange={e => setGlusterReplica(parseInt(e.target.value))}
                            className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                            <option value={2}>2 (2 copies)</option>
                            <option value={3}>3 (3 copies)</option>
                          </select>
                        </div>
                      </>
                    )}
                    {fsType === 'cephfs' && (
                      <>
                        <div>
                          <label className="block text-xs text-vmm-text-muted mb-1">Ceph Path</label>
                          <input type="text" value={cephPath} onChange={e => setCephPath(e.target.value)}
                            className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                        </div>
                        <div>
                          <label className="block text-xs text-vmm-text-muted mb-1">Ceph Secret</label>
                          <input type="password" value={cephSecret} onChange={e => setCephSecret(e.target.value)}
                            className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                        </div>
                      </>
                    )}
                  </div>

                  {/* Host selection (expert only for GlusterFS) */}
                  {fsType === 'glusterfs' && (
                    <div>
                      <label className="block text-xs text-vmm-text-muted mb-1">Hosts (select {glusterReplica}+)</label>
                      <div className="space-y-1">
                        {clusterHosts.map(h => (
                          <label key={h.id} className="flex items-center gap-2 text-sm text-vmm-text cursor-pointer">
                            <input type="checkbox" checked={selectedHostIds.includes(h.id)}
                              onChange={e => {
                                if (e.target.checked) setSelectedHostIds([...selectedHostIds, h.id])
                                else setSelectedHostIds(selectedHostIds.filter(id => id !== h.id))
                              }} />
                            {h.hostname}
                          </label>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              )}
            </div>
          </Card>

          <div className="flex justify-between">
            <Button variant="ghost" onClick={() => setStep(2)}><ArrowLeft size={14} /> Back</Button>
            <Button variant="primary" onClick={() => { setStep(4); runSetup() }}>
              Create Storage <ArrowRight size={14} />
            </Button>
          </div>
        </div>
      )}

      {/* ── Step 4: Execution ────────────────────────────────────── */}
      {step === 4 && (
        <div className="space-y-4">
          <h2 className="text-lg font-semibold text-vmm-text">Setting Up Storage</h2>
          {setupRunning && (
            <div className="flex items-center gap-2 text-vmm-accent text-sm">
              <Loader size={16} className="animate-spin" /> Working...
            </div>
          )}
          {setupError && (
            <div className="bg-vmm-danger/10 border border-vmm-danger/30 rounded-lg p-3 text-sm text-vmm-danger">{setupError}</div>
          )}
          <Card>
            <div className="divide-y divide-vmm-border">
              {setupSteps.map((s, i) => (
                <div key={i} className="px-4 py-3 flex items-center gap-3">
                  {s.status === 'done' && <Check size={16} className="text-vmm-success" />}
                  {s.status === 'error' && <X size={16} className="text-vmm-danger" />}
                  {s.status === 'running' && <Loader size={16} className="text-vmm-accent animate-spin" />}
                  {s.status === 'pending' && <Circle size={16} className="text-vmm-text-muted" />}
                  <span className="text-sm text-vmm-text">{s.label}</span>
                  {s.error && <span className="text-xs text-vmm-danger ml-auto">{s.error}</span>}
                </div>
              ))}
            </div>
          </Card>
        </div>
      )}

      {/* ── Step 5: Done ─────────────────────────────────────────── */}
      {step === 5 && (
        <div className="text-center py-8 space-y-4">
          <div className="w-16 h-16 mx-auto rounded-full bg-vmm-success/20 flex items-center justify-center">
            <Check size={32} className="text-vmm-success" />
          </div>
          <h2 className="text-xl font-bold text-vmm-text">Cluster Storage Created</h2>
          <p className="text-sm text-vmm-text-muted">
            Datastore "{datastoreName}" ({fsType?.toUpperCase()}) is ready and mounted on {selectedHostIds.length} hosts.
          </p>
          <div className="flex justify-center gap-3 pt-4">
            <Button variant="ghost" onClick={() => navigate('/storage/overview')}>Go to Storage</Button>
            <Button variant="primary" onClick={() => { setStep(1); setFsType(null) }}>Create Another</Button>
          </div>
        </div>
      )}
    </div>
  )
}
