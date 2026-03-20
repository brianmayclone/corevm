import type { ReactNode } from 'react'

interface Props {
  icon?: ReactNode
  label: string
  value: string | ReactNode
}

export default function SpecRow({ icon, label, value }: Props) {
  return (
    <div className="flex items-start gap-3 py-2.5">
      {icon && <div className="text-vmm-text-muted mt-0.5">{icon}</div>}
      <div>
        <div className="text-xs text-vmm-text-muted">{label}</div>
        <div className="text-sm font-medium text-vmm-text">{value}</div>
      </div>
    </div>
  )
}
