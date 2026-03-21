import { useEffect } from 'react'
import { Activity, CheckCircle, XCircle, Clock, Loader } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'

export default function TasksList() {
  const { tasks, fetchTasks } = useClusterStore()

  useEffect(() => { fetchTasks(); const i = setInterval(fetchTasks, 5000); return () => clearInterval(i) }, [])

  const statusIcon = (status: string) => {
    switch (status) {
      case 'completed': return <CheckCircle size={14} className="text-vmm-success" />
      case 'failed': return <XCircle size={14} className="text-vmm-danger" />
      case 'running': return <Loader size={14} className="text-vmm-accent animate-spin" />
      default: return <Clock size={14} className="text-vmm-text-muted" />
    }
  }

  return (
    <div className="space-y-5">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">Tasks</h1>
        <p className="text-sm text-vmm-text-muted mt-1">Long-running cluster operations</p>
      </div>

      <div className="space-y-2">
        {tasks.map(task => (
          <Card key={task.id}>
            <div className="p-3 flex items-center gap-3">
              {statusIcon(task.status)}
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <span className="text-sm font-medium text-vmm-text">{task.task_type.replace('.', ' ').replace('_', ' ')}</span>
                  <span className="text-xs px-2 py-0.5 bg-vmm-surface rounded-full text-vmm-text-muted">{task.target_type}: {task.target_id.substring(0, 8)}</span>
                </div>
                {task.error && <div className="text-xs text-vmm-danger mt-0.5">{task.error}</div>}
              </div>
              {task.status === 'running' && (
                <div className="w-20">
                  <div className="w-full bg-vmm-bg rounded-full h-1.5">
                    <div className="h-1.5 rounded-full bg-vmm-accent transition-all" style={{ width: `${task.progress_pct}%` }} />
                  </div>
                  <div className="text-[10px] text-vmm-text-muted text-center mt-0.5">{task.progress_pct}%</div>
                </div>
              )}
              <span className="text-[10px] text-vmm-text-muted whitespace-nowrap">
                {new Date(task.created_at).toLocaleTimeString()}
              </span>
            </div>
          </Card>
        ))}
        {tasks.length === 0 && (
          <div className="text-center text-vmm-text-muted py-12">
            <Activity size={32} className="mx-auto mb-3 opacity-30" />
            No recent tasks
          </div>
        )}
      </div>
    </div>
  )
}
