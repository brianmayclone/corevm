import { useEffect, useState } from 'react'
import { Circle, Server, Clock, Workflow, Shield } from 'lucide-react'
import api from '../api/client'
import { useClusterStore } from '../stores/clusterStore'
import type { SystemInfoExtended } from '../api/types'

export default function StatusBar() {
  const [info, setInfo] = useState<SystemInfoExtended | null>(null)
  const [connected, setConnected] = useState(false)
  const { backendMode } = useClusterStore()

  useEffect(() => {
    const check = () => {
      api.get<SystemInfoExtended>('/api/system/info')
        .then(({ data }) => { setInfo(data); setConnected(true) })
        .catch(() => setConnected(false))
    }
    check()
    const interval = setInterval(check, 15000)
    return () => clearInterval(interval)
  }, [])

  const isCluster = backendMode === 'cluster'

  return (
    <footer className="h-8 bg-vmm-sidebar border-t border-vmm-border flex items-center px-3 sm:px-4 gap-3 sm:gap-6 text-[10px] sm:text-[11px] text-vmm-text-muted overflow-x-auto">
      <span className="flex items-center gap-1.5 flex-shrink-0">
        <Circle size={6} className={connected ? 'fill-vmm-success text-vmm-success' : 'fill-vmm-danger text-vmm-danger'} />
        <span className="hidden sm:inline">{isCluster ? 'Cluster:' : 'Server:'}</span> {connected ? 'Connected' : 'Offline'}
      </span>
      {info && (
        <>
          {isCluster && info.total_hosts !== undefined && (
            <span className="items-center gap-1.5 hidden md:flex flex-shrink-0">
              <Workflow size={11} />
              {info.online_hosts}/{info.total_hosts} hosts online
            </span>
          )}
          {!isCluster && (
            <span className="items-center gap-1.5 hidden md:flex flex-shrink-0">
              <Server size={11} />
              {info.platform}/{info.arch} &bull; {info.cpu_count} cores
            </span>
          )}
          {isCluster && (
            <span className="items-center gap-1.5 hidden md:flex flex-shrink-0">
              <Shield size={11} />
              HA Active
            </span>
          )}
          <span className="flex items-center gap-1.5 ml-auto flex-shrink-0">
            <Clock size={11} />
            v{info.version}
          </span>
        </>
      )}
    </footer>
  )
}
