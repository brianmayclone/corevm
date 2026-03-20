interface Props {
  enabled: boolean
  onChange: (val: boolean) => void
  label: string
  description?: string
  icon?: React.ReactNode
}

export default function Toggle({ enabled, onChange, label, description, icon }: Props) {
  return (
    <div className="flex items-center justify-between py-3">
      <div className="flex items-center gap-3">
        {icon && <div className="text-vmm-accent">{icon}</div>}
        <div>
          <div className="text-sm font-medium text-vmm-text">{label}</div>
          {description && <div className="text-xs text-vmm-text-muted">{description}</div>}
        </div>
      </div>
      <button
        type="button"
        onClick={() => onChange(!enabled)}
        className={`relative w-11 h-6 rounded-full transition-colors cursor-pointer
          ${enabled ? 'bg-vmm-accent' : 'bg-vmm-border-light'}`}
      >
        <span className={`absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white transition-transform
          ${enabled ? 'translate-x-5' : 'translate-x-0'}`} />
      </button>
    </div>
  )
}
