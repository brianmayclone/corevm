import { useState, useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { ArrowLeft, Server, Plus, Loader } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
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

  useEffect(() => {
    fetchClusters()
  }, [])

  useEffect(() => {
    if (clusters.length > 0 && !clusterId) setClusterId(clusters[0].id)
  }, [clusters])

  /**
   * Resolve the user input (IP, hostname, or full URL) into a reachable vmm-server address.
   * Tries https first, then http, on port 8443.
   */
  const resolveAddress = async (input: string): Promise<string> => {
    // If user already provided a full URL, use it directly
    if (input.startsWith('http://') || input.startsWith('https://')) {
      return input.replace(/\/+$/, '')
    }

    // Strip any trailing port/slashes from raw input
    const host = input.replace(/[:/].*$/, '').trim()
    if (!host) throw new Error('Please enter an IP address or hostname')

    // Try https first (production), then http (development)
    const candidates = [
      `https://${host}:8443`,
      `http://${host}:8443`,
    ]

    for (const url of candidates) {
      setProbeStatus(`Trying ${url}...`)
      try {
        const controller = new AbortController()
        const timeout = setTimeout(() => controller.abort(), 3000)
        const resp = await fetch(`${url}/api/system/info`, {
          signal: controller.signal,
          // Skip TLS validation for self-signed certs in dev
        }).catch(() => null)
        clearTimeout(timeout)
        if (resp && resp.ok) {
          setProbeStatus(`Connected via ${url}`)
          return url
        }
      } catch {
        // Try next candidate
      }
    }

    throw new Error(`Cannot reach vmm-server at ${host}:8443 (tried https and http)`)
  }

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    setLoading(true)
    setProbeStatus('')
    try {
      const resolvedAddress = await resolveAddress(address)
      setProbeStatus('Registering host...')
      await registerHost(resolvedAddress, clusterId, adminUser, adminPass)
      navigate('/cluster/hosts')
    } catch (err: any) {
      setError(err.response?.data?.error || err.message || 'Registration failed')
      setProbeStatus('')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="space-y-5 max-w-2xl">
      <div className="flex items-center gap-3">
        <button onClick={() => navigate('/cluster/hosts')} className="text-vmm-text-muted hover:text-vmm-text cursor-pointer">
          <ArrowLeft size={20} />
        </button>
        <h1 className="text-2xl font-bold text-vmm-text">Add Host to Cluster</h1>
      </div>

      <Card>
        <form onSubmit={handleSubmit} className="p-6 space-y-5">
          <p className="text-sm text-vmm-text-muted">
            Register a vmm-server instance as a managed host. Enter the IP address or hostname —
            the connection will be detected automatically.
          </p>

          {error && (
            <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">
              {error}
            </div>
          )}

          <div className="space-y-4">
            <div>
              <label className="block text-sm font-medium text-vmm-text mb-1">Host Address</label>
              <input
                type="text" value={address} onChange={e => setAddress(e.target.value)}
                placeholder="192.168.1.10"
                className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm
                  focus:ring-1 focus:ring-vmm-accent focus:border-vmm-accent"
                required
              />
              <p className="text-xs text-vmm-text-muted mt-1">
                IP address or hostname of the vmm-server. Port 8443 is used automatically (https first, then http).
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
              <select
                value={clusterId} onChange={e => setClusterId(e.target.value)}
                className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm"
                required
              >
                {clusters.map(c => (
                  <option key={c.id} value={c.id}>{c.name}</option>
                ))}
              </select>
            </div>

            <div className="border-t border-vmm-border pt-4">
              <h3 className="text-sm font-semibold text-vmm-text mb-3 flex items-center gap-2">
                <Server size={14} /> Host Admin Credentials
              </h3>
              <p className="text-xs text-vmm-text-muted mb-3">
                These credentials are used once to verify access and register the host. They are not stored.
              </p>

              <div className="grid grid-cols-2 gap-4">
                <div>
                  <label className="block text-sm font-medium text-vmm-text mb-1">Admin Username</label>
                  <input
                    type="text" value={adminUser} onChange={e => setAdminUser(e.target.value)}
                    className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm"
                    required
                  />
                </div>
                <div>
                  <label className="block text-sm font-medium text-vmm-text mb-1">Admin Password</label>
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
              type="submit" disabled={loading}
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
