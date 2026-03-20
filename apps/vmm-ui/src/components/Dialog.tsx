import type { ReactNode } from 'react'
import { X } from 'lucide-react'

interface Props {
  open: boolean
  onClose: () => void
  title: string
  children: ReactNode
  width?: string
}

export default function Dialog({ open, onClose, title, children, width = 'max-w-lg' }: Props) {
  if (!open) return null
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0 bg-black/60" onClick={onClose} />
      <div className={`relative bg-vmm-surface border border-vmm-border rounded-xl shadow-2xl ${width} w-full mx-4`}>
        <div className="flex items-center justify-between px-6 py-4 border-b border-vmm-border">
          <h2 className="text-base font-semibold text-vmm-text">{title}</h2>
          <button onClick={onClose} className="p-1 text-vmm-text-muted hover:text-vmm-text transition-colors cursor-pointer">
            <X size={18} />
          </button>
        </div>
        <div className="px-6 py-5">{children}</div>
      </div>
    </div>
  )
}
