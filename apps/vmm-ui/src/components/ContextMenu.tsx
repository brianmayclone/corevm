/** Reusable dropdown context menu. */
import { useEffect, useRef, useState } from 'react'
import { MoreVertical } from 'lucide-react'

export interface MenuItem {
  label: string
  icon?: React.ReactNode
  danger?: boolean
  onClick: () => void
}

interface Props {
  items: MenuItem[]
}

export default function ContextMenu({ items }: Props) {
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    document.addEventListener('mousedown', handler)
    return () => document.removeEventListener('mousedown', handler)
  }, [])

  return (
    <div ref={ref} className="relative">
      <button
        onClick={(e) => { e.stopPropagation(); setOpen(!open) }}
        className="p-1.5 text-vmm-text-muted hover:text-vmm-text transition-colors cursor-pointer rounded hover:bg-vmm-surface-hover"
      >
        <MoreVertical size={16} />
      </button>

      {open && (
        <div className="absolute right-0 top-full mt-1 z-50 min-w-[160px] bg-vmm-surface border border-vmm-border rounded-lg shadow-xl py-1">
          {items.map((item, i) => (
            <button
              key={i}
              onClick={(e) => { e.stopPropagation(); item.onClick(); setOpen(false) }}
              className={`w-full text-left px-4 py-2 text-sm flex items-center gap-2 cursor-pointer transition-colors
                ${item.danger
                  ? 'text-vmm-danger hover:bg-vmm-danger/10'
                  : 'text-vmm-text hover:bg-vmm-surface-hover'}`}
            >
              {item.icon}
              {item.label}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}
