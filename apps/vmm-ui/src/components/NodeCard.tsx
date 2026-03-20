import { Server } from 'lucide-react'

interface Props {
  name: string
  ip: string
  status: 'online' | 'offline'
  detail: string
}

const statusStyle = {
  online: 'bg-vmm-success/20 text-vmm-success border-vmm-success/30',
  offline: 'bg-vmm-danger/20 text-vmm-danger border-vmm-danger/30',
}

export default function NodeCard({ name, ip, status, detail }: Props) {
  return (
    <div className={`bg-vmm-surface border rounded-xl p-4 flex items-center gap-4 transition-colors
      ${status === 'offline' ? 'border-vmm-warning/30' : 'border-vmm-border hover:border-vmm-border-light'}`}>
      <div className="w-10 h-10 rounded-lg bg-vmm-bg-alt flex items-center justify-center flex-shrink-0">
        <Server size={18} className="text-vmm-text-muted" />
      </div>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-sm font-semibold text-vmm-text truncate">{name}</span>
          <span className={`inline-flex items-center gap-1 px-2 py-0.5 text-[10px] font-bold tracking-wider rounded border ${statusStyle[status]}`}>
            <span className={`w-1.5 h-1.5 rounded-full ${status === 'online' ? 'bg-vmm-success' : 'bg-vmm-danger'}`} />
            {status.toUpperCase()}
          </span>
        </div>
        <div className="text-xs text-vmm-text-muted font-mono mt-0.5">{ip}</div>
      </div>
      <div className="text-xs text-vmm-text-muted text-right whitespace-nowrap">{detail}</div>
    </div>
  )
}
