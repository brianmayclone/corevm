import type { ReactNode, ButtonHTMLAttributes } from 'react'

type Variant = 'primary' | 'danger' | 'ghost' | 'outline'
type Size = 'sm' | 'md' | 'lg' | 'icon'

const variantStyles: Record<Variant, string> = {
  primary: 'bg-vmm-accent hover:bg-vmm-accent-hover text-white',
  danger: 'bg-vmm-danger hover:bg-vmm-danger-hover text-white',
  ghost: 'bg-transparent hover:bg-vmm-surface-hover text-vmm-text-dim hover:text-vmm-text',
  outline: 'bg-transparent border border-vmm-border hover:border-vmm-border-light text-vmm-text-dim hover:text-vmm-text',
}

const sizeStyles: Record<Size, string> = {
  sm: 'px-3 py-1.5 text-xs',
  md: 'px-4 py-2 text-sm',
  lg: 'px-6 py-2.5 text-base',
  icon: 'p-2.5',
}

interface Props extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant
  size?: Size
  icon?: ReactNode
  children?: ReactNode
}

export default function Button({ variant = 'primary', size = 'md', icon, children, className = '', ...props }: Props) {
  return (
    <button
      className={`inline-flex items-center justify-center gap-2 rounded-lg font-medium transition-colors
        disabled:opacity-50 disabled:cursor-not-allowed cursor-pointer
        ${variantStyles[variant]} ${sizeStyles[size]} ${className}`}
      {...props}
    >
      {icon}
      {children}
    </button>
  )
}
