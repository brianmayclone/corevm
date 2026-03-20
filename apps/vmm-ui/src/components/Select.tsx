import type { SelectHTMLAttributes } from 'react'

interface Option {
  value: string
  label: string
}

interface Props extends SelectHTMLAttributes<HTMLSelectElement> {
  options: Option[]
}

export default function Select({ options, className = '', ...props }: Props) {
  return (
    <select
      className={`w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-4 py-3 text-sm text-vmm-text
        focus:outline-none focus:border-vmm-accent/50 transition-colors appearance-none
        bg-[url('data:image/svg+xml;charset=utf-8,%3Csvg%20xmlns%3D%22http%3A%2F%2Fwww.w3.org%2F2000%2Fsvg%22%20width%3D%2212%22%20height%3D%2212%22%20viewBox%3D%220%200%2024%2024%22%20fill%3D%22none%22%20stroke%3D%22%236B7280%22%20stroke-width%3D%222%22%3E%3Cpolyline%20points%3D%226%209%2012%2015%2018%209%22%2F%3E%3C%2Fsvg%3E')]
        bg-no-repeat bg-[right_12px_center] pr-10 cursor-pointer ${className}`}
      {...props}
    >
      {options.map((o) => (
        <option key={o.value} value={o.value}>{o.label}</option>
      ))}
    </select>
  )
}
