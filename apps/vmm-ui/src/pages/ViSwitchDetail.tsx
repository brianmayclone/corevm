import { useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { Cable, ArrowLeft, Plus, Trash2, Network, Server, HardDrive, Monitor } from 'lucide-react'
import api from '../api/client'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import TabBar from '../components/TabBar'
import Toggle from '../components/Toggle'
import type { ViSwitch, ViSwitchUplink, ViSwitchPort, HostNicInfo } from '../api/types'

interface VirtualNetwork {
  id: number; name: string; subnet: string
}

const policyLabels: Record<string, string> = {
  roundrobin: 'Round-Robin',
  failover: 'Failover',
  rulebased: 'Rule-Based',
}

const tabs = [
  { id: 'overview', label: 'Overview' },
  { id: 'settings', label: 'Settings' },
  { id: 'uplinks', label: 'Uplinks' },
  { id: 'ports', label: 'Ports' },
]

export default function ViSwitchDetail() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const [tab, setTab] = useState('overview')
  const [vs, setVs] = useState<ViSwitch | null>(null)
  const [uplinks, setUplinks] = useState<ViSwitchUplink[]>([])
  const [ports, setPorts] = useState<ViSwitchPort[]>([])
  const [hostNics, setHostNics] = useState<HostNicInfo[]>([])
  const [networks, setNetworks] = useState<VirtualNetwork[]>([])
  const [showAddUplink, setShowAddUplink] = useState(false)
  const [uplinkForm, setUplinkForm] = useState({
    uplink_type: 'physical', physical_nic: '', network_id: null as number | null,
    active: true, traffic_types: ['vm'] as string[],
  })

  const fetchData = () => {
    if (!id) return
    api.get(`/api/viswitches/${id}`).then(({ data }) => {
      setVs(data.viswitch)
      setUplinks(data.uplinks || [])
      setPorts(data.ports || [])
    }).catch(() => {})
  }

  useEffect(() => {
    fetchData()
    api.get('/api/viswitches/host-nics').then(({ data }) => setHostNics(data)).catch(() => {})
    api.get('/api/networks').then(({ data }) => setNetworks(data)).catch(() => {})
  }, [id])

  const updateField = async (updates: Record<string, any>) => {
    await api.put(`/api/viswitches/${id}`, updates)
    fetchData()
  }

  const handleAddUplink = async () => {
    const tt = uplinkForm.traffic_types.join(',')
    await api.post(`/api/viswitches/${id}/uplinks`, {
      uplink_type: uplinkForm.uplink_type,
      physical_nic: uplinkForm.physical_nic,
      network_id: uplinkForm.uplink_type === 'virtual' ? uplinkForm.network_id : null,
      active: uplinkForm.active,
      traffic_types: tt,
    })
    setShowAddUplink(false)
    setUplinkForm({ uplink_type: 'physical', physical_nic: '', network_id: null, active: true, traffic_types: ['vm'] })
    fetchData()
  }

  const toggleTrafficType = (tt: string) => {
    setUplinkForm(f => ({
      ...f,
      traffic_types: f.traffic_types.includes(tt)
        ? f.traffic_types.filter(t => t !== tt)
        : [...f.traffic_types, tt],
    }))
  }

  if (!vs) return <div className="text-vmm-text-muted py-8 text-center">Loading...</div>

  return (
    <div className="space-y-5">
      <div className="flex items-center gap-3">
        <button onClick={() => navigate('/networks/viswitches')} className="text-vmm-text-muted hover:text-vmm-text">
          <ArrowLeft size={20} />
        </button>
        <div className="flex-1">
          <h1 className="text-2xl font-bold text-vmm-text">{vs.name}</h1>
          <p className="text-sm text-vmm-text-muted">{policyLabels[vs.uplink_policy]} &bull; MTU {vs.mtu} &bull; {vs.enabled ? 'Enabled' : 'Disabled'}</p>
        </div>
      </div>

      <TabBar tabs={tabs} active={tab} onChange={setTab} />

      {/* ── Overview ── */}
      {tab === 'overview' && (
        <div className="space-y-4">
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
            <Card><div className="p-4 text-center">
              <Cable size={24} className="mx-auto mb-2 text-vmm-accent" />
              <div className="text-lg font-bold text-vmm-text">{uplinks.length}</div>
              <div className="text-xs text-vmm-text-muted">Uplinks / {vs.max_uplinks}</div>
            </div></Card>
            <Card><div className="p-4 text-center">
              <Monitor size={24} className="mx-auto mb-2 text-vmm-accent" />
              <div className="text-lg font-bold text-vmm-text">{ports.length}</div>
              <div className="text-xs text-vmm-text-muted">Ports used / {vs.max_ports}</div>
            </div></Card>
            <Card><div className="p-4 text-center">
              <Network size={24} className="mx-auto mb-2 text-vmm-accent" />
              <div className="text-lg font-bold text-vmm-text">{vs.mtu}</div>
              <div className="text-xs text-vmm-text-muted">MTU</div>
            </div></Card>
            <Card><div className="p-4 text-center">
              <Server size={24} className={`mx-auto mb-2 ${vs.enabled ? 'text-vmm-success' : 'text-vmm-text-muted'}`} />
              <div className="text-lg font-bold text-vmm-text">{vs.enabled ? 'Active' : 'Disabled'}</div>
              <div className="text-xs text-vmm-text-muted">Status</div>
            </div></Card>
          </div>

          {/* Topology diagram */}
          <Card><div className="p-4">
            <SectionLabel>Switch Topology</SectionLabel>
            <div className="mt-4 flex items-center justify-center gap-4 text-xs overflow-x-auto">
              {/* Uplinks */}
              <div className="space-y-1 flex-shrink-0">
                {uplinks.length > 0 ? uplinks.map(u => (
                  <div key={u.id} className="border border-vmm-border rounded px-3 py-1.5 flex items-center gap-2">
                    {u.uplink_type === 'physical' ? <HardDrive size={11} className="text-vmm-text-muted" /> : <Network size={11} className="text-vmm-accent" />}
                    <span className="text-vmm-text">{u.uplink_type === 'physical' ? u.physical_nic : (u.network_name || `Net #${u.network_id}`)}</span>
                    {u.traffic_types.split(',').map(tt => (
                      <span key={tt} className={`px-1.5 py-0.5 rounded text-[9px] font-medium ${tt.trim() === 'vm' ? 'bg-blue-500/10 text-blue-400' : 'bg-orange-500/10 text-orange-400'}`}>
                        {tt.trim().toUpperCase()}
                      </span>
                    ))}
                  </div>
                )) : <div className="text-vmm-text-muted italic">No uplinks</div>}
              </div>
              <div className="w-12 border-t-2 border-dashed border-vmm-border flex-shrink-0" />
              {/* viSwitch */}
              <div className="border-2 border-vmm-accent rounded-lg px-6 py-4 text-center flex-shrink-0">
                <Cable size={20} className="mx-auto mb-1 text-vmm-accent" />
                <div className="text-vmm-accent font-semibold">{vs.name}</div>
                <div className="text-vmm-text-muted mt-1">{policyLabels[vs.uplink_policy]}</div>
              </div>
              <div className="w-12 border-t-2 border-dashed border-vmm-border flex-shrink-0" />
              {/* Ports / VMs */}
              <div className="space-y-1 flex-shrink-0">
                {ports.length > 0 ? ports.slice(0, 8).map(p => (
                  <div key={p.id} className="border border-vmm-border rounded px-3 py-1.5 flex items-center gap-2 cursor-pointer hover:border-vmm-accent/50"
                    onClick={() => p.vm_id && navigate(`/vms/${p.vm_id}`)}>
                    <Monitor size={11} className="text-vmm-text-muted" />
                    <span className="text-vmm-text">{p.vm_name || `Port ${p.port_index}`}</span>
                  </div>
                )) : <div className="text-vmm-text-muted italic">No VMs connected</div>}
                {ports.length > 8 && <div className="text-vmm-text-muted">+{ports.length - 8} more</div>}
              </div>
            </div>
          </div></Card>
        </div>
      )}

      {/* ── Settings ── */}
      {tab === 'settings' && (
        <Card><div className="p-5 space-y-5">
          <h3 className="text-sm font-semibold text-vmm-text">viSwitch Configuration</h3>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
            <div>
              <label className="block text-xs text-vmm-text-muted mb-1">Name</label>
              <input type="text" value={vs.name}
                onChange={e => setVs({ ...vs, name: e.target.value })}
                onBlur={e => updateField({ name: e.target.value })}
                className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
            </div>
            <div>
              <label className="block text-xs text-vmm-text-muted mb-1">Description</label>
              <input type="text" value={vs.description}
                onChange={e => setVs({ ...vs, description: e.target.value })}
                onBlur={e => updateField({ description: e.target.value })}
                className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
            </div>
            <div>
              <label className="block text-xs text-vmm-text-muted mb-1">Uplink Policy</label>
              <select value={vs.uplink_policy}
                onChange={e => { setVs({ ...vs, uplink_policy: e.target.value as any }); updateField({ uplink_policy: e.target.value }) }}
                className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                <option value="failover">Failover (active / standby)</option>
                <option value="roundrobin">Round-Robin (load balance)</option>
              </select>
            </div>
            <div>
              <label className="block text-xs text-vmm-text-muted mb-1">MTU</label>
              <select value={vs.mtu}
                onChange={e => { const v = parseInt(e.target.value); setVs({ ...vs, mtu: v }); updateField({ mtu: v }) }}
                className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                <option value={1500}>1500 (Standard)</option>
                <option value={9000}>9000 (Jumbo Frames)</option>
              </select>
            </div>
          </div>
          <Toggle label="Enabled" description="Activate this virtual switch"
            enabled={vs.enabled} onChange={v => updateField({ enabled: v })} />
          <p className="text-xs text-vmm-text-muted">Changes are saved automatically.</p>
        </div></Card>
      )}

      {/* ── Uplinks ── */}
      {tab === 'uplinks' && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <SectionLabel>Configured Uplinks ({uplinks.length})</SectionLabel>
            <button onClick={() => setShowAddUplink(true)}
              className="flex items-center gap-2 px-3 py-1.5 bg-vmm-accent/10 text-vmm-accent hover:bg-vmm-accent/20 rounded-lg text-xs font-medium">
              <Plus size={12} /> Add Uplink
            </button>
          </div>

          {showAddUplink && (
            <Card><div className="p-4 space-y-3">
              <h4 className="text-sm font-semibold text-vmm-text">New Uplink</h4>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                <div>
                  <label className="block text-xs text-vmm-text-muted mb-1">Type</label>
                  <select value={uplinkForm.uplink_type} onChange={e => setUplinkForm({ ...uplinkForm, uplink_type: e.target.value, physical_nic: '', network_id: null })}
                    className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                    <option value="physical">Physical Network Adapter</option>
                    <option value="virtual">Virtual Network</option>
                  </select>
                </div>
                {uplinkForm.uplink_type === 'physical' ? (
                  <div>
                    <label className="block text-xs text-vmm-text-muted mb-1">Network Adapter</label>
                    <select value={uplinkForm.physical_nic} onChange={e => setUplinkForm({ ...uplinkForm, physical_nic: e.target.value })}
                      className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                      <option value="">Select adapter...</option>
                      {hostNics.map(h => (
                        h.nics.filter(n => n.state === 'up' && !['lo', 'virbr0'].includes(n.name) && !n.name.startsWith('vs') && !n.name.startsWith('vx') && !n.name.startsWith('bond-'))
                          .map(n => (
                            <option key={`${h.host_id}-${n.name}`} value={n.name}>
                              {h.hostname} — {n.name} ({n.speed_mbps ? `${n.speed_mbps >= 1000 ? `${n.speed_mbps/1000}Gbps` : `${n.speed_mbps}Mbps`}` : 'unknown speed'})
                            </option>
                          ))
                      ))}
                    </select>
                  </div>
                ) : (
                  <div>
                    <label className="block text-xs text-vmm-text-muted mb-1">Virtual Network</label>
                    <select value={uplinkForm.network_id ?? ''} onChange={e => setUplinkForm({ ...uplinkForm, network_id: parseInt(e.target.value) || null })}
                      className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                      <option value="">Select network...</option>
                      {networks.map(n => <option key={n.id} value={n.id}>{n.name} ({n.subnet})</option>)}
                    </select>
                  </div>
                )}
              </div>

              {/* Traffic Types */}
              <div>
                <label className="block text-xs text-vmm-text-muted mb-2">Allowed Traffic Types</label>
                <div className="flex gap-4">
                  <label className="flex items-center gap-2 text-sm text-vmm-text cursor-pointer">
                    <input type="checkbox" checked={uplinkForm.traffic_types.includes('vm')} onChange={() => toggleTrafficType('vm')}
                      className="rounded border-vmm-border" />
                    <span className="px-2 py-0.5 rounded text-xs font-medium bg-blue-500/10 text-blue-400">VM Traffic</span>
                    <span className="text-xs text-vmm-text-muted">Virtual machine data</span>
                  </label>
                  <label className="flex items-center gap-2 text-sm text-vmm-text cursor-pointer">
                    <input type="checkbox" checked={uplinkForm.traffic_types.includes('san')} onChange={() => toggleTrafficType('san')}
                      className="rounded border-vmm-border" />
                    <span className="px-2 py-0.5 rounded text-xs font-medium bg-orange-500/10 text-orange-400">CoreSAN Storage</span>
                    <span className="text-xs text-vmm-text-muted">Replication traffic</span>
                  </label>
                </div>
              </div>

              {/* Active/Standby (failover only) */}
              {vs.uplink_policy === 'failover' && (
                <Toggle label="Active Uplink" description="Standby uplinks take over when active ones fail"
                  enabled={uplinkForm.active} onChange={v => setUplinkForm({ ...uplinkForm, active: v })} />
              )}

              <div className="flex gap-2 justify-end">
                <button onClick={() => setShowAddUplink(false)} className="px-3 py-1.5 text-sm text-vmm-text-muted">Cancel</button>
                <button onClick={handleAddUplink} className="px-4 py-1.5 bg-vmm-accent text-white rounded-lg text-sm font-medium">Add Uplink</button>
              </div>
            </div></Card>
          )}

          {uplinks.length > 0 ? (
            <Card padding={false}>
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-vmm-border text-xs text-vmm-text-muted uppercase tracking-wider">
                    <th className="text-left px-4 py-2">#</th>
                    <th className="text-left px-4 py-2">Type</th>
                    <th className="text-left px-4 py-2">Target</th>
                    <th className="text-left px-4 py-2">Traffic</th>
                    <th className="text-left px-4 py-2">Status</th>
                    <th className="text-right px-4 py-2"></th>
                  </tr>
                </thead>
                <tbody>
                  {uplinks.map(u => (
                    <tr key={u.id} className="border-b border-vmm-border last:border-b-0">
                      <td className="px-4 py-2 text-vmm-text-muted">{u.uplink_index}</td>
                      <td className="px-4 py-2">
                        <span className="flex items-center gap-2 text-vmm-text">
                          {u.uplink_type === 'physical' ? <HardDrive size={12} /> : <Network size={12} />}
                          {u.uplink_type === 'physical' ? 'Physical' : 'Virtual'}
                        </span>
                      </td>
                      <td className="px-4 py-2 font-mono text-vmm-text">
                        {u.uplink_type === 'physical' ? u.physical_nic : (u.network_name || `Network #${u.network_id}`)}
                      </td>
                      <td className="px-4 py-2">
                        <div className="flex gap-1">
                          {u.traffic_types.split(',').map(tt => (
                            <span key={tt} className={`px-2 py-0.5 rounded text-[10px] font-medium ${tt.trim() === 'vm' ? 'bg-blue-500/10 text-blue-400' : 'bg-orange-500/10 text-orange-400'}`}>
                              {tt.trim().toUpperCase()}
                            </span>
                          ))}
                        </div>
                      </td>
                      <td className="px-4 py-2">
                        <span className={`text-xs px-2 py-0.5 rounded-full ${u.active ? 'bg-vmm-success/10 text-vmm-success' : 'bg-vmm-surface text-vmm-text-muted'}`}>
                          {u.active ? 'Active' : 'Standby'}
                        </span>
                      </td>
                      <td className="px-4 py-2 text-right">
                        <button onClick={() => api.delete(`/api/viswitches/${id}/${u.id}/uplink`).then(fetchData)}
                          className="text-vmm-text-muted hover:text-vmm-danger"><Trash2 size={13} /></button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </Card>
          ) : (
            <Card><div className="p-4 text-center text-vmm-text-muted text-sm py-8">
              <Cable size={28} className="mx-auto mb-2 opacity-30" />
              <p>No uplinks configured.</p>
              <p className="text-xs mt-1">Add physical adapters or virtual networks as uplinks.</p>
            </div></Card>
          )}
        </div>
      )}

      {/* ── Ports ── */}
      {tab === 'ports' && (
        <div className="space-y-4">
          <SectionLabel>Connected VMs ({ports.length} / {vs.max_ports})</SectionLabel>

          {/* Port utilization bar */}
          <div className="w-full bg-vmm-surface rounded-full h-2">
            <div className="bg-vmm-accent h-2 rounded-full transition-all" style={{ width: `${Math.min((ports.length / vs.max_ports) * 100, 100)}%` }} />
          </div>

          {ports.length > 0 ? (
            <Card padding={false}>
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b border-vmm-border text-xs text-vmm-text-muted uppercase tracking-wider">
                    <th className="text-left px-4 py-2">Port</th>
                    <th className="text-left px-4 py-2">VM</th>
                    <th className="text-left px-4 py-2">VLAN</th>
                  </tr>
                </thead>
                <tbody>
                  {ports.map(p => (
                    <tr key={p.id} className="border-b border-vmm-border last:border-b-0 cursor-pointer hover:bg-vmm-surface-hover/30"
                      onClick={() => p.vm_id && navigate(`/vms/${p.vm_id}`)}>
                      <td className="px-4 py-2 text-vmm-text-muted">{p.port_index}</td>
                      <td className="px-4 py-2 text-vmm-accent font-medium">{p.vm_name || p.vm_id || '—'}</td>
                      <td className="px-4 py-2 text-vmm-text-muted">{p.vlan_id ?? 'Untagged'}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </Card>
          ) : (
            <Card><div className="p-4 text-center text-vmm-text-muted text-sm py-8">
              <Monitor size={28} className="mx-auto mb-2 opacity-30" />
              <p>No VMs connected to this viSwitch.</p>
              <p className="text-xs mt-1">Assign VMs to this viSwitch when creating or editing a VM.</p>
            </div></Card>
          )}
        </div>
      )}
    </div>
  )
}
