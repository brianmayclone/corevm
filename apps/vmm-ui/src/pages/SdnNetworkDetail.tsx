import { useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { Network, ArrowLeft, Wifi, Globe, Server, HardDrive, Plus, Trash2, Disc, Save } from 'lucide-react'
import api from '../api/client'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import TabBar from '../components/TabBar'
import Toggle from '../components/Toggle'
import { formatBytes } from '../utils/format'

interface VirtualNetwork {
  id: number; cluster_id: string; name: string; vlan_id: number | null
  subnet: string; gateway: string
  dhcp_enabled: boolean; dhcp_range_start: string; dhcp_range_end: string; dhcp_lease_secs: number
  dns_enabled: boolean; dns_domain: string; dns_upstream: string
  pxe_enabled: boolean; pxe_boot_file: string; pxe_tftp_root: string; pxe_next_server: string
  auto_register_dns: boolean
}

interface DhcpLease {
  id: number; mac_address: string; ip_address: string; hostname: string | null; vm_id: string | null
  lease_start: string; lease_end: string | null
}

interface DnsRecord {
  id: number; record_type: string; name: string; value: string; ttl: number; auto_registered: boolean
}

interface PxeEntry {
  id: number; name: string; iso_id: string | null; iso_path: string; boot_args: string; sort_order: number; enabled: boolean
}

interface Iso {
  id: string; name: string; path: string; size_bytes: number
}

const tabs = [
  { id: 'overview', label: 'Overview' },
  { id: 'settings', label: 'Settings' },
  { id: 'dhcp', label: 'DHCP' },
  { id: 'dns', label: 'DNS' },
  { id: 'pxe', label: 'PXE Boot' },
]

export default function SdnNetworkDetail() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const [tab, setTab] = useState('overview')
  const [net, setNet] = useState<VirtualNetwork | null>(null)
  const [leases, setLeases] = useState<DhcpLease[]>([])
  const [dnsRecords, setDnsRecords] = useState<DnsRecord[]>([])
  const [pxeEntries, setPxeEntries] = useState<PxeEntry[]>([])
  const [isos, setIsos] = useState<Iso[]>([])
  const [showAddPxe, setShowAddPxe] = useState(false)
  const [pxeForm, setPxeForm] = useState({ name: '', iso_path: '', boot_args: '' })

  const fetchData = () => {
    if (!id) return
    api.get(`/api/networks/${id}`).then(({ data }) => {
      setNet(data.network)
      setLeases(data.leases || [])
      setDnsRecords(data.dns_records || [])
    }).catch(() => {})
    api.get<Iso[]>('/api/storage/isos').then(({ data }) => setIsos(data)).catch(() => {})
  }

  useEffect(() => { fetchData() }, [id])

  const updateField = async (updates: Record<string, any>) => {
    await api.put(`/api/networks/${id}`, updates)
    fetchData()
  }

  if (!net) return <div className="text-vmm-text-muted py-8 text-center">Loading...</div>

  return (
    <div className="space-y-5">
      <div className="flex items-center gap-3">
        <button onClick={() => navigate('/cluster/networks')} className="text-vmm-text-muted hover:text-vmm-text">
          <ArrowLeft size={20} />
        </button>
        <div className="flex-1">
          <h1 className="text-2xl font-bold text-vmm-text">{net.name}</h1>
          <p className="text-sm text-vmm-text-muted">{net.subnet} &bull; Gateway: {net.gateway} {net.vlan_id ? `• VLAN ${net.vlan_id}` : ''}</p>
        </div>
      </div>

      <TabBar tabs={tabs} active={tab} onChange={setTab} />

      {/* ── Overview ──────────────────────────────────────────────── */}
      {tab === 'overview' && (
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
          <Card>
            <div className="p-4 text-center">
              <Network size={24} className="mx-auto mb-2 text-vmm-accent" />
              <div className="text-lg font-bold text-vmm-text">{net.subnet}</div>
              <div className="text-xs text-vmm-text-muted">Subnet</div>
            </div>
          </Card>
          <Card>
            <div className="p-4 text-center">
              <Wifi size={24} className={`mx-auto mb-2 ${net.dhcp_enabled ? 'text-vmm-success' : 'text-vmm-text-muted'}`} />
              <div className="text-lg font-bold text-vmm-text">{net.dhcp_enabled ? 'Active' : 'Off'}</div>
              <div className="text-xs text-vmm-text-muted">DHCP ({leases.length} leases)</div>
            </div>
          </Card>
          <Card>
            <div className="p-4 text-center">
              <Globe size={24} className={`mx-auto mb-2 ${net.dns_enabled ? 'text-vmm-success' : 'text-vmm-text-muted'}`} />
              <div className="text-lg font-bold text-vmm-text">{net.dns_enabled ? 'Active' : 'Off'}</div>
              <div className="text-xs text-vmm-text-muted">DNS ({dnsRecords.length} records)</div>
            </div>
          </Card>
          <Card>
            <div className="p-4 text-center">
              <Server size={24} className={`mx-auto mb-2 ${net.pxe_enabled ? 'text-vmm-success' : 'text-vmm-text-muted'}`} />
              <div className="text-lg font-bold text-vmm-text">{net.pxe_enabled ? 'Active' : 'Off'}</div>
              <div className="text-xs text-vmm-text-muted">PXE Boot</div>
            </div>
          </Card>

          {/* Topology diagram */}
          <div className="col-span-full">
            <Card>
              <div className="p-4">
                <SectionLabel>Network Topology</SectionLabel>
                <div className="mt-4 flex items-center justify-center gap-4 text-xs">
                  <div className="border border-vmm-accent/50 bg-vmm-accent/5 rounded-lg px-4 py-3 text-center">
                    <div className="text-vmm-accent font-semibold">Gateway</div>
                    <div className="text-vmm-text font-mono mt-1">{net.gateway}</div>
                  </div>
                  <div className="w-16 border-t border-dashed border-vmm-border" />
                  <div className="border border-vmm-border rounded-lg px-4 py-3 text-center">
                    <div className="text-vmm-text-muted font-semibold">{net.name}</div>
                    <div className="text-vmm-text font-mono mt-1">{net.subnet}</div>
                  </div>
                  <div className="w-16 border-t border-dashed border-vmm-border" />
                  <div className="space-y-1">
                    {net.dhcp_enabled && <div className="border border-vmm-border rounded px-3 py-1.5 flex items-center gap-2"><Wifi size={11} className="text-vmm-success" /> DHCP {net.dhcp_range_start}–{net.dhcp_range_end}</div>}
                    {net.dns_enabled && <div className="border border-vmm-border rounded px-3 py-1.5 flex items-center gap-2"><Globe size={11} className="text-vmm-success" /> DNS {net.dns_domain || '(no domain)'}</div>}
                    {net.pxe_enabled && <div className="border border-vmm-border rounded px-3 py-1.5 flex items-center gap-2"><Server size={11} className="text-vmm-success" /> PXE {net.pxe_boot_file}</div>}
                  </div>
                </div>
              </div>
            </Card>
          </div>
        </div>
      )}

      {/* ── Settings ─────────────────────────────────────────────── */}
      {tab === 'settings' && (
        <Card>
          <div className="p-5 space-y-5">
            <h3 className="text-sm font-semibold text-vmm-text">Network Configuration</h3>
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Network Name</label>
                <input type="text" value={net.name}
                  onChange={e => setNet({ ...net, name: e.target.value })}
                  onBlur={e => updateField({ name: e.target.value })}
                  className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              </div>
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Subnet (CIDR)</label>
                <input type="text" value={net.subnet}
                  onChange={e => setNet({ ...net, subnet: e.target.value })}
                  onBlur={e => updateField({ subnet: e.target.value })}
                  className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              </div>
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Gateway</label>
                <input type="text" value={net.gateway}
                  onChange={e => setNet({ ...net, gateway: e.target.value })}
                  onBlur={e => updateField({ gateway: e.target.value })}
                  className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              </div>
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">VLAN ID (optional)</label>
                <input type="number" value={net.vlan_id ?? ''}
                  onChange={e => setNet({ ...net, vlan_id: e.target.value ? parseInt(e.target.value) : null })}
                  onBlur={e => updateField({ vlan_id: e.target.value ? parseInt(e.target.value) : null })}
                  placeholder="None"
                  className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              </div>
            </div>
            <p className="text-xs text-vmm-text-muted">
              Changes are saved automatically when you leave a field.
            </p>
          </div>
        </Card>
      )}

      {/* ── DHCP ──────────────────────────────────────────────────── */}
      {tab === 'dhcp' && (
        <div className="space-y-4">
          <Card>
            <div className="p-4 space-y-3">
              <Toggle label="DHCP Server" description="Automatically assign IP addresses to VMs on this network"
                enabled={net.dhcp_enabled} onChange={v => updateField({ dhcp_enabled: v })} />
              {net.dhcp_enabled && (
                <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 mt-3">
                  <div>
                    <label className="block text-xs text-vmm-text-muted mb-1">Range Start</label>
                    <input type="text" value={net.dhcp_range_start}
                      onBlur={e => updateField({ dhcp_range_start: e.target.value })}
                      onChange={e => setNet({ ...net, dhcp_range_start: e.target.value })}
                      className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                  </div>
                  <div>
                    <label className="block text-xs text-vmm-text-muted mb-1">Range End</label>
                    <input type="text" value={net.dhcp_range_end}
                      onBlur={e => updateField({ dhcp_range_end: e.target.value })}
                      onChange={e => setNet({ ...net, dhcp_range_end: e.target.value })}
                      className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                  </div>
                  <div>
                    <label className="block text-xs text-vmm-text-muted mb-1">Lease Time (sec)</label>
                    <input type="number" value={net.dhcp_lease_secs}
                      onBlur={e => updateField({ dhcp_lease_secs: parseInt(e.target.value) || 3600 })}
                      onChange={e => setNet({ ...net, dhcp_lease_secs: parseInt(e.target.value) || 3600 })}
                      className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                  </div>
                  <div>
                    <label className="block text-xs text-vmm-text-muted mb-1">Gateway</label>
                    <input type="text" value={net.gateway} disabled
                      className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text-muted text-sm" />
                  </div>
                </div>
              )}
            </div>
          </Card>

          {net.dhcp_enabled && (
            <>
              {/* Static Reservations */}
              <div>
                <div className="flex items-center justify-between mb-2">
                  <SectionLabel>Static Reservations (MAC → IP)</SectionLabel>
                  <button onClick={() => {
                    const mac = prompt('MAC Address (XX:XX:XX:XX:XX:XX):')
                    if (!mac) return
                    const ip = prompt('IP Address:')
                    if (!ip) return
                    const hostname = prompt('Hostname (optional):') || undefined
                    api.post(`/api/networks/${id}/reservations`, { mac_address: mac, ip_address: ip, hostname })
                      .then(fetchData).catch(e => alert(e.response?.data?.error || 'Failed'))
                  }}
                    className="flex items-center gap-1 px-3 py-1.5 bg-vmm-accent/10 text-vmm-accent hover:bg-vmm-accent/20 rounded-lg text-xs font-medium">
                    <Plus size={12} /> Add Reservation
                  </button>
                </div>
                {leases.filter(l => l.lease_end === '9999-12-31 23:59:59').length > 0 && (
                  <Card padding={false}>
                    <table className="w-full text-sm">
                      <thead>
                        <tr className="border-b border-vmm-border text-xs text-vmm-text-muted uppercase tracking-wider">
                          <th className="text-left px-4 py-2">MAC Address</th>
                          <th className="text-left px-4 py-2">IP Address</th>
                          <th className="text-left px-4 py-2">Hostname</th>
                          <th className="text-right px-4 py-2"></th>
                        </tr>
                      </thead>
                      <tbody>
                        {leases.filter(l => l.lease_end === '9999-12-31 23:59:59').map(l => (
                          <tr key={l.id} className="border-b border-vmm-border last:border-b-0">
                            <td className="px-4 py-2 font-mono text-vmm-text">{l.mac_address}</td>
                            <td className="px-4 py-2 font-mono text-vmm-accent">{l.ip_address}</td>
                            <td className="px-4 py-2 text-vmm-text">{l.hostname || '—'}</td>
                            <td className="px-4 py-2 text-right">
                              <button onClick={() => api.delete(`/api/networks/${id}/${l.id}/reservation`).then(fetchData)}
                                className="text-vmm-text-muted hover:text-vmm-danger"><Trash2 size={13} /></button>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </Card>
                )}
              </div>

              {/* Dynamic Leases */}
              <div>
                <SectionLabel>Active Leases ({leases.filter(l => l.lease_end !== '9999-12-31 23:59:59').length})</SectionLabel>
                <Card padding={false}>
                  <table className="w-full text-sm">
                    <thead>
                      <tr className="border-b border-vmm-border text-xs text-vmm-text-muted uppercase tracking-wider">
                        <th className="text-left px-4 py-2">IP Address</th>
                        <th className="text-left px-4 py-2">MAC Address</th>
                        <th className="text-left px-4 py-2">Hostname</th>
                        <th className="text-left px-4 py-2">Lease Start</th>
                        <th className="text-left px-4 py-2">Expires</th>
                      </tr>
                    </thead>
                    <tbody>
                      {leases.filter(l => l.lease_end !== '9999-12-31 23:59:59').map(l => (
                        <tr key={l.id} className="border-b border-vmm-border last:border-b-0">
                          <td className="px-4 py-2 font-mono text-vmm-text">{l.ip_address}</td>
                          <td className="px-4 py-2 font-mono text-vmm-text-dim">{l.mac_address}</td>
                          <td className="px-4 py-2 text-vmm-text">{l.hostname || '—'}</td>
                          <td className="px-4 py-2 text-vmm-text-muted">{l.lease_start}</td>
                          <td className="px-4 py-2 text-vmm-text-muted">{l.lease_end || '—'}</td>
                        </tr>
                      ))}
                      {leases.filter(l => l.lease_end !== '9999-12-31 23:59:59').length === 0 && (
                        <tr><td colSpan={5} className="px-4 py-6 text-center text-vmm-text-muted">No active DHCP leases</td></tr>
                      )}
                    </tbody>
                  </table>
                </Card>
              </div>
            </>
          )}
        </div>
      )}

      {/* ── DNS ───────────────────────────────────────────────────── */}
      {tab === 'dns' && (
        <div className="space-y-4">
          <Card>
            <div className="p-4 space-y-3">
              <Toggle label="DNS Server" description="Resolve hostnames for VMs on this network"
                enabled={net.dns_enabled} onChange={v => updateField({ dns_enabled: v })} />
              {net.dns_enabled && (
                <div className="grid grid-cols-2 gap-3 mt-3">
                  <div>
                    <label className="block text-xs text-vmm-text-muted mb-1">Domain</label>
                    <input type="text" value={net.dns_domain} placeholder="vm.local"
                      onBlur={e => updateField({ dns_domain: e.target.value })}
                      onChange={e => setNet({ ...net, dns_domain: e.target.value })}
                      className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                  </div>
                  <div>
                    <label className="block text-xs text-vmm-text-muted mb-1">Upstream DNS</label>
                    <input type="text" value={net.dns_upstream} placeholder="8.8.8.8,8.8.4.4"
                      onBlur={e => updateField({ dns_upstream: e.target.value })}
                      onChange={e => setNet({ ...net, dns_upstream: e.target.value })}
                      className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                  </div>
                </div>
              )}
              {net.dns_enabled && (
                <Toggle label="Auto-register VM names" description="Automatically create DNS A records when VMs start"
                  enabled={net.auto_register_dns} onChange={v => updateField({ auto_register_dns: v })} />
              )}
            </div>
          </Card>

          {net.dns_enabled && (
            <div>
              <div className="flex items-center justify-between mb-2">
                <SectionLabel>DNS Records ({dnsRecords.length})</SectionLabel>
                <button onClick={() => {
                  const name = prompt('Record Name (e.g. myserver.vm.local):')
                  if (!name) return
                  const value = prompt('Value (e.g. 10.0.0.50):')
                  if (!value) return
                  const type_ = prompt('Record Type (A, AAAA, CNAME, PTR):', 'A') || 'A'
                  api.post(`/api/networks/${id}/dns-records`, { record_type: type_, name, value, ttl: 3600 })
                    .then(fetchData).catch(e => alert(e.response?.data?.error || 'Failed'))
                }}
                  className="flex items-center gap-1 px-3 py-1.5 bg-vmm-accent/10 text-vmm-accent hover:bg-vmm-accent/20 rounded-lg text-xs font-medium">
                  <Plus size={12} /> Add Record
                </button>
              </div>
              <Card padding={false}>
                <table className="w-full text-sm">
                  <thead>
                    <tr className="border-b border-vmm-border text-xs text-vmm-text-muted uppercase tracking-wider">
                      <th className="text-left px-4 py-2">Type</th>
                      <th className="text-left px-4 py-2">Name</th>
                      <th className="text-left px-4 py-2">Value</th>
                      <th className="text-left px-4 py-2">TTL</th>
                      <th className="text-left px-4 py-2">Source</th>
                      <th className="text-right px-4 py-2"></th>
                    </tr>
                  </thead>
                  <tbody>
                    {dnsRecords.map(r => (
                      <tr key={r.id} className="border-b border-vmm-border last:border-b-0">
                        <td className="px-4 py-2 font-mono text-vmm-accent">{r.record_type}</td>
                        <td className="px-4 py-2 font-mono text-vmm-text">{r.name}</td>
                        <td className="px-4 py-2 font-mono text-vmm-text-dim">{r.value}</td>
                        <td className="px-4 py-2 text-vmm-text-muted">{r.ttl}s</td>
                        <td className="px-4 py-2">
                          <span className={`text-xs px-2 py-0.5 rounded-full ${r.auto_registered ? 'bg-vmm-accent/10 text-vmm-accent' : 'bg-vmm-surface text-vmm-text-muted'}`}>
                            {r.auto_registered ? 'Auto' : 'Manual'}
                          </span>
                        </td>
                        <td className="px-4 py-2 text-right">
                          {!r.auto_registered && (
                            <button onClick={() => api.delete(`/api/networks/${id}/${r.id}/dns-record`).then(fetchData)}
                              className="text-vmm-text-muted hover:text-vmm-danger"><Trash2 size={13} /></button>
                          )}
                        </td>
                      </tr>
                    ))}
                    {dnsRecords.length === 0 && (
                      <tr><td colSpan={5} className="px-4 py-6 text-center text-vmm-text-muted">No DNS records</td></tr>
                    )}
                  </tbody>
                </table>
              </Card>
            </div>
          )}
        </div>
      )}

      {/* ── PXE Boot ──────────────────────────────────────────────── */}
      {tab === 'pxe' && (
        <div className="space-y-4">
          <Card>
            <div className="p-4 space-y-3">
              <Toggle label="PXE Boot Server" description="Network boot — install OS without ISO media"
                enabled={net.pxe_enabled} onChange={v => updateField({ pxe_enabled: v })} />
              {net.pxe_enabled && (
                <div className="grid grid-cols-3 gap-3 mt-3">
                  <div>
                    <label className="block text-xs text-vmm-text-muted mb-1">Boot File</label>
                    <input type="text" value={net.pxe_boot_file} placeholder="ipxe.efi"
                      onBlur={e => updateField({ pxe_boot_file: e.target.value })}
                      onChange={e => setNet({ ...net, pxe_boot_file: e.target.value })}
                      className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                  </div>
                  <div>
                    <label className="block text-xs text-vmm-text-muted mb-1">TFTP Server IP</label>
                    <input type="text" value={net.pxe_next_server} placeholder="Cluster IP"
                      onBlur={e => updateField({ pxe_next_server: e.target.value })}
                      onChange={e => setNet({ ...net, pxe_next_server: e.target.value })}
                      className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                  </div>
                  <div>
                    <label className="block text-xs text-vmm-text-muted mb-1">TFTP Root</label>
                    <input type="text" value={net.pxe_tftp_root} placeholder="/vmm/tftp"
                      onBlur={e => updateField({ pxe_tftp_root: e.target.value })}
                      onChange={e => setNet({ ...net, pxe_tftp_root: e.target.value })}
                      className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                  </div>
                </div>
              )}
            </div>
          </Card>

          {net.pxe_enabled && (
            <div>
              <div className="flex items-center justify-between mb-3">
                <SectionLabel>Boot Menu Entries (ISOs)</SectionLabel>
                <button onClick={() => setShowAddPxe(true)}
                  className="flex items-center gap-2 px-3 py-1.5 bg-vmm-accent/10 text-vmm-accent hover:bg-vmm-accent/20 rounded-lg text-xs font-medium">
                  <Plus size={12} /> Add ISO Entry
                </button>
              </div>

              {showAddPxe && (
                <Card>
                  <div className="p-4 space-y-3">
                    <div className="grid grid-cols-3 gap-3">
                      <input type="text" value={pxeForm.name} onChange={e => setPxeForm({ ...pxeForm, name: e.target.value })}
                        placeholder="Menu label (e.g. Ubuntu 24.04)" className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                      <select value={pxeForm.iso_path} onChange={e => setPxeForm({ ...pxeForm, iso_path: e.target.value })}
                        className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                        <option value="">Select ISO...</option>
                        {isos.map(iso => <option key={iso.id} value={iso.path}>{iso.name} ({formatBytes(iso.size_bytes)})</option>)}
                      </select>
                      <input type="text" value={pxeForm.boot_args} onChange={e => setPxeForm({ ...pxeForm, boot_args: e.target.value })}
                        placeholder="Boot args (optional)" className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                    </div>
                    <div className="flex gap-2 justify-end">
                      <button type="button" onClick={() => setShowAddPxe(false)} className="px-3 py-1.5 text-sm text-vmm-text-muted">Cancel</button>
                      <button onClick={async () => {
                        // TODO: POST /api/networks/{id}/pxe-entries
                        setShowAddPxe(false)
                      }} className="px-4 py-1.5 bg-vmm-accent text-white rounded-lg text-sm font-medium">Add Entry</button>
                    </div>
                  </div>
                </Card>
              )}

              <Card>
                <div className="p-4 text-center text-vmm-text-muted text-sm py-8">
                  <Disc size={28} className="mx-auto mb-2 opacity-30" />
                  <p>PXE boot entries will appear here.</p>
                  <p className="text-xs mt-1">Link ISOs from your datastores to offer them via network boot to VMs.</p>
                </div>
              </Card>
            </div>
          )}
        </div>
      )}
    </div>
  )
}
