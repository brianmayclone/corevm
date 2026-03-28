import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Cable, Network, Server, Monitor, HardDrive, Database, RefreshCw } from 'lucide-react'
import api from '../api/client'
import Card from '../components/Card'
import type { ViSwitch, ViSwitchUplink, ViSwitchPort, HostNicInfo } from '../api/types'

interface VirtualNetwork {
  id: number; name: string; subnet: string
  dhcp_enabled: boolean; dns_enabled: boolean; pxe_enabled: boolean
}

interface SanNodeInfo {
  host_id: string; hostname: string; san_node_id: string
  san_address: string; san_enabled: boolean; volumes: number
  status: string
}

interface ViSwitchFull extends ViSwitch {
  uplinks: ViSwitchUplink[]
  ports: ViSwitchPort[]
}

export default function NetworkTopology() {
  const navigate = useNavigate()
  const [hostNics, setHostNics] = useState<HostNicInfo[]>([])
  const [switches, setSwitches] = useState<ViSwitchFull[]>([])
  const [networks, setNetworks] = useState<VirtualNetwork[]>([])
  const [sanNodes, setSanNodes] = useState<SanNodeInfo[]>([])
  const [loading, setLoading] = useState(true)

  const fetchAll = async () => {
    setLoading(true)
    try {
      const [nicsRes, switchesRes, netsRes, hostsRes] = await Promise.all([
        api.get('/api/viswitches/host-nics'),
        api.get('/api/viswitches'),
        api.get('/api/networks'),
        api.get('/api/hosts'),
      ])
      setHostNics(nicsRes.data || [])
      setNetworks(netsRes.data || [])

      // Fetch full details for each viSwitch
      const rawSwitches = switchesRes.data || []
      const fullSwitches: ViSwitchFull[] = await Promise.all(
        rawSwitches.map(async (vs: ViSwitch) => {
          try {
            const { data } = await api.get(`/api/viswitches/${vs.id}`)
            return { ...data.viswitch, uplinks: data.uplinks || [], ports: data.ports || [] }
          } catch { return { ...vs, uplinks: [], ports: [] } }
        })
      )
      setSwitches(fullSwitches)

      // Extract SAN nodes from hosts
      const hosts = hostsRes.data || []
      const sans: SanNodeInfo[] = hosts.filter((h: any) => h.san_enabled).map((h: any) => ({
        host_id: h.id, hostname: h.hostname || h.display_name,
        san_node_id: h.san_node_id, san_address: h.san_address,
        san_enabled: h.san_enabled, volumes: h.san_volumes || 0,
        status: h.status,
      }))
      setSanNodes(sans)
    } catch {}
    setLoading(false)
  }

  useEffect(() => { fetchAll() }, [])

  // Determine which NICs are assigned to viSwitches
  const assignedNics = new Set<string>()
  switches.forEach(vs => vs.uplinks.forEach(u => {
    if (u.uplink_type === 'physical') assignedNics.add(u.physical_nic)
  }))

  // Get connected network IDs
  const connectedNetIds = new Set<number>()
  switches.forEach(vs => vs.uplinks.forEach(u => {
    if (u.uplink_type === 'virtual' && u.network_id) connectedNetIds.add(u.network_id)
  }))

  if (loading) return <div className="text-vmm-text-muted py-8 text-center">Loading topology...</div>

  return (
    <div className="space-y-5">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Network Topology</h1>
          <p className="text-sm text-vmm-text-muted mt-1">Visual overview of physical NICs, virtual switches, networks, VMs, and CoreSAN</p>
        </div>
        <button onClick={fetchAll}
          className="flex items-center gap-2 px-3 py-2 text-vmm-text-muted hover:text-vmm-text text-sm">
          <RefreshCw size={14} /> Refresh
        </button>
      </div>

      {/* Topology visualization */}
      <Card>
        <div className="p-5">
          <div className="flex items-start gap-6 overflow-x-auto pb-4 min-h-[300px]">

            {/* Column 1: Physical NICs (per host) */}
            <div className="flex-shrink-0 min-w-[200px]">
              <div className="text-[10px] uppercase tracking-wider text-vmm-text-muted font-semibold mb-3">Physical NICs</div>
              {hostNics.map(host => (
                <div key={host.host_id} className="mb-4">
                  <div className="flex items-center gap-2 mb-2">
                    <Server size={12} className="text-vmm-text-muted" />
                    <span className="text-xs font-medium text-vmm-text">{host.hostname}</span>
                  </div>
                  <div className="space-y-1 ml-5">
                    {host.nics
                      .filter(n => !['lo'].includes(n.name) && !n.name.startsWith('vs') && !n.name.startsWith('vx') && !n.name.startsWith('bond-') && !n.name.startsWith('sdn'))
                      .map(nic => {
                        const isAssigned = assignedNics.has(nic.name)
                        // Find traffic types for this NIC
                        const trafficTypes: string[] = []
                        switches.forEach(vs => vs.uplinks.forEach(u => {
                          if (u.physical_nic === nic.name) {
                            u.traffic_types.split(',').forEach(t => {
                              if (!trafficTypes.includes(t.trim())) trafficTypes.push(t.trim())
                            })
                          }
                        }))
                        return (
                          <div key={nic.name}
                            className={`border rounded px-2.5 py-1.5 text-[11px] flex items-center gap-2 ${
                              isAssigned ? 'border-vmm-accent/40 bg-vmm-accent/5' : 'border-vmm-border opacity-50'
                            }`}>
                            <HardDrive size={10} className={isAssigned ? 'text-vmm-accent' : 'text-vmm-text-muted'} />
                            <span className={isAssigned ? 'text-vmm-text font-medium' : 'text-vmm-text-muted'}>{nic.name}</span>
                            <span className="text-vmm-text-muted text-[9px]">
                              {nic.speed_mbps ? (nic.speed_mbps >= 1000 ? `${nic.speed_mbps / 1000}G` : `${nic.speed_mbps}M`) : ''}
                            </span>
                            {trafficTypes.map(tt => (
                              <span key={tt} className={`px-1 py-0 rounded text-[8px] font-medium ${
                                tt === 'vm' ? 'bg-blue-500/15 text-blue-400' : 'bg-orange-500/15 text-orange-400'
                              }`}>{tt.toUpperCase()}</span>
                            ))}
                          </div>
                        )
                      })}
                  </div>
                </div>
              ))}
              {hostNics.length === 0 && <div className="text-xs text-vmm-text-muted italic">No hosts online</div>}
            </div>

            {/* Connector lines */}
            <div className="flex flex-col justify-center flex-shrink-0 pt-8">
              <div className="w-10 border-t-2 border-dashed border-vmm-border" />
            </div>

            {/* Column 2: viSwitches */}
            <div className="flex-shrink-0 min-w-[180px]">
              <div className="text-[10px] uppercase tracking-wider text-vmm-text-muted font-semibold mb-3">viSwitches</div>
              <div className="space-y-3">
                {switches.map(vs => (
                  <div key={vs.id}
                    className="border-2 border-vmm-accent/50 rounded-lg px-4 py-3 cursor-pointer hover:border-vmm-accent transition-colors"
                    onClick={() => navigate(`/networks/viswitches/${vs.id}`)}>
                    <div className="flex items-center gap-2 mb-1">
                      <Cable size={14} className="text-vmm-accent" />
                      <span className="text-xs font-semibold text-vmm-text">{vs.name}</span>
                    </div>
                    <div className="text-[10px] text-vmm-text-muted">
                      {vs.uplink_policy === 'roundrobin' ? 'Round-Robin' : vs.uplink_policy === 'failover' ? 'Failover' : vs.uplink_policy}
                    </div>
                    {/* Port utilization mini-bar */}
                    <div className="mt-2 w-full bg-vmm-surface rounded-full h-1">
                      <div className="bg-vmm-accent h-1 rounded-full" style={{ width: `${Math.min((vs.ports.length / vs.max_ports) * 100, 100)}%` }} />
                    </div>
                    <div className="text-[9px] text-vmm-text-muted mt-0.5">{vs.ports.length}/{vs.max_ports} ports</div>
                  </div>
                ))}
                {switches.length === 0 && <div className="text-xs text-vmm-text-muted italic">No viSwitches</div>}
              </div>
            </div>

            {/* Connector lines */}
            <div className="flex flex-col justify-center flex-shrink-0 pt-8">
              <div className="w-10 border-t-2 border-dashed border-vmm-border" />
            </div>

            {/* Column 3: Virtual Networks */}
            <div className="flex-shrink-0 min-w-[180px]">
              <div className="text-[10px] uppercase tracking-wider text-vmm-text-muted font-semibold mb-3">Virtual Networks</div>
              <div className="space-y-2">
                {networks.map(net => (
                  <div key={net.id}
                    className={`border rounded px-3 py-2 cursor-pointer hover:border-vmm-accent/50 transition-colors ${
                      connectedNetIds.has(net.id) ? 'border-vmm-accent/30 bg-vmm-accent/5' : 'border-vmm-border'
                    }`}
                    onClick={() => navigate(`/cluster/networks/${net.id}`)}>
                    <div className="flex items-center gap-2">
                      <Network size={12} className="text-vmm-accent" />
                      <span className="text-xs font-medium text-vmm-text">{net.name}</span>
                    </div>
                    <div className="text-[10px] text-vmm-text-muted mt-0.5">{net.subnet}</div>
                    <div className="flex gap-1 mt-1">
                      {net.dhcp_enabled && <span className="w-1.5 h-1.5 rounded-full bg-vmm-success" title="DHCP" />}
                      {net.dns_enabled && <span className="w-1.5 h-1.5 rounded-full bg-vmm-accent" title="DNS" />}
                      {net.pxe_enabled && <span className="w-1.5 h-1.5 rounded-full bg-yellow-400" title="PXE" />}
                    </div>
                  </div>
                ))}
                {networks.length === 0 && <div className="text-xs text-vmm-text-muted italic">No networks</div>}
              </div>
            </div>

            {/* Connector lines */}
            <div className="flex flex-col justify-center flex-shrink-0 pt-8">
              <div className="w-10 border-t-2 border-dashed border-vmm-border" />
            </div>

            {/* Column 4: VMs & CoreSAN */}
            <div className="flex-shrink-0 min-w-[180px]">
              {/* VMs */}
              <div className="text-[10px] uppercase tracking-wider text-vmm-text-muted font-semibold mb-3">VMs</div>
              <div className="space-y-1 mb-6">
                {switches.flatMap(vs => vs.ports).length > 0 ? (
                  switches.flatMap(vs => vs.ports).slice(0, 12).map(p => (
                    <div key={p.id}
                      className="border border-vmm-border rounded px-2.5 py-1.5 text-[11px] flex items-center gap-2 cursor-pointer hover:border-vmm-accent/50"
                      onClick={() => p.vm_id && navigate(`/vms/${p.vm_id}`)}>
                      <Monitor size={10} className="text-vmm-text-muted" />
                      <span className="text-vmm-text">{p.vm_name || `VM ${p.vm_id?.substring(0, 8)}`}</span>
                    </div>
                  ))
                ) : (
                  <div className="text-xs text-vmm-text-muted italic">No VMs connected</div>
                )}
                {switches.flatMap(vs => vs.ports).length > 12 && (
                  <div className="text-[10px] text-vmm-text-muted">+{switches.flatMap(vs => vs.ports).length - 12} more</div>
                )}
              </div>

              {/* CoreSAN Nodes */}
              {sanNodes.length > 0 && (
                <>
                  <div className="text-[10px] uppercase tracking-wider text-orange-400 font-semibold mb-3 flex items-center gap-1">
                    <Database size={10} /> CoreSAN
                  </div>
                  <div className="space-y-2">
                    {sanNodes.map(san => (
                      <div key={san.host_id}
                        className="border border-orange-500/30 rounded px-2.5 py-2 bg-orange-500/5 cursor-pointer hover:border-orange-500/50"
                        onClick={() => navigate('/storage/coresan')}>
                        <div className="flex items-center gap-2">
                          <Database size={10} className="text-orange-400" />
                          <span className="text-xs font-medium text-vmm-text">{san.hostname}</span>
                          <span className={`w-1.5 h-1.5 rounded-full ${san.status === 'online' ? 'bg-vmm-success' : 'bg-vmm-danger'}`} />
                        </div>
                        <div className="text-[10px] text-vmm-text-muted mt-0.5">{san.volumes} volumes</div>
                      </div>
                    ))}
                  </div>
                  {/* Peer connections */}
                  {sanNodes.length > 1 && (
                    <div className="mt-2 text-[9px] text-orange-400/60 italic">
                      {sanNodes.length} nodes — peer mesh active
                    </div>
                  )}
                </>
              )}
            </div>
          </div>
        </div>
      </Card>

      {/* Legend */}
      <div className="flex items-center gap-6 text-[10px] text-vmm-text-muted">
        <div className="flex items-center gap-1.5">
          <span className="w-3 h-2 rounded bg-blue-500/20 border border-blue-400/30" />
          VM Traffic
        </div>
        <div className="flex items-center gap-1.5">
          <span className="w-3 h-2 rounded bg-orange-500/20 border border-orange-400/30" />
          CoreSAN Storage
        </div>
        <div className="flex items-center gap-1.5">
          <span className="w-1.5 h-1.5 rounded-full bg-vmm-success" />
          DHCP
        </div>
        <div className="flex items-center gap-1.5">
          <span className="w-1.5 h-1.5 rounded-full bg-vmm-accent" />
          DNS
        </div>
        <div className="flex items-center gap-1.5">
          <span className="w-1.5 h-1.5 rounded-full bg-yellow-400" />
          PXE
        </div>
      </div>
    </div>
  )
}
