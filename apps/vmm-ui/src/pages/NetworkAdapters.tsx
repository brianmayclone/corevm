/** Adapter Bindings — shows physical NIC bindings and assignments. */
import { useEffect, useState } from 'react'
import api from '../api/client'
import type { NetworkInterface } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import NetworkInterfaceRow from '../components/NetworkInterfaceRow'
import { formatBytes } from '../utils/format'

export default function NetworkAdapters() {
  const [interfaces, setInterfaces] = useState<NetworkInterface[]>([])

  useEffect(() => {
    api.get<NetworkInterface[]>('/api/network/interfaces').then(({ data }) => {
      setInterfaces(data.filter(i => i.kind === 'ethernet' || i.kind === 'wireless'))
    })
  }, [])

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">Adapter Bindings</h1>
        <p className="text-sm text-vmm-text-muted mt-1">
          Physical network adapter assignments and passthrough configuration
        </p>
      </div>

      {/* Summary cards */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
        {interfaces.filter(i => i.state === 'up').map((iface) => (
          <Card key={iface.name}>
            <div className="flex items-center justify-between mb-2">
              <span className="text-sm font-semibold text-vmm-text">{iface.name}</span>
              <span className="px-2 py-0.5 text-[10px] font-bold tracking-wider rounded bg-vmm-success/20 text-vmm-success border border-vmm-success/30">
                ACTIVE
              </span>
            </div>
            <div className="space-y-1 text-xs text-vmm-text-muted">
              <div className="flex justify-between"><span>IP</span><span className="font-mono text-vmm-text">{iface.ipv4 || '—'}</span></div>
              <div className="flex justify-between"><span>MAC</span><span className="font-mono text-vmm-text-dim">{iface.mac}</span></div>
              <div className="flex justify-between"><span>Speed</span><span>{iface.speed_mbps ? `${iface.speed_mbps >= 1000 ? `${iface.speed_mbps / 1000} Gbps` : `${iface.speed_mbps} Mbps`}` : '—'}</span></div>
              <div className="flex justify-between"><span>MTU</span><span>{iface.mtu}</span></div>
              <div className="flex justify-between"><span>RX/TX</span><span>{formatBytes(iface.rx_bytes)} / {formatBytes(iface.tx_bytes)}</span></div>
            </div>
          </Card>
        ))}
      </div>

      {/* All adapters */}
      <div>
        <h2 className="text-lg font-bold text-vmm-text mb-3">All Physical Adapters</h2>
        <Card padding={false}>
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
            <div className="text-vmm-text-muted text-sm py-8 text-center">No physical adapters detected.</div>
          ) : (
            interfaces.map((iface) => <NetworkInterfaceRow key={iface.name} iface={iface} />)
          )}
        </Card>
      </div>
    </div>
  )
}
