import type { ReactNode } from 'react'

interface Props {
  icon: ReactNode
  title: string
  subtitle: string
}

export default function ActivityCard({ icon, title, subtitle }: Props) {
  return (
    <div className="flex items-center gap-3 bg-vmm-bg-alt border border-vmm-border rounded-lg p-4">
      <div className="flex-shrink-0 w-10 h-10 rounded-lg bg-vmm-surface flex items-center justify-center text-vmm-text-muted">
        {icon}
      </div>
      <div>
        <div className="text-sm font-medium text-vmm-text">{title}</div>
        <div className="text-xs text-vmm-text-muted">{subtitle}</div>
      </div>
    </div>
  )
}
