import type { TextareaHTMLAttributes } from 'react'

interface Props extends TextareaHTMLAttributes<HTMLTextAreaElement> {}

export default function TextArea({ className = '', ...props }: Props) {
  return (
    <textarea
      className={`w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-4 py-3 text-sm text-vmm-text
        placeholder-vmm-text-muted focus:outline-none focus:border-vmm-accent/50 transition-colors
        resize-none ${className}`}
      {...props}
    />
  )
}
