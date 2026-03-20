interface Props {
  children: string
  className?: string
}

export default function SectionLabel({ children, className = '' }: Props) {
  return (
    <h3 className={`text-[11px] font-semibold tracking-widest text-vmm-text-muted uppercase ${className}`}>
      {children}
    </h3>
  )
}
