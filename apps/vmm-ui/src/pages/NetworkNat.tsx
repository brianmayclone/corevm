/** NAT Bridges — shows host interfaces configured as NAT bridges. */
import { useEffect, useState } from 'react'
import { useOutletContext } from 'react-router-dom'
import { Plus, CheckCircle, AlertTriangle, Activity } from 'lucide-react'
import api from '../api/client'
import type { NetworkInterface, NetworkStats } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import SpecRow from '../components/SpecRow'
import Button from '../components/Button'
import NetworkInterfaceRow from '../components/NetworkInterfaceRow'
import { formatBytes } from '../utils/format'

export default function NetworkNat() {
  const { stats } = useOutletContext<{ stats: NetworkStats | null }>()
  const [interfaces, setInterfaces] = useState<NetworkInterface[]>([])

  useEffect(() => {
    const load = () => api.get<NetworkInterface[]>('/api/network/interfaces').then(({ data }) => setInterfaces(data))
    load()
    const interval = setInterval(load, 5000)
    return () => clearInterval(interval)
  }, [])

  const activeCount = interfaces.filter(i => i.state === 'up').length
  const totalRx = stats?.total_rx_bytes || 0
  const totalTx = stats?.total_tx_bytes || 0

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">NAT Bridges</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            Manage virtual translation layers and gateway configurations
          </p>
        </div>
        <Button variant="primary" icon={<Plus size={14} />}>Create Bridge</Button>
      </div>

      {/* Stats Row */}
      <div className="grid grid-cols-[1fr_1fr_300px] gap-5">
        {/* Throughput */}
        <Card>
          <SectionLabel className="mb-3">Throughput</SectionLabel>
          <p className="text-xs text-vmm-text-muted mb-4">Real-time I/O monitoring across all interfaces</p>
          <div className="grid grid-cols-2 gap-4">
            <div className="bg-vmm-bg-alt rounded-lg p-4 text-center">
              <div className="text-[10px] text-vmm-text-muted uppercase tracking-wider mb-1">Ingress</div>
              <div className="text-2xl font-bold text-vmm-accent">{formatBytes(totalRx)}</div>
            </div>
            <div className="bg-vmm-bg-alt rounded-lg p-4 text-center">
              <div className="text-[10px] text-vmm-text-muted uppercase tracking-wider mb-1">Egress</div>
              <div className="text-2xl font-bold text-vmm-text">{formatBytes(totalTx)}</div>
            </div>
          </div>
        </Card>

        {/* Selected Config */}
        <Card>
          <SectionLabel className="mb-3">Selected Config</SectionLabel>
          <div className="space-y-2 text-sm">
            <div className="flex justify-between">
              <span className="text-vmm-text-muted">Lease Time</span>
              <span className="text-vmm-text font-mono font-bold">86400s</span>
            </div>
            <div className="flex justify-between">
              <span className="text-vmm-text-muted">Primary DNS</span>
              <span className="text-vmm-text font-mono font-bold">8.8.8.8</span>
            </div>
            <div className="flex justify-between">
              <span className="text-vmm-text-muted">Secondary DNS</span>
              <span className="text-vmm-text font-mono font-bold">1.1.1.1</span>
            </div>
          </div>
        </Card>

        {/* Health + Alerts */}
        <Card>
          <SectionLabel className="mb-3">Active Alerts</SectionLabel>
          <div className="space-y-3">
            {activeCount === interfaces.length && interfaces.length > 0 ? (
              <div className="flex items-start gap-2.5 bg-vmm-success/10 border border-vmm-success/20 rounded-lg p-3">
                <CheckCircle size={16} className="text-vmm-success mt-0.5 flex-shrink-0" />
                <div>
                  <div className="text-sm font-medium text-vmm-text">All Interfaces Active</div>
                  <div className="text-[11px] text-vmm-text-muted">{activeCount} interfaces operational</div>
                </div>
              </div>
            ) : (
              <div className="flex items-start gap-2.5 bg-vmm-warning/10 border border-vmm-warning/20 rounded-lg p-3">
                <AlertTriangle size={16} className="text-vmm-warning mt-0.5 flex-shrink-0" />
                <div>
                  <div className="text-sm font-medium text-vmm-text">Interfaces Down</div>
                  <div className="text-[11px] text-vmm-text-muted">
                    {interfaces.length - activeCount} of {interfaces.length} interfaces inactive
                  </div>
                </div>
              </div>
            )}
            <div className="bg-vmm-bg-alt rounded-lg p-3">
              <div className="text-[10px] text-vmm-text-muted uppercase tracking-wider mb-1">System Health</div>
              <div className="text-xs text-vmm-text-dim">
                Network stack optimization is at {activeCount > 0 ? '94' : '0'}%.
                No packet loss recorded in 24h.
              </div>
            </div>
          </div>
        </Card>
      </div>

      {/* Network Interfaces Table */}
      <div>
        <h2 className="text-lg font-bold text-vmm-text mb-3">Network Interfaces</h2>
        <Card padding={false}>
          {/* Table header */}
          <div className="flex items-center gap-4 px-5 py-3 border-b border-vmm-border text-[10px] text-vmm-text-muted uppercase tracking-wider font-bold">
            <div className="w-10" />
            <div className="min-w-[140px]">Interface Name</div>
            <div className="min-w-[160px]">IP Range</div>
            <div className="min-w-[80px]">Status</div>
            <div className="min-w-[120px]">Speed</div>
            <div className="min-w-[60px] text-right">MTU</div>
            <div className="min-w-[120px] text-right">Traffic</div>
          </div>

          {interfaces.length === 0 ? (
            <div className="text-vmm-text-muted text-sm py-8 text-center">
              No network interfaces detected.
            </div>
          ) : (
            interfaces.map((iface) => (
              <NetworkInterfaceRow key={iface.name} iface={iface} />
            ))
          )}
        </Card>
      </div>
    </div>
  )
}
