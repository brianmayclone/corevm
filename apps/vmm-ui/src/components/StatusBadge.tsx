import type { VmState } from '../api/types'

const styles: Record<VmState, string> = {
  running: 'bg-vmm-success/20 text-vmm-success border-vmm-success/30',
  stopped: 'bg-vmm-danger/20 text-vmm-danger border-vmm-danger/30',
  paused: 'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30',
  stopping: 'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30',
}

const labels: Record<VmState, string> = {
  running: 'RUNNING',
  stopped: 'POWERED OFF',
  paused: 'PAUSED',
  stopping: 'STOPPING',
}

interface Props {
  state: VmState
  size?: 'sm' | 'md'
}

export default function StatusBadge({ state, size = 'md' }: Props) {
  const cls = size === 'sm'
    ? 'px-1.5 py-0.5 text-[10px]'
    : 'px-2.5 py-1 text-xs'
  return (
    <span className={`inline-flex items-center rounded font-bold border tracking-wider ${styles[state]} ${cls}`}>
      {labels[state]}
    </span>
  )
}
