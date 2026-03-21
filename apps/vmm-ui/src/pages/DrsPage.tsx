import { useEffect } from 'react'
import { Activity, ArrowRight, Check, X } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'

export default function DrsPage() {
  const { drsRecommendations, fetchDrsRecommendations, applyDrsRecommendation, dismissDrsRecommendation } = useClusterStore()

  useEffect(() => { fetchDrsRecommendations() }, [])

  const priorityColor = (p: string) => {
    switch (p) {
      case 'high': return 'bg-vmm-danger/10 text-vmm-danger'
      case 'critical': return 'bg-vmm-danger/20 text-vmm-danger'
      default: return 'bg-yellow-500/10 text-yellow-400'
    }
  }

  return (
    <div className="space-y-5">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">DRS Recommendations</h1>
        <p className="text-sm text-vmm-text-muted mt-1">Resource balancing suggestions from the scheduler</p>
      </div>

      <div className="space-y-3">
        {drsRecommendations.map(rec => (
          <Card key={rec.id}>
            <div className="p-4">
              <div className="flex items-center justify-between mb-2">
                <div className="flex items-center gap-2">
                  <span className={`text-xs px-2 py-0.5 rounded-full font-medium uppercase ${priorityColor(rec.priority)}`}>
                    {rec.priority}
                  </span>
                  <span className="text-sm font-medium text-vmm-text">Move VM "{rec.vm_name}"</span>
                </div>
                <div className="flex gap-2">
                  <button onClick={() => applyDrsRecommendation(rec.id)}
                    className="flex items-center gap-1 px-3 py-1.5 bg-vmm-accent/10 text-vmm-accent hover:bg-vmm-accent/20 rounded-lg text-xs font-medium">
                    <Check size={12} /> Apply
                  </button>
                  <button onClick={() => dismissDrsRecommendation(rec.id)}
                    className="flex items-center gap-1 px-3 py-1.5 bg-vmm-surface text-vmm-text-muted hover:text-vmm-text rounded-lg text-xs font-medium">
                    <X size={12} /> Dismiss
                  </button>
                </div>
              </div>
              <div className="flex items-center gap-2 text-sm text-vmm-text-dim">
                <span>{rec.source_host_name}</span>
                <ArrowRight size={14} className="text-vmm-text-muted" />
                <span>{rec.target_host_name}</span>
              </div>
              <p className="text-xs text-vmm-text-muted mt-1">{rec.reason}</p>
            </div>
          </Card>
        ))}
        {drsRecommendations.length === 0 && (
          <div className="text-center text-vmm-text-muted py-12">
            <Activity size={32} className="mx-auto mb-3 opacity-30" />
            No recommendations — cluster is balanced
          </div>
        )}
      </div>
    </div>
  )
}
