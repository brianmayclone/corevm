import { useEffect, useState } from 'react'
import { Circle, Server, Clock } from 'lucide-react'
import api from '../api/client'
import type { SystemInfo } from '../api/types'

export default function StatusBar() {
  const [info, setInfo] = useState<SystemInfo | null>(null)
  const [connected, setConnected] = useState(false)

  useEffect(() => {
    const check = () => {
      api.get<SystemInfo>('/api/system/info')
        .then(({ data }) => { setInfo(data); setConnected(true) })
        .catch(() => setConnected(false))
    }
    check()
    const interval = setInterval(check, 15000)
    return () => clearInterval(interval)
  }, [])

  return (
    <footer className="h-8 bg-vmm-sidebar border-t border-vmm-border flex items-center px-3 sm:px-4 gap-3 sm:gap-6 text-[10px] sm:text-[11px] text-vmm-text-muted overflow-x-auto">
      <span className="flex items-center gap-1.5 flex-shrink-0">
        <Circle size={6} className={connected ? 'fill-vmm-success text-vmm-success' : 'fill-vmm-danger text-vmm-danger'} />
        <span className="hidden sm:inline">Server:</span> {connected ? 'Connected' : 'Offline'}
      </span>
      {info && (
        <>
          <span className="items-center gap-1.5 hidden md:flex flex-shrink-0">
            <Server size={11} />
            {info.platform}/{info.arch} &bull; {info.cpu_count} cores
          </span>
          <span className="flex items-center gap-1.5 ml-auto flex-shrink-0">
            <Clock size={11} />
            v{info.version}
          </span>
        </>
      )}
    </footer>
  )
}
