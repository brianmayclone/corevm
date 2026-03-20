import type { ReactNode } from 'react'

interface Props {
  label: string
  value: string
  subtitle?: string
  icon?: ReactNode
  progress?: number  // 0–100
  progressColor?: string
}

export default function MetricCard({ label, value, subtitle, icon, progress, progressColor = 'bg-vmm-accent' }: Props) {
  return (
    <div className="bg-vmm-surface border border-vmm-border rounded-xl p-5 flex flex-col justify-between min-h-[120px]">
      <div className="flex items-start justify-between">
        <span className="text-[11px] font-semibold tracking-widest text-vmm-text-muted uppercase">{label}</span>
        {icon && <div className="text-vmm-text-muted opacity-40">{icon}</div>}
      </div>
      <div>
        <div className="text-3xl font-bold text-vmm-text leading-tight">{value}</div>
        {subtitle && <div className="text-xs text-vmm-text-muted mt-0.5">{subtitle}</div>}
      </div>
      {progress !== undefined && (
        <div className="w-full h-1.5 bg-vmm-border rounded-full overflow-hidden mt-3">
          <div className={`h-full rounded-full ${progressColor}`}
            style={{ width: `${Math.min(100, Math.max(0, progress))}%` }} />
        </div>
      )}
    </div>
  )
}
