interface Tab {
  id: string
  label: string
}

interface Props {
  tabs: Tab[]
  active: string
  onChange: (id: string) => void
}

export default function TabBar({ tabs, active, onChange }: Props) {
  return (
    <div className="flex gap-1 border-b border-vmm-border overflow-x-auto scrollbar-none -mx-3 px-3 sm:mx-0 sm:px-0">
      {tabs.map((tab) => (
        <button
          key={tab.id}
          onClick={() => onChange(tab.id)}
          className={`px-3 sm:px-4 py-2 sm:py-2.5 text-xs sm:text-sm font-medium transition-colors cursor-pointer whitespace-nowrap flex-shrink-0
            ${active === tab.id
              ? 'text-vmm-text border-b-2 border-vmm-accent'
              : 'text-vmm-text-muted hover:text-vmm-text-dim'
            }`}
        >
          {tab.label}
        </button>
      ))}
    </div>
  )
}
