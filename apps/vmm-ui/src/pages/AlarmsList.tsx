import { useEffect } from 'react'
import { Bell, AlertTriangle, Check } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'

export default function AlarmsList() {
  const { alarms, fetchAlarms, acknowledgeAlarm } = useClusterStore()

  useEffect(() => { fetchAlarms() }, [])

  const active = alarms.filter(a => a.triggered && !a.acknowledged)
  const acknowledged = alarms.filter(a => a.acknowledged)
  const cleared = alarms.filter(a => !a.triggered && !a.acknowledged)

  const severityColor = (s: string) => {
    switch (s) {
      case 'critical': return 'text-vmm-danger bg-vmm-danger/10'
      case 'warning': return 'text-yellow-400 bg-yellow-500/10'
      default: return 'text-vmm-text-muted bg-vmm-surface'
    }
  }

  return (
    <div className="space-y-5">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">Alarms</h1>
        <p className="text-sm text-vmm-text-muted mt-1">
          {active.length} active &bull; {acknowledged.length} acknowledged &bull; {cleared.length} cleared
        </p>
      </div>

      {active.length > 0 && (
        <div className="space-y-2">
          <h3 className="text-sm font-semibold text-vmm-danger flex items-center gap-2">
            <AlertTriangle size={14} /> Active Alarms
          </h3>
          {active.map(alarm => (
            <Card key={alarm.id}>
              <div className="p-3 flex items-center gap-3">
                <span className={`text-xs px-2 py-0.5 rounded-full font-medium uppercase ${severityColor(alarm.severity)}`}>
                  {alarm.severity}
                </span>
                <div className="flex-1">
                  <div className="text-sm font-medium text-vmm-text">{alarm.name}</div>
                  <div className="text-xs text-vmm-text-muted">
                    {alarm.condition_type} on {alarm.target_type}:{alarm.target_id.substring(0, 8)}
                    {alarm.threshold && ` (threshold: ${alarm.threshold}%)`}
                  </div>
                  {alarm.triggered_at && (
                    <div className="text-[10px] text-vmm-text-muted mt-0.5">
                      Triggered: {new Date(alarm.triggered_at).toLocaleString()}
                    </div>
                  )}
                </div>
                <button
                  onClick={() => acknowledgeAlarm(alarm.id)}
                  className="flex items-center gap-1 px-3 py-1.5 bg-vmm-surface hover:bg-vmm-surface-hover rounded-lg text-xs font-medium text-vmm-text-muted"
                >
                  <Check size={12} /> Acknowledge
                </button>
              </div>
            </Card>
          ))}
        </div>
      )}

      {active.length === 0 && (
        <div className="text-center py-12">
          <Bell size={32} className="mx-auto mb-3 text-vmm-text-muted opacity-30" />
          <div className="text-vmm-text-muted">No active alarms</div>
        </div>
      )}

      {acknowledged.length > 0 && (
        <div className="space-y-2">
          <h3 className="text-sm font-semibold text-vmm-text-muted">Acknowledged</h3>
          {acknowledged.map(alarm => (
            <Card key={alarm.id}>
              <div className="p-3 flex items-center gap-3 opacity-60">
                <span className="text-xs px-2 py-0.5 rounded-full bg-vmm-surface text-vmm-text-muted uppercase">{alarm.severity}</span>
                <div className="flex-1">
                  <div className="text-sm text-vmm-text">{alarm.name}</div>
                  <div className="text-xs text-vmm-text-muted">{alarm.condition_type} on {alarm.target_type}</div>
                </div>
                <Check size={14} className="text-vmm-success" />
              </div>
            </Card>
          ))}
        </div>
      )}
    </div>
  )
}
