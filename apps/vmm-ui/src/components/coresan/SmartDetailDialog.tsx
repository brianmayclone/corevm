import { useEffect, useState } from 'react'
import type { SmartDetail } from '../../api/types'
import { useClusterStore } from '../../stores/clusterStore'
import Dialog from '../Dialog'

interface Props {
  open: boolean
  onClose: () => void
  deviceName: string
  hostId?: string
  hostName?: string
  sanAddress?: string
}

export default function SmartDetailDialog({ open, onClose, deviceName, hostId, hostName, sanAddress }: Props) {
  const { backendMode } = useClusterStore()
  const isCluster = backendMode === 'cluster'
  const [data, setData] = useState<SmartDetail | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    if (!open || !deviceName) return
    setLoading(true)
    setError('')

    const url = isCluster && hostId
      ? `/api/san/disks/${hostId}/${deviceName}/smart`
      : sanAddress
        ? `${sanAddress}/api/disks/${deviceName}/smart`
        : `${window.location.protocol}//${window.location.hostname}:7443/api/disks/${deviceName}/smart`

    const headers: HeadersInit = isCluster
      ? { Authorization: `Bearer ${localStorage.getItem('vmm_token') || ''}` }
      : {}

    fetch(url, { headers })
      .then(r => r.json())
      .then(d => { setData(d); setLoading(false) })
      .catch(e => { setError(e.message); setLoading(false) })
  }, [open, deviceName, hostId])

  const formatHours = (h: number | null) => {
    if (h == null) return 'N/A'
    const days = Math.floor(h / 24)
    const years = Math.floor(days / 365)
    const remDays = days % 365
    if (years > 0) return `${h.toLocaleString()}h (${years}y ${remDays}d)`
    if (days > 0) return `${h.toLocaleString()}h (${days}d)`
    return `${h}h`
  }

  return (
    <Dialog open={open} title={`S.M.A.R.T. Details: /dev/${deviceName}`} onClose={onClose}>
      {loading && <div className="py-8 text-center text-vmm-text-muted">Loading SMART data...</div>}
      {error && <div className="py-4 text-vmm-danger text-sm">{error}</div>}
      {data && !loading && (
        <div className="space-y-4">
          {/* Health banner */}
          {!data.supported ? (
            <div className="flex items-center gap-3 p-3 rounded-lg bg-vmm-surface border border-vmm-border">
              <span className="text-lg">&#x26A0;</span>
              <div>
                <div className="text-sm font-medium text-vmm-warning">SMART Not Supported</div>
                <div className="text-xs text-vmm-text-muted">
                  This disk does not support S.M.A.R.T. monitoring. Proactive failure detection is not available.
                  Consider using hardware RAID or redundant storage for data protection.
                </div>
              </div>
            </div>
          ) : data.health_passed === false ? (
            <div className="flex items-center gap-3 p-3 rounded-lg bg-vmm-danger/10 border border-vmm-danger/30">
              <span className="text-2xl">&#x274C;</span>
              <div>
                <div className="text-sm font-bold text-vmm-danger">SMART Health: FAILED</div>
                <div className="text-xs text-vmm-text-muted">
                  This disk is reporting imminent failure. Replace immediately and ensure data is backed up.
                </div>
              </div>
            </div>
          ) : (
            <div className="flex items-center gap-3 p-3 rounded-lg bg-vmm-success/10 border border-vmm-success/30">
              <span className="text-2xl">&#x2705;</span>
              <div>
                <div className="text-sm font-bold text-vmm-success">SMART Health: PASSED</div>
                <div className="text-xs text-vmm-text-muted">No critical issues detected.</div>
              </div>
            </div>
          )}

          {/* Device info */}
          {data.supported && (
            <>
              <div className="grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
                <div>
                  <span className="text-vmm-text-muted text-xs block">Model</span>
                  <span className="text-vmm-text font-mono text-xs">{data.model || 'N/A'}</span>
                </div>
                <div>
                  <span className="text-vmm-text-muted text-xs block">Serial</span>
                  <span className="text-vmm-text font-mono text-xs">{data.serial || 'N/A'}</span>
                </div>
                <div>
                  <span className="text-vmm-text-muted text-xs block">Firmware</span>
                  <span className="text-vmm-text font-mono text-xs">{data.firmware || 'N/A'}</span>
                </div>
                <div>
                  <span className="text-vmm-text-muted text-xs block">Transport</span>
                  <span className="text-vmm-text font-mono text-xs uppercase">{data.transport}</span>
                </div>
              </div>

              {/* Attributes */}
              <div className="space-y-2">
                <h3 className="text-xs font-bold text-vmm-text-muted uppercase tracking-wider">Health Attributes</h3>
                <div className="grid gap-2">
                  <AttrRow label="Power-On Hours" value={formatHours(data.power_on_hours)}
                    warn={data.power_on_hours != null && data.power_on_hours > 40000} />
                  <AttrRow label="Temperature" value={data.temperature_celsius != null ? `${data.temperature_celsius}\u00B0C` : 'N/A'}
                    warn={data.temperature_celsius != null && data.temperature_celsius > 55}
                    critical={data.temperature_celsius != null && data.temperature_celsius > 65} />
                  <AttrRow label="Reallocated Sectors" value={String(data.reallocated_sectors ?? 'N/A')}
                    warn={data.reallocated_sectors != null && data.reallocated_sectors > 0}
                    critical={data.reallocated_sectors != null && data.reallocated_sectors > 10} />
                  <AttrRow label="Pending Sectors" value={String(data.pending_sectors ?? 'N/A')}
                    warn={data.pending_sectors != null && data.pending_sectors > 0} />
                  <AttrRow label="Uncorrectable Sectors" value={String(data.uncorrectable_sectors ?? 'N/A')}
                    critical={data.uncorrectable_sectors != null && data.uncorrectable_sectors > 0} />
                  {data.wear_leveling_pct != null && (
                    <AttrRow label="Wear Leveling" value={`${data.wear_leveling_pct}%`}
                      warn={data.wear_leveling_pct < 20} critical={data.wear_leveling_pct < 5} />
                  )}
                  {data.media_errors != null && (
                    <AttrRow label="Media Errors (NVMe)" value={String(data.media_errors)}
                      critical={data.media_errors > 0} />
                  )}
                  {data.percentage_used != null && (
                    <AttrRow label="Endurance Used (NVMe)" value={`${data.percentage_used}%`}
                      warn={data.percentage_used > 80} critical={data.percentage_used > 95} />
                  )}
                </div>
              </div>

              <div className="text-[10px] text-vmm-text-muted pt-2 border-t border-vmm-border">
                {hostName && <span>Host: {hostName} &middot; </span>}
                Last collected: {data.collected_at ? new Date(data.collected_at).toLocaleString() : 'N/A'}
              </div>
            </>
          )}
        </div>
      )}
    </Dialog>
  )
}

function AttrRow({ label, value, warn, critical }: {
  label: string; value: string; warn?: boolean; critical?: boolean
}) {
  const color = critical ? 'text-vmm-danger font-bold' : warn ? 'text-vmm-warning font-medium' : 'text-vmm-text'
  return (
    <div className="flex items-center justify-between py-1.5 px-3 rounded-lg bg-vmm-bg/50 border border-vmm-border">
      <span className="text-xs text-vmm-text-muted">{label}</span>
      <span className={`text-xs font-mono ${color}`}>{value}</span>
    </div>
  )
}
