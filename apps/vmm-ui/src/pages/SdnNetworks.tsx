import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Network, Plus, Trash2, Wifi, Globe, Server, Circle, Shield } from 'lucide-react'
import api from '../api/client'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'

interface VirtualNetwork {
  id: number; cluster_id: string; name: string; vlan_id: number | null
  subnet: string; gateway: string
  dhcp_enabled: boolean; dhcp_range_start: string; dhcp_range_end: string
  dns_enabled: boolean; dns_domain: string
  pxe_enabled: boolean; pxe_boot_file: string
  auto_register_dns: boolean; created_at: string
}

export default function SdnNetworks() {
  const navigate = useNavigate()
  const { clusters, fetchClusters } = useClusterStore()
  const [networks, setNetworks] = useState<VirtualNetwork[]>([])
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState({ cluster_id: '', name: '', subnet: '10.0.0.0/24', gateway: '10.0.0.1', vlan_id: '' })

  const fetchNetworks = () => api.get<VirtualNetwork[]>('/api/networks').then(({ data }) => setNetworks(data)).catch(() => {})

  useEffect(() => { fetchNetworks(); fetchClusters() }, [])
  useEffect(() => { if (clusters.length > 0 && !form.cluster_id) setForm(f => ({ ...f, cluster_id: clusters[0].id })) }, [clusters])

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault()
    await api.post('/api/networks', { ...form, vlan_id: form.vlan_id ? parseInt(form.vlan_id) : null })
    setShowCreate(false)
    setForm({ cluster_id: clusters[0]?.id || '', name: '', subnet: '10.0.0.0/24', gateway: '10.0.0.1', vlan_id: '' })
    fetchNetworks()
  }

  const clusterName = (id: string) => clusters.find(c => c.id === id)?.name || id.substring(0, 8)

  return (
    <div className="space-y-5">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Networks</h1>
          <p className="text-sm text-vmm-text-muted mt-1">Software-defined virtual networks with integrated DHCP, DNS and PXE</p>
        </div>
        <button onClick={() => setShowCreate(true)}
          className="flex items-center gap-2 px-4 py-2 bg-vmm-accent hover:bg-vmm-accent-hover text-white rounded-lg text-sm font-medium">
          <Plus size={16} /> New Network
        </button>
      </div>

      {/* Create form */}
      {showCreate && (
        <Card>
          <form onSubmit={handleCreate} className="p-5 space-y-4">
            <h3 className="text-sm font-semibold text-vmm-text">Create Virtual Network</h3>
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Network Name</label>
                <input type="text" value={form.name} onChange={e => setForm({ ...form, name: e.target.value })}
                  placeholder="Production-LAN" required className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              </div>
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Assigned Cluster</label>
                <select value={form.cluster_id} onChange={e => setForm({ ...form, cluster_id: e.target.value })}
                  className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  {clusters.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
                </select>
              </div>
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Subnet (CIDR)</label>
                <input type="text" value={form.subnet} onChange={e => setForm({ ...form, subnet: e.target.value })}
                  placeholder="10.0.0.0/24" className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              </div>
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Gateway</label>
                <input type="text" value={form.gateway} onChange={e => setForm({ ...form, gateway: e.target.value })}
                  placeholder="10.0.0.1" className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              </div>
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">VLAN ID (optional)</label>
                <input type="number" value={form.vlan_id} onChange={e => setForm({ ...form, vlan_id: e.target.value })}
                  placeholder="None" className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              </div>
            </div>
            <div className="flex gap-2 justify-end pt-2">
              <button type="button" onClick={() => setShowCreate(false)} className="px-4 py-2 text-sm text-vmm-text-muted">Cancel</button>
              <button type="submit" className="px-5 py-2 bg-vmm-accent text-white rounded-lg text-sm font-medium">Create Network</button>
            </div>
          </form>
        </Card>
      )}

      {/* Network topology overview */}
      {networks.length > 0 && (
        <Card>
          <div className="p-5">
            <h3 className="text-sm font-semibold text-vmm-text mb-4">Network Topology</h3>
            <div className="flex items-start gap-6 overflow-x-auto pb-2">
              {/* Internet / Upstream */}
              <div className="flex flex-col items-center flex-shrink-0">
                <div className="w-14 h-14 rounded-full border-2 border-vmm-border flex items-center justify-center">
                  <Globe size={22} className="text-vmm-text-muted" />
                </div>
                <span className="text-[10px] text-vmm-text-muted mt-1">Upstream</span>
              </div>
              <div className="flex flex-col items-center justify-center flex-shrink-0 pt-4">
                <div className="w-12 border-t-2 border-dashed border-vmm-border" />
              </div>
              {/* Gateway / Cluster */}
              <div className="flex flex-col items-center flex-shrink-0">
                <div className="w-14 h-14 rounded-full border-2 border-vmm-accent bg-vmm-accent/10 flex items-center justify-center">
                  <Server size={22} className="text-vmm-accent" />
                </div>
                <span className="text-[10px] text-vmm-text mt-1 font-medium">Cluster</span>
              </div>
              <div className="flex flex-col items-center justify-center flex-shrink-0 pt-4">
                <div className="w-8 border-t-2 border-vmm-accent" />
              </div>
              {/* Networks */}
              <div className="flex flex-col gap-3 flex-shrink-0">
                {networks.map(net => (
                  <div key={net.id} onClick={() => navigate(`/cluster/networks/${net.id}`)}
                    className="flex items-center gap-3 px-4 py-2.5 border border-vmm-border rounded-lg cursor-pointer hover:border-vmm-accent/50 hover:bg-vmm-accent/5 transition-colors">
                    <Network size={14} className="text-vmm-accent" />
                    <div>
                      <div className="text-xs font-medium text-vmm-text">{net.name}</div>
                      <div className="text-[10px] text-vmm-text-muted">{net.subnet}</div>
                    </div>
                    <div className="flex gap-1 ml-3">
                      {net.dhcp_enabled && <span className="w-1.5 h-1.5 rounded-full bg-vmm-success" title="DHCP" />}
                      {net.dns_enabled && <span className="w-1.5 h-1.5 rounded-full bg-vmm-accent" title="DNS" />}
                      {net.pxe_enabled && <span className="w-1.5 h-1.5 rounded-full bg-yellow-400" title="PXE" />}
                    </div>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </Card>
      )}

      {/* Network cards */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {networks.map(net => (
          <Card key={net.id}>
            <div className="p-5 cursor-pointer hover:bg-vmm-surface-hover/50 rounded-xl transition-colors"
              onClick={() => navigate(`/cluster/networks/${net.id}`)}>
              <div className="flex items-start justify-between mb-4">
                <div className="flex items-center gap-3">
                  <div className="w-10 h-10 rounded-lg bg-vmm-accent/10 flex items-center justify-center">
                    <Network size={20} className="text-vmm-accent" />
                  </div>
                  <div>
                    <h3 className="text-base font-semibold text-vmm-text">{net.name}</h3>
                    <div className="text-xs text-vmm-text-muted mt-0.5">
                      {net.subnet} &bull; GW {net.gateway}{net.vlan_id ? ` • VLAN ${net.vlan_id}` : ''}
                    </div>
                  </div>
                </div>
                <button onClick={(e) => { e.stopPropagation(); if (confirm(`Delete "${net.name}"?`)) api.delete(`/api/networks/${net.id}`).then(fetchNetworks) }}
                  className="text-vmm-text-muted hover:text-vmm-danger p-1"><Trash2 size={14} /></button>
              </div>

              <div className="flex items-center gap-2 mb-3 text-xs">
                <Shield size={11} className="text-vmm-text-muted" />
                <span className="text-vmm-text-muted">Cluster:</span>
                <span className="text-vmm-text font-medium">{clusterName(net.cluster_id)}</span>
              </div>

              <div className="grid grid-cols-3 gap-2">
                <div className={`flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium ${net.dhcp_enabled ? 'bg-vmm-success/10 text-vmm-success' : 'bg-vmm-surface text-vmm-text-muted'}`}>
                  <Circle size={5} className={net.dhcp_enabled ? 'fill-current' : ''} />
                  <Wifi size={11} /> DHCP
                </div>
                <div className={`flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium ${net.dns_enabled ? 'bg-vmm-success/10 text-vmm-success' : 'bg-vmm-surface text-vmm-text-muted'}`}>
                  <Circle size={5} className={net.dns_enabled ? 'fill-current' : ''} />
                  <Globe size={11} /> DNS
                </div>
                <div className={`flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium ${net.pxe_enabled ? 'bg-vmm-success/10 text-vmm-success' : 'bg-vmm-surface text-vmm-text-muted'}`}>
                  <Circle size={5} className={net.pxe_enabled ? 'fill-current' : ''} />
                  <Server size={11} /> PXE
                </div>
              </div>
            </div>
          </Card>
        ))}
      </div>

      {networks.length === 0 && !showCreate && (
        <div className="text-center py-16">
          <Network size={40} className="mx-auto mb-3 text-vmm-text-muted opacity-20" />
          <h3 className="text-vmm-text font-medium mb-1">No virtual networks</h3>
          <p className="text-sm text-vmm-text-muted">Create a software-defined network to provide DHCP, DNS, and PXE boot to your VMs.</p>
        </div>
      )}
    </div>
  )
}
