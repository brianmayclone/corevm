import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { ArrowLeft, Server, Plus, Loader, Wifi, Boxes, Workflow } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import api from '../api/client'
import type { DiscoveredNode } from '../api/types'
import Card from '../components/Card'

export default function AddHost() {
  const navigate = useNavigate()
  const { registerHost, clusters, fetchClusters } = useClusterStore()
  const [address, setAddress] = useState('')
  const [clusterId, setClusterId] = useState('')
  const [adminUser, setAdminUser] = useState('admin')
  const [adminPass, setAdminPass] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [probeStatus, setProbeStatus] = useState('')
  const [results, setResults] = useState<{ address: string; ok: boolean; error?: string }[]>([])

  // Cluster creation (when no clusters exist)
  const [newClusterName, setNewClusterName] = useState('Default Cluster')
  const [creatingCluster, setCreatingCluster] = useState(false)

  // Auto-discovered nodes
  const [discovered, setDiscovered] = useState<DiscoveredNode[]>([])

  useEffect(() => {
    fetchClusters()
    // Poll for discovered nodes
    const load = () => {
      api.get<DiscoveredNode[]>('/api/discovery/servers')
        .then(({ data }) => setDiscovered(data))
        .catch(() => {})
    }
    load()
    const timer = setInterval(load, 5000)
    return () => clearInterval(timer)
  }, [])

  useEffect(() => {
    if (clusters.length > 0 && !clusterId) setClusterId(clusters[0].id)
  }, [clusters])

  const resolveAddress = async (input: string): Promise<string> => {
    if (input.startsWith('http://') || input.startsWith('https://')) {
      return input.replace(/\/+$/, '')
    }
    const host = input.replace(/[:/].*$/, '').trim()
    if (!host) throw new Error('Please enter an IP address or hostname')

    const candidates = [
      `https://${host}:8443`,
      `http://${host}:8443`,
    ]
    for (const url of candidates) {
      setProbeStatus(`Trying ${url}...`)
      try {
        const controller = new AbortController()
        const timeout = setTimeout(() => controller.abort(), 3000)
        const resp = await fetch(`${url}/api/system/info`, { signal: controller.signal }).catch(() => null)
        clearTimeout(timeout)
        if (resp && resp.ok) {
          setProbeStatus(`Connected via ${url}`)
          return url
        }
      } catch { /* next */ }
    }
    throw new Error(`Cannot reach vmm-server at ${host}:8443 (tried https and http)`)
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    setLoading(true)
    setProbeStatus('')
    setResults([])

    // Split by comma, trim, filter empty
    const addresses = address.split(',').map(a => a.trim()).filter(Boolean)
    if (addresses.length === 0) {
      setError('Please enter at least one address')
      setLoading(false)
      return
    }

    const registerResults: typeof results = []

    for (let i = 0; i < addresses.length; i++) {
      const addr = addresses[i]
      const progress = addresses.length > 1 ? ` (${i + 1}/${addresses.length})` : ''
      setProbeStatus(`Resolving ${addr}...${progress}`)
      try {
        const resolvedAddress = await resolveAddress(addr)
        setProbeStatus(`Registering ${addr}...${progress}`)
        await registerHost(resolvedAddress, clusterId, adminUser, adminPass)
        registerResults.push({ address: addr, ok: true })
      } catch (err: any) {
        const msg = err.response?.data?.error || err.message || 'Registration failed'
        registerResults.push({ address: addr, ok: false, error: msg })
      }
    }

    setResults(registerResults)
    setProbeStatus('')
    setLoading(false)

    const allOk = registerResults.every(r => r.ok)
    const anyFailed = registerResults.some(r => !r.ok)

    if (allOk) {
      navigate('/cluster/hosts')
    } else if (anyFailed) {
      const failures = registerResults.filter(r => !r.ok)
      setError(failures.map(f => `${f.address}: ${f.error}`).join('\n'))
    }
  }

  const selectDiscovered = (node: DiscoveredNode) => {
    const current = address.split(',').map(a => a.trim()).filter(Boolean)
    const idx = current.indexOf(node.address)
    if (idx >= 0) {
      current.splice(idx, 1)
    } else {
      current.push(node.address)
    }
    setAddress(current.join(', '))
  }

  return (
    <div className="space-y-5 max-w-2xl">
      <div className="flex items-center gap-3">
        <button onClick={() => navigate('/cluster/hosts')} className="text-vmm-text-muted hover:text-vmm-text cursor-pointer">
          <ArrowLeft size={20} />
        </button>
        <h1 className="text-2xl font-bold text-vmm-text">Add Host to Cluster</h1>
      </div>

      {/* Auto-discovered nodes */}
      {discovered.length > 0 && (
        <Card>
          <div className="flex items-center gap-2 mb-3">
            <Wifi size={14} className="text-vmm-success" />
            <span className="text-xs font-semibold tracking-widest text-vmm-text-muted uppercase">
              Discovered on Network
            </span>
            <span className="text-[10px] text-vmm-success bg-vmm-success/10 px-2 py-0.5 rounded-full font-bold">
              {discovered.length} found
            </span>
          </div>
          <div className="space-y-2">
            {discovered.map(node => (
              <button
                key={node.address}
                onClick={() => selectDiscovered(node)}
                className={`w-full flex items-center gap-3 p-3 rounded-lg border text-left transition-colors cursor-pointer
                  ${address.split(',').map(a => a.trim()).includes(node.address)
                    ? 'border-vmm-accent bg-vmm-accent/5'
                    : 'border-vmm-border hover:border-vmm-accent/30 hover:bg-vmm-surface-hover'}`}
              >
                <Server size={16} className={address.split(',').map(a => a.trim()).includes(node.address) ? 'text-vmm-accent' : 'text-vmm-text-muted'} />
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-vmm-text">{node.hostname}</span>
                    <span className="text-[10px] text-vmm-text-muted">v{node.version}</span>
                    {node.san_volumes > 0 && (
                      <span className="inline-flex items-center gap-0.5 text-[10px] text-vmm-accent bg-vmm-accent/10 px-1.5 py-0.5 rounded font-bold">
                        <Boxes size={8} /> SAN
                      </span>
                    )}
                  </div>
                  <div className="text-xs text-vmm-text-muted truncate">{node.address}</div>
                </div>
                <span className="text-xs text-vmm-text-muted">{node.age_secs}s ago</span>
              </button>
            ))}
          </div>
        </Card>
      )}

      <Card>
        <form onSubmit={handleSubmit} className="p-6 space-y-5">
          <p className="text-sm text-vmm-text-muted">
            Register a vmm-server instance as a managed host.
            {discovered.length > 0
              ? ' Select a discovered node above, or enter an address manually.'
              : ' Enter the IP address or hostname — the connection will be detected automatically.'}
          </p>

          {error && (
            <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm whitespace-pre-line">
              {error}
            </div>
          )}

          {results.length > 1 && (
            <div className="space-y-1">
              {results.map((r, i) => (
                <div key={i} className={`flex items-center gap-2 text-sm px-3 py-1.5 rounded-lg ${
                  r.ok ? 'bg-vmm-success/10 text-vmm-success' : 'bg-vmm-danger/10 text-vmm-danger'
                }`}>
                  <span>{r.ok ? '\u2713' : '\u2717'}</span>
                  <span className="font-medium">{r.address}</span>
                  {r.error && <span className="text-xs opacity-75">— {r.error}</span>}
                </div>
              ))}
            </div>
          )}

          <div className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-vmm-text mb-1">Host Address</label>
              <input
                type="text" value={address} onChange={e => setAddress(e.target.value)}
                placeholder="192.168.1.10, 192.168.1.11, 192.168.1.12"
                className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm
                  focus:ring-1 focus:ring-vmm-accent focus:border-vmm-accent"
                required
              />
              <p className="text-xs text-vmm-text-muted mt-1">
                IP, hostname, or full URL. Comma-separated for multiple hosts. Port 8443 with https/http auto-detection.
              </p>
              {probeStatus && (
                <p className="text-xs text-vmm-accent mt-1 flex items-center gap-1.5">
                  {loading && <Loader size={10} className="animate-spin" />}
                  {probeStatus}
                </p>
              )}
            </div>

            <div>
              <label className="block text-sm font-medium text-vmm-text mb-1">Cluster</label>
              {clusters.length > 0 ? (
                <select
                  value={clusterId} onChange={e => setClusterId(e.target.value)}
                  className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm"
                  required
                >
                  {clusters.map(c => (
                    <option key={c.id} value={c.id}>{c.name}</option>
                  ))}
                </select>
              ) : (
                <div className="space-y-3">
                  <div className="flex items-center gap-2 p-3 rounded-lg bg-vmm-warning/5 border border-vmm-warning/20">
                    <Workflow size={14} className="text-vmm-warning shrink-0" />
                    <p className="text-xs text-vmm-text-dim">
                      No cluster exists yet. Create one to register hosts.
                    </p>
                  </div>
                  <div className="flex items-center gap-2">
                    <input
                      type="text" value={newClusterName} onChange={e => setNewClusterName(e.target.value)}
                      placeholder="Cluster name"
                      className="flex-1 px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm"
                    />
                    <button
                      type="button"
                      disabled={creatingCluster || !newClusterName.trim()}
                      onClick={async () => {
                        setCreatingCluster(true)
                        try {
                          const { data } = await api.post('/api/clusters', { name: newClusterName })
                          setClusterId(data.id)
                          fetchClusters()
                        } catch (err: any) {
                          setError(err.response?.data?.error || 'Failed to create cluster')
                        } finally {
                          setCreatingCluster(false)
                        }
                      }}
                      className="flex items-center gap-1.5 px-4 py-2 bg-vmm-accent hover:bg-vmm-accent-hover text-white
                        rounded-lg text-sm font-medium transition-colors disabled:opacity-50 cursor-pointer whitespace-nowrap"
                    >
                      <Plus size={14} /> {creatingCluster ? 'Creating...' : 'Create Cluster'}
                    </button>
                  </div>
                </div>
              )}
            </div>

            <div className="border-t border-vmm-border pt-4">
              <h3 className="text-sm font-semibold text-vmm-text mb-3 flex items-center gap-2">
                <Server size={14} /> Host Admin Credentials
              </h3>
              <p className="text-xs text-vmm-text-muted mb-3">
                Used once to verify access and register the host. Not stored.
              </p>

              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label className="block text-sm font-medium text-vmm-text mb-1">Username</label>
                  <input
                    type="text" value={adminUser} onChange={e => setAdminUser(e.target.value)}
                    className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm"
                    required
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-vmm-text mb-1">Password</label>
                  <input
                    type="password" value={adminPass} onChange={e => setAdminPass(e.target.value)}
                    className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm"
                    required
                  />
                </div>
              </div>
            </div>
          </div>

          <div className="flex justify-end pt-2">
            <button
              type="submit" disabled={loading || !clusterId}
              className="flex items-center gap-2 px-6 py-2.5 bg-vmm-accent hover:bg-vmm-accent-hover text-white
                rounded-lg text-sm font-medium transition-colors disabled:opacity-50 cursor-pointer"
            >
              <Plus size={16} /> {loading ? 'Registering...' : 'Register Host'}
            </button>
          </div>
        </form>
      </Card>
    </div>
  )
}
