import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { Server, Plus, Circle, Cpu, MemoryStick, Wrench, Boxes } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'
import { formatRam } from '../utils/format'

export default function HostsList() {
  const { hosts, fetchHosts } = useClusterStore()
  const navigate = useNavigate()

  useEffect(() => {
    fetchHosts()
    const timer = setInterval(fetchHosts, 5000)
    return () => clearInterval(timer)
  }, [])

  const statusColor = (status: string) => {
    switch (status) {
      case 'online': return 'text-vmm-success fill-vmm-success'
      case 'maintenance': return 'text-yellow-400 fill-yellow-400'
      case 'connecting': return 'text-blue-400 fill-blue-400'
      default: return 'text-vmm-danger fill-vmm-danger'
    }
  }

  return (
    <div className="space-y-5">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Hosts</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            {hosts.length} registered hosts &bull; {hosts.filter(h => h.status === 'online').length} online
          </p>
        </div>
        <button
          onClick={() => navigate('/cluster/hosts/add')}
          className="flex items-center gap-2 px-4 py-2 bg-vmm-accent hover:bg-vmm-accent-hover text-white rounded-lg text-sm font-medium transition-colors"
        >
          <Plus size={16} /> Add Host
        </button>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
        {hosts.map((host) => {
          const ramPct = host.total_ram_mb > 0
            ? Math.round(((host.total_ram_mb - host.free_ram_mb) / host.total_ram_mb) * 100)
            : 0

          return (
            <Card key={host.id}>
              <div
                onClick={() => navigate(`/cluster/hosts/${host.id}`)}
                className="p-4 cursor-pointer hover:bg-vmm-surface-hover rounded-lg transition-colors"
              >
                <div className="flex items-center justify-between mb-3">
                  <div className="flex items-center gap-2">
                    <Server size={16} className="text-vmm-text-muted" />
                    <span className="font-semibold text-vmm-text">{host.hostname}</span>
                  </div>
                  <div className="flex items-center gap-1.5">
                    <Circle size={6} className={statusColor(host.status)} />
                    <span className="text-xs text-vmm-text-muted capitalize">{host.status}</span>
                    {host.maintenance_mode && <Wrench size={12} className="text-yellow-400 ml-1" />}
                  </div>
                </div>

                <div className="text-xs text-vmm-text-muted mb-2">{host.address}</div>

                <div className="space-y-2">
                  <div className="flex items-center gap-2 text-xs text-vmm-text-dim">
                    <Cpu size={12} />
                    <span>{host.cpu_cores} cores &bull; {host.cpu_usage_pct.toFixed(0)}% used</span>
                  </div>
                  <div className="flex items-center gap-2 text-xs text-vmm-text-dim">
                    <MemoryStick size={12} />
                    <span>{formatRam(host.total_ram_mb - host.free_ram_mb)} / {formatRam(host.total_ram_mb)}</span>
                  </div>
                  <div className="w-full bg-vmm-bg rounded-full h-1.5">
                    <div
                      className={`h-1.5 rounded-full transition-all ${ramPct > 90 ? 'bg-vmm-danger' : ramPct > 70 ? 'bg-yellow-400' : 'bg-vmm-accent'}`}
                      style={{ width: `${ramPct}%` }}
                    />
                  </div>
                  <div className="flex items-center justify-between text-xs text-vmm-text-muted">
                    <span className="flex items-center gap-2">
                      {host.vm_count} VMs
                      {host.san_enabled && (
                        <span className="inline-flex items-center gap-0.5 text-[10px] text-vmm-accent bg-vmm-accent/10 px-1.5 py-0.5 rounded font-bold">
                          <Boxes size={8} /> SAN
                        </span>
                      )}
                    </span>
                    <span>v{host.version}</span>
                  </div>
                </div>
              </div>
            </Card>
          )
        })}
      </div>
    </div>
  )
}
