import { useEffect } from 'react'
import { AlertTriangle, Info, AlertCircle, XOctagon } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'

export default function EventsList() {
  const { events, fetchEvents } = useClusterStore()

  useEffect(() => { fetchEvents() }, [])

  const severityIcon = (severity: string) => {
    switch (severity) {
      case 'critical': return <XOctagon size={14} className="text-vmm-danger" />
      case 'error': return <AlertCircle size={14} className="text-vmm-danger" />
      case 'warning': return <AlertTriangle size={14} className="text-yellow-400" />
      default: return <Info size={14} className="text-vmm-accent" />
    }
  }

  return (
    <div className="space-y-5">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">Events</h1>
        <p className="text-sm text-vmm-text-muted mt-1">Cluster-wide event log</p>
      </div>

      <div className="space-y-1">
        {events.map(event => (
          <Card key={event.id}>
            <div className="px-3 py-2 flex items-start gap-2">
              <div className="mt-0.5">{severityIcon(event.severity)}</div>
              <div className="flex-1 min-w-0">
                <div className="text-sm text-vmm-text">{event.message}</div>
                <div className="flex items-center gap-2 mt-0.5 text-[10px] text-vmm-text-muted">
                  <span className="uppercase">{event.category}</span>
                  {event.target_type && <span>{event.target_type}:{event.target_id?.substring(0, 8)}</span>}
                </div>
              </div>
              <span className="text-[10px] text-vmm-text-muted whitespace-nowrap">
                {new Date(event.created_at).toLocaleString()}
              </span>
            </div>
          </Card>
        ))}
      </div>
    </div>
  )
}
