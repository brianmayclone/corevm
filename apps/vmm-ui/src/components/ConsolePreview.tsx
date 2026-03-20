import type { ReactNode } from 'react'
import { MonitorOff } from 'lucide-react'

interface Props {
  state: 'off' | 'connecting' | 'live'
  children?: ReactNode
  onOpenConsole?: () => void
}

export default function ConsolePreview({ state, children, onOpenConsole }: Props) {
  return (
    <div className="bg-vmm-console-bg rounded-xl overflow-hidden border border-vmm-border aspect-video flex items-center justify-center relative">
      {state === 'live' && children ? (
        children
      ) : (
        <div className="flex flex-col items-center gap-4 text-vmm-text-muted">
          <div className="w-16 h-16 rounded-2xl bg-vmm-surface/50 flex items-center justify-center">
            <MonitorOff size={28} />
          </div>
          <div className="text-center">
            <div className="text-base font-medium text-vmm-text-dim">No Console Session</div>
            <div className="text-sm mt-1">
              {state === 'connecting' ? 'Connecting...' : (
                <>The virtual machine is currently powered off.<br/>Start the machine to initialize the console.</>
              )}
            </div>
          </div>
          {onOpenConsole && (
            <button
              onClick={onOpenConsole}
              className="mt-2 px-5 py-2 border border-vmm-border-light rounded-lg text-sm
                text-vmm-text-dim hover:text-vmm-text hover:bg-vmm-surface-hover
                transition-colors flex items-center gap-2 cursor-pointer"
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
                <path d="M18 13v6a2 2 0 01-2 2H5a2 2 0 01-2-2V8a2 2 0 012-2h6"/><polyline points="15 3 21 3 21 9"/><line x1="10" y1="14" x2="21" y2="3"/>
              </svg>
              Open Web Console
            </button>
          )}
        </div>
      )}
    </div>
  )
}
