import type { ReactNode } from 'react'

interface Props {
  icon?: ReactNode
  title: string
  children: ReactNode
  className?: string
}

export default function SectionCard({ icon, title, children, className = '' }: Props) {
  return (
    <div className={`bg-vmm-surface border border-vmm-border rounded-xl p-6 ${className}`}>
      <h2 className="flex items-center gap-2.5 text-base font-semibold text-vmm-text mb-5">
        {icon && <span className="text-vmm-accent">{icon}</span>}
        {title}
      </h2>
      {children}
    </div>
  )
}
