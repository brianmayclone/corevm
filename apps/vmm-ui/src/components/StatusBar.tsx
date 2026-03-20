import { Circle, Server, Wifi, Clock } from 'lucide-react'

interface Props {
  vmState?: 'running' | 'stopped' | string
  nodeName?: string
  latency?: string
  netIn?: string
  netOut?: string
}

export default function StatusBar({ vmState, nodeName, latency, netIn, netOut }: Props) {
  const isUp = vmState === 'running'
  return (
    <footer className="h-8 bg-vmm-sidebar border-t border-vmm-border flex items-center px-4 gap-6 text-[11px] text-vmm-text-muted">
      <span className="flex items-center gap-1.5">
        <Circle size={6} className={isUp ? 'fill-vmm-success text-vmm-success' : 'fill-vmm-danger text-vmm-danger'} />
        Uptime Status: {isUp ? 'Up' : 'Down'}
      </span>
      {nodeName && (
        <span className="flex items-center gap-1.5">
          <Server size={11} /> Hypervisor Node: {nodeName}
        </span>
      )}
      {latency && (
        <span className="flex items-center gap-1.5 ml-auto">
          <Clock size={11} /> Latency: {latency}
        </span>
      )}
      {(netIn || netOut) && (
        <span className="flex items-center gap-1.5">
          <Wifi size={11} /> Network: {netIn || '0.0 B/s'} In &bull; {netOut || '0.0 B/s'} Out
        </span>
      )}
    </footer>
  )
}
