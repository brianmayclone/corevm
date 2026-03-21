/** Host-Only Networks — isolated networks without external access. */
import { useEffect, useState } from 'react'
import api from '../api/client'
import type { NetworkInterface } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import NetworkInterfaceRow from '../components/NetworkInterfaceRow'
import Button from '../components/Button'
import { Plus } from 'lucide-react'

export default function NetworkHostOnly() {
  const [interfaces, setInterfaces] = useState<NetworkInterface[]>([])

  useEffect(() => {
    api.get<NetworkInterface[]>('/api/network/interfaces').then(({ data }) => {
      // Filter to virtual/bridge interfaces typically used for host-only
      setInterfaces(data.filter(i => i.kind === 'virtual' || i.kind === 'bridge'))
    })
  }, [])

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Host-Only Networks</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            Isolated virtual networks for VM-to-host communication only
          </p>
        </div>
        <Button variant="primary" icon={<Plus size={14} />}>Create Network</Button>
      </div>

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
          <div className="text-vmm-text-muted text-sm py-8 text-center">
            No host-only networks configured. Create one to enable VM-to-host isolation.
          </div>
        ) : (
          interfaces.map((iface) => <NetworkInterfaceRow key={iface.name} iface={iface} />)
        )}
      </Card>
    </div>
  )
}
