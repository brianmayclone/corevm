interface Props {
  label: string
  onClick: () => void
}

export default function QuickAction({ label, onClick }: Props) {
  return (
    <button
      onClick={onClick}
      className="w-full text-left px-4 py-2.5 text-sm text-vmm-text-dim hover:text-vmm-text
        hover:bg-vmm-surface-hover transition-colors cursor-pointer border-b border-vmm-border last:border-b-0"
    >
      {label}
    </button>
  )
}
