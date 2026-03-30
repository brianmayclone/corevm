import { Monitor, Power, Trash2 } from 'lucide-react'
import ProgressBar from './ProgressBar'
import OsIcon from './OsIcon'
import StatusBadge from './StatusBadge'
import type { VmState } from '../api/types'

interface Props {
  name: string
  guestOs: string
  state: VmState
  tag: string
  cpuPercent: number
  ramPercent: number
  onClick?: () => void
  onConsole?: () => void
  onPower?: () => void
  onDelete?: () => void
}

export default function VmPriorityCard({ name, guestOs, state, tag, cpuPercent, ramPercent, onClick, onConsole, onPower, onDelete }: Props) {
  return (
    <div
      onClick={onClick}
      className={`bg-vmm-surface border border-vmm-border rounded-xl p-4 transition-colors
        ${onClick ? 'hover:border-vmm-border-light cursor-pointer' : ''}`}
    >
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-3">
          <OsIcon guestOs={guestOs} size={36} />
          <div>
            <div className="flex items-center gap-2">
              <span className="text-sm font-semibold text-vmm-text">{name}</span>
              <StatusBadge state={state} size="sm" />
            </div>
            <div className="text-[10px] text-vmm-text-muted font-mono tracking-wider">{tag}</div>
          </div>
        </div>
        <div className="flex items-center gap-1" onClick={(e) => e.stopPropagation()}>
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
          {onDelete && state === 'stopped' && (
            <button onClick={onDelete} className="p-1.5 hover:bg-vmm-danger/20 rounded text-vmm-text-muted hover:text-vmm-danger transition-colors cursor-pointer" title="Delete VM">
              <Trash2 size={13} />
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
