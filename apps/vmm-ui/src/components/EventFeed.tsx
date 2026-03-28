/**
 * Reusable event feed component — shows recent events filtered by scope.
 * Use on Host detail pages, VM detail, Cluster overview, CoreSAN pages, etc.
 */
import { useEffect, useState } from 'react'
import { AlertTriangle, Info, AlertCircle, XOctagon } from 'lucide-react'
import api from '../api/client'
import Card from './Card'

interface Event {
  id: number
  severity: string
  category: string
  message: string
  target_type: string | null
  target_id: string | null
  host_id: string | null
  created_at: string
}

interface EventFeedProps {
  /** Filter by category: "san", "vm", "server", "disk", "network", "cluster" */
  category?: string
  /** Filter by host_id */
  hostId?: string
  /** Filter by target_id (e.g. VM UUID) */
  targetId?: string
  /** Max events to show */
  limit?: number
  /** Title shown above the feed */
  title?: string
  /** Compact mode: smaller, no card wrapper */
  compact?: boolean
}

export default function EventFeed({ category, hostId, targetId, limit = 10, title = 'Recent Events', compact }: EventFeedProps) {
  const [events, setEvents] = useState<Event[]>([])

  useEffect(() => {
    const params = new URLSearchParams()
    params.set('limit', String(limit))
    if (category) params.set('category', category)

    api.get<Event[]>(`/api/events?${params}`).then(({ data }) => {
      let filtered = data
      if (hostId) filtered = filtered.filter(e => e.host_id === hostId)
      if (targetId) filtered = filtered.filter(e => e.target_id === targetId)
      setEvents(filtered)
    }).catch(() => {})
  }, [category, hostId, targetId, limit])

  const severityIcon = (severity: string) => {
    switch (severity) {
      case 'critical': return <XOctagon size={compact ? 12 : 14} className="text-vmm-danger" />
      case 'warning': return <AlertTriangle size={compact ? 12 : 14} className="text-yellow-400" />
      default: return <Info size={compact ? 12 : 14} className="text-vmm-accent" />
    }
  }

  const severityBg = (severity: string) => {
    switch (severity) {
      case 'critical': return 'border-l-2 border-l-vmm-danger'
      case 'warning': return 'border-l-2 border-l-yellow-400'
      default: return 'border-l-2 border-l-transparent'
    }
  }

  if (events.length === 0) return null

  const content = (
    <div className="space-y-0.5">
      {!compact && <h3 className="text-xs font-bold text-vmm-text-muted uppercase tracking-wider mb-2">{title}</h3>}
      {events.map(event => (
        <div key={event.id} className={`flex items-start gap-2 px-2 py-1.5 rounded ${severityBg(event.severity)} ${compact ? 'text-[11px]' : 'text-xs'}`}>
          <div className="mt-0.5 shrink-0">{severityIcon(event.severity)}</div>
          <div className="flex-1 min-w-0">
            <span className="text-vmm-text">{event.message}</span>
            <div className="flex items-center gap-2 text-vmm-text-muted" style={{ fontSize: compact ? '9px' : '10px' }}>
              <span className="uppercase">{event.category}</span>
              <span>{new Date(event.created_at).toLocaleString()}</span>
            </div>
          </div>
        </div>
      ))}
    </div>
  )

  if (compact) return content

  return <Card>{content}</Card>
}
