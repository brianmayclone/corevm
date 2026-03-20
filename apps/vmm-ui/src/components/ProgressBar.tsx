interface Props {
  value: number   // 0–100
  label?: string
  detail?: string
  color?: string
}

export default function ProgressBar({ value, label, detail, color = 'bg-vmm-accent' }: Props) {
  return (
    <div>
      {(label || detail) && (
        <div className="flex justify-between items-baseline mb-1.5">
          {label && <span className="text-xs text-vmm-text-muted">{label}</span>}
          {detail && <span className="text-xs font-medium text-vmm-text">{detail}</span>}
        </div>
      )}
      <div className="w-full h-1.5 bg-vmm-border rounded-full overflow-hidden">
        <div className={`h-full rounded-full transition-all duration-300 ${color}`}
          style={{ width: `${Math.min(100, Math.max(0, value))}%` }} />
      </div>
    </div>
  )
}
