import type { ReactNode } from 'react'

interface Props {
  children: ReactNode
  className?: string
  padding?: boolean
}

export default function Card({ children, className = '', padding = true }: Props) {
  return (
    <div className={`bg-vmm-surface border border-vmm-border rounded-xl ${padding ? 'p-5' : ''} ${className}`}>
      {children}
    </div>
  )
}
