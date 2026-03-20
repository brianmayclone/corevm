import type { ReactNode } from 'react'

interface Props {
  label: string
  children: ReactNode
  className?: string
}

export default function FormField({ label, children, className = '' }: Props) {
  return (
    <div className={className}>
      <label className="block text-[11px] font-semibold tracking-widest text-vmm-text-muted uppercase mb-2">
        {label}
      </label>
      {children}
    </div>
  )
}
