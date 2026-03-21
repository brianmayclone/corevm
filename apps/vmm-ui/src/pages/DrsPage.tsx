import { useEffect, useState } from 'react'
import { Activity, ArrowRight, Check, X, Plus, Settings, Trash2, Power, ToggleLeft, ToggleRight } from 'lucide-react'
import api from '../api/client'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import type { Cluster } from '../api/types'

interface DrsRule {
  id: number
  cluster_id: string
  name: string
  enabled: boolean
  metric: string
  threshold: number
  action: string
  cooldown_secs: number
  priority: string
  created_at: string
}

export default function DrsPage() {
  const { drsRecommendations, fetchDrsRecommendations, applyDrsRecommendation, dismissDrsRecommendation, clusters, fetchClusters } = useClusterStore()
  const [rules, setRules] = useState<DrsRule[]>([])
  const [showCreateRule, setShowCreateRule] = useState(false)
  const [newRule, setNewRule] = useState({ cluster_id: '', name: '', metric: 'cpu_usage', threshold: 80, action: 'recommend', cooldown_secs: 3600, priority: 'medium' })

  const fetchRules = () => api.get<DrsRule[]>('/api/drs/rules').then(({ data }) => setRules(data)).catch(() => {})

  useEffect(() => {
    fetchDrsRecommendations()
    fetchRules()
    fetchClusters()
  }, [])

  useEffect(() => {
    if (clusters.length > 0 && !newRule.cluster_id) setNewRule(r => ({ ...r, cluster_id: clusters[0].id }))
  }, [clusters])

  const handleCreateRule = async (e: React.FormEvent) => {
    e.preventDefault()
    await api.post('/api/drs/rules', newRule)
    setShowCreateRule(false)
    setNewRule({ cluster_id: clusters[0]?.id || '', name: '', metric: 'cpu_usage', threshold: 80, action: 'recommend', cooldown_secs: 3600, priority: 'medium' })
    fetchRules()
  }

  const toggleRule = async (rule: DrsRule) => {
    await api.put(`/api/drs/rules/${rule.id}`, { enabled: !rule.enabled })
    fetchRules()
  }

  const deleteRule = async (id: number) => {
    if (!confirm('Delete this DRS rule?')) return
    await api.delete(`/api/drs/rules/${id}`)
    fetchRules()
  }

  const priorityColor = (p: string) => {
    switch (p) {
      case 'high': return 'bg-vmm-danger/10 text-vmm-danger'
      case 'critical': return 'bg-vmm-danger/20 text-vmm-danger'
      default: return 'bg-yellow-500/10 text-yellow-400'
    }
  }

  const metricLabel = (m: string) => {
    switch (m) {
      case 'cpu_usage': return 'CPU Usage'
      case 'ram_usage': return 'RAM Usage'
      case 'vm_count_imbalance': return 'VM Count Imbalance'
      default: return m
    }
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">DRS — Distributed Resource Scheduler</h1>
        <p className="text-sm text-vmm-text-muted mt-1">Configure rules and review migration recommendations</p>
      </div>

      {/* ── Rules Section ──────────────────────────────────────────── */}
      <div>
        <div className="flex items-center justify-between mb-3">
          <SectionLabel>Rules</SectionLabel>
          <button onClick={() => setShowCreateRule(true)}
            className="flex items-center gap-2 px-3 py-1.5 bg-vmm-accent/10 text-vmm-accent hover:bg-vmm-accent/20 rounded-lg text-xs font-medium">
            <Plus size={12} /> Add Rule
          </button>
        </div>

        {showCreateRule && (
          <Card>
            <form onSubmit={handleCreateRule} className="p-4 space-y-3">
              <div className="grid grid-cols-2 sm:grid-cols-3 gap-3">
                <input type="text" value={newRule.name} onChange={e => setNewRule({ ...newRule, name: e.target.value })}
                  placeholder="Rule name" required
                  className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                <select value={newRule.cluster_id} onChange={e => setNewRule({ ...newRule, cluster_id: e.target.value })}
                  className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  {clusters.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
                </select>
                <select value={newRule.metric} onChange={e => setNewRule({ ...newRule, metric: e.target.value })}
                  className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  <option value="cpu_usage">CPU Usage</option>
                  <option value="ram_usage">RAM Usage</option>
                  <option value="vm_count_imbalance">VM Count Imbalance</option>
                </select>
                <div className="flex items-center gap-2">
                  <span className="text-xs text-vmm-text-muted whitespace-nowrap">Threshold:</span>
                  <input type="number" min={1} max={100} value={newRule.threshold}
                    onChange={e => setNewRule({ ...newRule, threshold: parseInt(e.target.value) || 80 })}
                    className="w-20 px-2 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                  <span className="text-xs text-vmm-text-muted">%</span>
                </div>
                <select value={newRule.action} onChange={e => setNewRule({ ...newRule, action: e.target.value })}
                  className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  <option value="recommend">Recommend (manual)</option>
                  <option value="auto_migrate">Auto-migrate</option>
                </select>
                <select value={newRule.priority} onChange={e => setNewRule({ ...newRule, priority: e.target.value })}
                  className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  <option value="low">Low</option>
                  <option value="medium">Medium</option>
                  <option value="high">High</option>
                  <option value="critical">Critical</option>
                </select>
              </div>
              <div className="flex gap-2 justify-end">
                <button type="button" onClick={() => setShowCreateRule(false)}
                  className="px-3 py-1.5 text-sm text-vmm-text-muted">Cancel</button>
                <button type="submit" className="px-4 py-1.5 bg-vmm-accent text-white rounded-lg text-sm font-medium">Create Rule</button>
              </div>
            </form>
          </Card>
        )}

        <div className="space-y-2">
          {rules.map(rule => (
            <Card key={rule.id}>
              <div className="p-3 flex items-center gap-3">
                <button onClick={() => toggleRule(rule)} className="flex-shrink-0">
                  {rule.enabled
                    ? <ToggleRight size={20} className="text-vmm-success" />
                    : <ToggleLeft size={20} className="text-vmm-text-muted" />
                  }
                </button>
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className={`text-sm font-medium ${rule.enabled ? 'text-vmm-text' : 'text-vmm-text-muted line-through'}`}>
                      {rule.name}
                    </span>
                    <span className={`text-[10px] px-1.5 py-0.5 rounded-full uppercase ${priorityColor(rule.priority)}`}>
                      {rule.priority}
                    </span>
                  </div>
                  <div className="text-xs text-vmm-text-muted mt-0.5">
                    {metricLabel(rule.metric)} &gt; {rule.threshold}% &rarr; {rule.action === 'auto_migrate' ? 'Auto-migrate' : 'Recommend'}
                    &nbsp;&bull;&nbsp;Cooldown: {Math.round(rule.cooldown_secs / 60)}min
                  </div>
                </div>
                <button onClick={() => deleteRule(rule.id)} className="text-vmm-text-muted hover:text-vmm-danger flex-shrink-0">
                  <Trash2 size={14} />
                </button>
              </div>
            </Card>
          ))}
          {rules.length === 0 && !showCreateRule && (
            <div className="text-xs text-vmm-text-muted text-center py-4">
              No DRS rules configured. Default thresholds (CPU 80%, RAM 90%) are used.
            </div>
          )}
        </div>
      </div>

      {/* ── Recommendations Section ────────────────────────────────── */}
      <div>
        <SectionLabel>Pending Recommendations</SectionLabel>
        <div className="space-y-3 mt-3">
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
            <div className="text-center text-vmm-text-muted py-8">
              <Activity size={28} className="mx-auto mb-2 opacity-30" />
              No recommendations — cluster is balanced
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
