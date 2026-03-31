import { useState } from 'react'
import { NavLink } from 'react-router-dom'
import { ChevronDown, ChevronRight } from 'lucide-react'

export interface TreeNodeProps {
  icon: React.ElementType
  label: string
  to?: string
  statusDot?: 'green' | 'yellow' | 'red' | 'gray'
  children?: React.ReactNode
  defaultExpanded?: boolean
  onNavigate?: () => void
  depth?: number
  count?: number
}

const dotColors: Record<string, string> = {
  green: 'bg-emerald-400',
  yellow: 'bg-amber-400',
  red: 'bg-red-400',
  gray: 'bg-gray-500',
}

export default function TreeNode({ icon: Icon, label, to, statusDot, children, defaultExpanded = false, onNavigate, depth = 0, count }: TreeNodeProps) {
  const [expanded, setExpanded] = useState(defaultExpanded)
  const hasChildren = !!children

  const content = (
    <span className="flex items-center gap-1.5 min-w-0 flex-1">
      {statusDot && <span className={`w-1.5 h-1.5 rounded-full flex-shrink-0 ${dotColors[statusDot]}`} />}
      <Icon size={14} className="flex-shrink-0 opacity-70" />
      <span className="truncate">{label}</span>
      {count !== undefined && (
        <span className="ml-auto text-[10px] text-vmm-text-muted bg-vmm-bg/50 px-1.5 py-0.5 rounded-full font-mono flex-shrink-0">
          {count}
        </span>
      )}
    </span>
  )

  const paddingLeft = depth * 14 + 8

  if (to && !hasChildren) {
    return (
      <NavLink
        to={to}
        onClick={onNavigate}
        style={{ paddingLeft }}
        className={({ isActive }) =>
          `flex items-center py-1 pr-2 rounded-md text-[12px] transition-colors
          ${isActive
            ? 'bg-vmm-accent/10 text-vmm-accent'
            : 'text-vmm-text-muted hover:text-vmm-text hover:bg-vmm-surface-hover'
          }`
        }
      >
        {content}
      </NavLink>
    )
  }

  return (
    <div>
      <button
        onClick={() => {
          if (hasChildren) setExpanded(!expanded)
        }}
        style={{ paddingLeft }}
        className="w-full flex items-center gap-0.5 py-1 pr-2 rounded-md text-[12px] text-vmm-text-dim hover:text-vmm-text hover:bg-vmm-surface-hover transition-colors cursor-pointer"
      >
        {hasChildren ? (
          expanded
            ? <ChevronDown size={12} className="flex-shrink-0 opacity-50" />
            : <ChevronRight size={12} className="flex-shrink-0 opacity-50" />
        ) : (
          <span className="w-3 flex-shrink-0" />
        )}
        {content}
      </button>
      {expanded && hasChildren && (
        <div className="relative">
          <div
            className="absolute top-0 bottom-0 border-l border-vmm-border/40"
            style={{ left: depth * 14 + 14 }}
          />
          {children}
        </div>
      )}
    </div>
  )
}
