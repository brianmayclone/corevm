import { Monitor, Power } from 'lucide-react'
import ProgressBar from './ProgressBar'

interface Props {
  name: string
  tag: string
  cpuPercent: number
  ramPercent: number
  onConsole?: () => void
  onPower?: () => void
}

export default function VmPriorityCard({ name, tag, cpuPercent, ramPercent, onConsole, onPower }: Props) {
  return (
    <div className="bg-vmm-surface border border-vmm-border rounded-xl p-4">
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-3">
          <div className="w-8 h-8 rounded-lg bg-vmm-bg-alt flex items-center justify-center">
            <Monitor size={14} className="text-vmm-text-muted" />
          </div>
          <div>
            <div className="text-sm font-semibold text-vmm-text">{name}</div>
            <div className="text-[10px] text-vmm-text-muted font-mono tracking-wider">{tag}</div>
          </div>
        </div>
        <div className="flex items-center gap-1">
          {onConsole && (
            <button onClick={onConsole} className="p-1.5 hover:bg-vmm-surface-hover rounded text-vmm-text-muted hover:text-vmm-text transition-colors cursor-pointer">
              <Monitor size={13} />
            </button>
          )}
          {onPower && (
            <button onClick={onPower} className="p-1.5 hover:bg-vmm-surface-hover rounded text-vmm-text-muted hover:text-vmm-text transition-colors cursor-pointer">
              <Power size={13} />
            </button>
          )}
        </div>
      </div>
      <div className="grid grid-cols-2 gap-4">
        <ProgressBar label="CPU USAGE" detail={`${cpuPercent}%`} value={cpuPercent} />
        <ProgressBar label="RAM USAGE" detail={`${ramPercent}%`} value={ramPercent} />
      </div>
    </div>
  )
}
