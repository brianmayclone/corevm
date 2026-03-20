import type { ReactNode } from 'react'

type Severity = 'info' | 'success' | 'warning' | 'danger'

const iconStyles: Record<Severity, string> = {
  info: 'bg-vmm-accent/20 text-vmm-accent',
  success: 'bg-vmm-success/20 text-vmm-success',
  warning: 'bg-vmm-warning/20 text-vmm-warning',
  danger: 'bg-vmm-danger/20 text-vmm-danger',
}

interface Props {
  icon: ReactNode
  severity?: Severity
  title: string | ReactNode
  subtitle: string
}

export default function ActivityRow({ icon, severity = 'info', title, subtitle }: Props) {
  return (
    <div className="flex items-center gap-4 px-5 py-3.5 border-b border-vmm-border last:border-b-0 hover:bg-vmm-surface-hover/50 transition-colors">
      <div className={`w-9 h-9 rounded-full flex items-center justify-center flex-shrink-0 ${iconStyles[severity]}`}>
        {icon}
      </div>
      <div className="min-w-0">
        <div className="text-sm text-vmm-text">{title}</div>
        <div className="text-xs text-vmm-text-muted">{subtitle}</div>
      </div>
    </div>
  )
}
