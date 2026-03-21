/** Row component for displaying a network interface. */
import { Network, Wifi, Globe, Unplug, Cable } from 'lucide-react'
import type { NetworkInterface } from '../api/types'
import { formatBytes } from '../utils/format'

interface Props {
  iface: NetworkInterface
}

const kindIcons: Record<string, React.ElementType> = {
  ethernet: Cable,
  wireless: Wifi,
  bridge: Network,
  virtual: Globe,
  loopback: Unplug,
}

const kindLabels: Record<string, string> = {
  ethernet: 'Ethernet',
  wireless: 'Wireless',
  bridge: 'Bridge',
  virtual: 'Virtual',
  loopback: 'Loopback',
}

export default function NetworkInterfaceRow({ iface }: Props) {
  const Icon = kindIcons[iface.kind] || Network
  const isUp = iface.state === 'up'

  return (
    <div className="flex items-center gap-3 sm:gap-4 px-3 sm:px-5 py-3 sm:py-4 border-b border-vmm-border last:border-b-0 hover:bg-vmm-surface-hover/30 transition-colors min-w-0">
      {/* Icon */}
      <div className="w-10 h-10 rounded-lg bg-vmm-bg-alt flex items-center justify-center flex-shrink-0">
        <Icon size={18} className={isUp ? 'text-vmm-accent' : 'text-vmm-text-muted'} />
      </div>

      {/* Name + type */}
      <div className="min-w-[140px]">
        <div className="flex items-center gap-2">
          <span className="text-sm font-semibold text-vmm-text">{iface.name}</span>
          <span className="text-[10px] text-vmm-text-muted">({kindLabels[iface.kind] || iface.kind})</span>
        </div>
        <div className="text-[11px] text-vmm-text-muted font-mono mt-0.5">{iface.mac}</div>
      </div>

      {/* IP */}
      <div className="min-w-[160px]">
        <span className="text-sm text-vmm-text font-mono">{iface.ipv4 || '—'}</span>
      </div>

      {/* Status badge */}
      <div className="min-w-[80px]">
        <span className={`px-2 py-0.5 text-[10px] font-bold tracking-wider rounded border
          ${isUp
            ? 'bg-vmm-success/20 text-vmm-success border-vmm-success/30'
            : 'bg-vmm-text-muted/20 text-vmm-text-muted border-vmm-text-muted/30'
          }`}>
          {isUp ? 'ACTIVE' : 'DOWN'}
        </span>
      </div>

      {/* Speed + adapter */}
      <div className="min-w-[120px] text-sm text-vmm-text-dim">
        {iface.speed_mbps ? `${iface.speed_mbps >= 1000 ? `${iface.speed_mbps / 1000} Gbps` : `${iface.speed_mbps} Mbps`}` : '—'}
      </div>

      {/* MTU */}
      <div className="min-w-[60px] text-sm text-vmm-text-dim text-right">
        {iface.mtu}
      </div>

      {/* Traffic */}
      <div className="text-right text-[11px] text-vmm-text-muted min-w-[120px]">
        <div>RX: {formatBytes(iface.rx_bytes)}</div>
        <div>TX: {formatBytes(iface.tx_bytes)}</div>
      </div>
    </div>
  )
}
