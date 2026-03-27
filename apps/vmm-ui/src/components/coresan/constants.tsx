export const fttLabels: Record<number, string> = {
  0: 'FTT=0 (No Protection)',
  1: 'FTT=1 (1 Failure)',
  2: 'FTT=2 (2 Failures)',
}

export const fttColors: Record<number, string> = {
  0: 'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30',
  1: 'bg-vmm-success/20 text-vmm-success border-vmm-success/30',
  2: 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30',
}

export const raidLabels: Record<string, string> = {
  stripe: 'Stripe (RAID-0)',
  mirror: 'Mirror (RAID-1)',
  stripe_mirror: 'Stripe+Mirror (RAID-10)',
}

export const statusColors: Record<string, string> = {
  online: 'bg-vmm-success/20 text-vmm-success border-vmm-success/30',
  degraded: 'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30',
  offline: 'bg-vmm-danger/20 text-vmm-danger border-vmm-danger/30',
  creating: 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30',
  draining: 'bg-vmm-warning/20 text-vmm-warning border-vmm-warning/30',
  connecting: 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30',
}

export function Badge({ label, color }: { label: string; color: string }) {
  return (
    <span className={`inline-flex items-center px-2 py-0.5 rounded text-[10px] font-bold border tracking-wider uppercase ${color}`}>
      {label}
    </span>
  )
}
