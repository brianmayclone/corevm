import type { InputHTMLAttributes } from 'react'

interface Props extends InputHTMLAttributes<HTMLInputElement> {}

export default function TextInput({ className = '', ...props }: Props) {
  return (
    <input
      className={`w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-4 py-3 text-sm text-vmm-text
        placeholder-vmm-text-muted focus:outline-none focus:border-vmm-accent/50 transition-colors ${className}`}
      {...props}
    />
  )
}
