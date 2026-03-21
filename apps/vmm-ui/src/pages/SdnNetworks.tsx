import { useEffect, useState } from 'react'
import { Network, Plus, Trash2, Wifi, Globe, Server, HardDrive, Settings } from 'lucide-react'
import api from '../api/client'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import Toggle from '../components/Toggle'

interface VirtualNetwork {
  id: number; cluster_id: string; name: string; vlan_id: number | null
  subnet: string; gateway: string
  dhcp_enabled: boolean; dhcp_range_start: string; dhcp_range_end: string; dhcp_lease_secs: number
  dns_enabled: boolean; dns_domain: string; dns_upstream: string
  pxe_enabled: boolean; pxe_boot_file: string; pxe_tftp_root: string; pxe_next_server: string
  auto_register_dns: boolean; created_at: string
}

export default function SdnNetworks() {
  const { clusters, fetchClusters } = useClusterStore()
  const [networks, setNetworks] = useState<VirtualNetwork[]>([])
  const [showCreate, setShowCreate] = useState(false)
  const [editId, setEditId] = useState<number | null>(null)
  const [form, setForm] = useState({ cluster_id: '', name: '', subnet: '10.0.0.0/24', gateway: '10.0.0.1', vlan_id: null as number | null })

  const fetchNetworks = () => api.get<VirtualNetwork[]>('/api/networks').then(({ data }) => setNetworks(data)).catch(() => {})

  useEffect(() => { fetchNetworks(); fetchClusters() }, [])
  useEffect(() => { if (clusters.length > 0 && !form.cluster_id) setForm(f => ({ ...f, cluster_id: clusters[0].id })) }, [clusters])

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault()
    await api.post('/api/networks', form)
    setShowCreate(false); setForm({ cluster_id: clusters[0]?.id || '', name: '', subnet: '10.0.0.0/24', gateway: '10.0.0.1', vlan_id: null }); fetchNetworks()
  }

  const updateField = async (id: number, updates: Record<string, any>) => {
    await api.put(`/api/networks/${id}`, updates)
    fetchNetworks()
  }

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Software Defined Networking</h1>
          <p className="text-sm text-vmm-text-muted mt-1">Virtual networks with integrated DHCP, DNS, and PXE boot services</p>
        </div>
        <button onClick={() => setShowCreate(true)}
          className="flex items-center gap-2 px-4 py-2 bg-vmm-accent hover:bg-vmm-accent-hover text-white rounded-lg text-sm font-medium">
          <Plus size={16} /> New Network
        </button>
      </div>

      {showCreate && (
        <Card>
          <form onSubmit={handleCreate} className="p-4 space-y-3">
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
              <input type="text" value={form.name} onChange={e => setForm({ ...form, name: e.target.value })}
                placeholder="Network name" required className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              <select value={form.cluster_id} onChange={e => setForm({ ...form, cluster_id: e.target.value })}
                className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                {clusters.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
              </select>
              <input type="text" value={form.subnet} onChange={e => setForm({ ...form, subnet: e.target.value })}
                placeholder="Subnet (CIDR)" className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              <input type="text" value={form.gateway} onChange={e => setForm({ ...form, gateway: e.target.value })}
                placeholder="Gateway" className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
            </div>
            <div className="flex gap-2 justify-end">
              <button type="button" onClick={() => setShowCreate(false)} className="px-3 py-1.5 text-sm text-vmm-text-muted">Cancel</button>
              <button type="submit" className="px-4 py-1.5 bg-vmm-accent text-white rounded-lg text-sm font-medium">Create</button>
            </div>
          </form>
        </Card>
      )}

      {networks.map(net => (
        <Card key={net.id}>
          <div className="p-4 space-y-4">
            {/* Header */}
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-3">
                <Network size={18} className="text-vmm-accent" />
                <div>
                  <h3 className="text-sm font-semibold text-vmm-text">{net.name}</h3>
                  <div className="text-xs text-vmm-text-muted">{net.subnet} &bull; GW: {net.gateway} {net.vlan_id ? `• VLAN ${net.vlan_id}` : ''}</div>
                </div>
              </div>
              <button onClick={async () => { if (confirm(`Delete network "${net.name}"?`)) { await api.delete(`/api/networks/${net.id}`); fetchNetworks() } }}
                className="text-vmm-text-muted hover:text-vmm-danger"><Trash2 size={14} /></button>
            </div>

            {/* Services */}
            <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
              {/* DHCP */}
              <div className="bg-vmm-bg-alt border border-vmm-border rounded-lg p-3 space-y-2">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-semibold text-vmm-text flex items-center gap-1.5"><Wifi size={12} /> DHCP Server</span>
                  <Toggle enabled={net.dhcp_enabled} onChange={v => updateField(net.id, { dhcp_enabled: v })} />
                </div>
                {net.dhcp_enabled && (
                  <div className="space-y-1.5 text-xs">
                    <div className="flex gap-2">
                      <input type="text" value={net.dhcp_range_start} placeholder="Start IP"
                        onChange={e => updateField(net.id, { dhcp_range_start: e.target.value })}
                        className="flex-1 px-2 py-1 bg-vmm-bg border border-vmm-border rounded text-vmm-text" />
                      <input type="text" value={net.dhcp_range_end} placeholder="End IP"
                        onChange={e => updateField(net.id, { dhcp_range_end: e.target.value })}
                        className="flex-1 px-2 py-1 bg-vmm-bg border border-vmm-border rounded text-vmm-text" />
                    </div>
                  </div>
                )}
              </div>

              {/* DNS */}
              <div className="bg-vmm-bg-alt border border-vmm-border rounded-lg p-3 space-y-2">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-semibold text-vmm-text flex items-center gap-1.5"><Globe size={12} /> DNS Server</span>
                  <Toggle enabled={net.dns_enabled} onChange={v => updateField(net.id, { dns_enabled: v })} />
                </div>
                {net.dns_enabled && (
                  <div className="space-y-1.5 text-xs">
                    <input type="text" value={net.dns_domain} placeholder="Domain (e.g. vm.local)"
                      onChange={e => updateField(net.id, { dns_domain: e.target.value })}
                      className="w-full px-2 py-1 bg-vmm-bg border border-vmm-border rounded text-vmm-text" />
                    <input type="text" value={net.dns_upstream} placeholder="Upstream DNS (e.g. 8.8.8.8)"
                      onChange={e => updateField(net.id, { dns_upstream: e.target.value })}
                      className="w-full px-2 py-1 bg-vmm-bg border border-vmm-border rounded text-vmm-text" />
                    <Toggle label="Auto-register VM names" enabled={net.auto_register_dns}
                      onChange={v => updateField(net.id, { auto_register_dns: v })} />
                  </div>
                )}
              </div>

              {/* PXE */}
              <div className="bg-vmm-bg-alt border border-vmm-border rounded-lg p-3 space-y-2">
                <div className="flex items-center justify-between">
                  <span className="text-xs font-semibold text-vmm-text flex items-center gap-1.5"><Server size={12} /> PXE Boot</span>
                  <Toggle enabled={net.pxe_enabled} onChange={v => updateField(net.id, { pxe_enabled: v })} />
                </div>
                {net.pxe_enabled && (
                  <div className="space-y-1.5 text-xs">
                    <input type="text" value={net.pxe_boot_file} placeholder="Boot file (e.g. ipxe.efi)"
                      onChange={e => updateField(net.id, { pxe_boot_file: e.target.value })}
                      className="w-full px-2 py-1 bg-vmm-bg border border-vmm-border rounded text-vmm-text" />
                    <input type="text" value={net.pxe_next_server} placeholder="TFTP Server IP"
                      onChange={e => updateField(net.id, { pxe_next_server: e.target.value })}
                      className="w-full px-2 py-1 bg-vmm-bg border border-vmm-border rounded text-vmm-text" />
                    <input type="text" value={net.pxe_tftp_root} placeholder="TFTP root path"
                      onChange={e => updateField(net.id, { pxe_tftp_root: e.target.value })}
                      className="w-full px-2 py-1 bg-vmm-bg border border-vmm-border rounded text-vmm-text" />
                  </div>
                )}
              </div>
            </div>
          </div>
        </Card>
      ))}

      {networks.length === 0 && !showCreate && (
        <div className="text-center py-12 text-vmm-text-muted">
          <Network size={32} className="mx-auto mb-3 opacity-30" />
          No virtual networks configured
        </div>
      )}
    </div>
  )
}
