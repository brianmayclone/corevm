import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Cable, Plus, Trash2, Circle, Shield, Network, Server } from 'lucide-react'
import api from '../api/client'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'
import ConfirmDialog from '../components/ConfirmDialog'
import type { ViSwitch } from '../api/types'

const policyLabels: Record<string, string> = {
  roundrobin: 'Round-Robin',
  failover: 'Failover',
  rulebased: 'Rule-Based',
}

const policyDescriptions: Record<string, string> = {
  roundrobin: 'Load balance across all uplinks',
  failover: 'Active / standby uplinks',
  rulebased: 'Route by IP / subnet (coming soon)',
}

export default function ViSwitches() {
  const navigate = useNavigate()
  const { clusters, fetchClusters } = useClusterStore()
  const [switches, setSwitches] = useState<(ViSwitch & { uplink_count?: number; port_count?: number })[]>([])
  const [showCreate, setShowCreate] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<ViSwitch | null>(null)
  const [form, setForm] = useState({
    cluster_id: '', name: '', description: '',
    uplink_policy: 'failover', mtu: 1500, max_ports: 1024, max_uplinks: 128,
  })

  const fetchSwitches = () => api.get('/api/viswitches').then(({ data }) => setSwitches(data)).catch(() => {})

  useEffect(() => { fetchSwitches(); fetchClusters() }, [])
  useEffect(() => { if (clusters.length > 0 && !form.cluster_id) setForm(f => ({ ...f, cluster_id: clusters[0].id })) }, [clusters])

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault()
    await api.post('/api/viswitches', form)
    setShowCreate(false)
    setForm(f => ({ ...f, name: '', description: '' }))
    fetchSwitches()
  }

  const handleDelete = async () => {
    if (!deleteTarget) return
    await api.delete(`/api/viswitches/${deleteTarget.id}`)
    setDeleteTarget(null)
    fetchSwitches()
  }

  const clusterName = (id: string) => clusters.find(c => c.id === id)?.name || id.substring(0, 8)

  return (
    <div className="space-y-5">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">viSwitches</h1>
          <p className="text-sm text-vmm-text-muted mt-1">Virtual switches with uplink teaming, traffic type control, and CoreSAN integration</p>
        </div>
        <button onClick={() => setShowCreate(true)}
          className="flex items-center gap-2 px-4 py-2 bg-vmm-accent hover:bg-vmm-accent-hover text-white rounded-lg text-sm font-medium">
          <Plus size={16} /> New viSwitch
        </button>
      </div>

      {/* Create form */}
      {showCreate && (
        <Card>
          <form onSubmit={handleCreate} className="p-5 space-y-4">
            <h3 className="text-sm font-semibold text-vmm-text">Create Virtual Switch</h3>
            <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Switch Name</label>
                <input type="text" value={form.name} onChange={e => setForm({ ...form, name: e.target.value })}
                  placeholder="Production-Switch" required className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              </div>
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Assigned Cluster</label>
                <select value={form.cluster_id} onChange={e => setForm({ ...form, cluster_id: e.target.value })}
                  className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  {clusters.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
                </select>
              </div>
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Uplink Policy</label>
                <select value={form.uplink_policy} onChange={e => setForm({ ...form, uplink_policy: e.target.value })}
                  className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  <option value="failover">Failover (active / standby)</option>
                  <option value="roundrobin">Round-Robin (load balance)</option>
                  <option value="rulebased" disabled>Rule-Based (coming soon)</option>
                </select>
              </div>
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Description</label>
                <input type="text" value={form.description} onChange={e => setForm({ ...form, description: e.target.value })}
                  placeholder="Optional description" className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              </div>
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">MTU</label>
                <select value={form.mtu} onChange={e => setForm({ ...form, mtu: parseInt(e.target.value) })}
                  className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  <option value={1500}>1500 (Standard)</option>
                  <option value={9000}>9000 (Jumbo Frames)</option>
                </select>
              </div>
              <div>
                <label className="block text-xs text-vmm-text-muted mb-1">Max Ports</label>
                <select value={form.max_ports} onChange={e => setForm({ ...form, max_ports: parseInt(e.target.value) })}
                  className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  <option value={256}>256</option>
                  <option value={512}>512</option>
                  <option value={1024}>1024</option>
                </select>
              </div>
            </div>
            <div className="flex gap-2 justify-end pt-2">
              <button type="button" onClick={() => setShowCreate(false)} className="px-4 py-2 text-sm text-vmm-text-muted">Cancel</button>
              <button type="submit" className="px-5 py-2 bg-vmm-accent text-white rounded-lg text-sm font-medium">Create viSwitch</button>
            </div>
          </form>
        </Card>
      )}

      {/* Switch cards */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        {switches.map(vs => (
          <Card key={vs.id}>
            <div className="p-5 cursor-pointer hover:bg-vmm-surface-hover/50 rounded-xl transition-colors"
              onClick={() => navigate(`/networks/viswitches/${vs.id}`)}>
              <div className="flex items-start justify-between mb-4">
                <div className="flex items-center gap-3">
                  <div className="w-10 h-10 rounded-lg bg-vmm-accent/10 flex items-center justify-center">
                    <Cable size={20} className="text-vmm-accent" />
                  </div>
                  <div>
                    <h3 className="text-base font-semibold text-vmm-text">{vs.name}</h3>
                    <div className="text-xs text-vmm-text-muted mt-0.5">
                      {vs.description || 'No description'}
                    </div>
                  </div>
                </div>
                <button onClick={(e) => { e.stopPropagation(); setDeleteTarget(vs) }}
                  className="text-vmm-text-muted hover:text-vmm-danger p-1"><Trash2 size={14} /></button>
              </div>

              <div className="flex items-center gap-2 mb-3 text-xs">
                <Shield size={11} className="text-vmm-text-muted" />
                <span className="text-vmm-text-muted">Cluster:</span>
                <span className="text-vmm-text font-medium">{clusterName(vs.cluster_id)}</span>
              </div>

              <div className="grid grid-cols-3 gap-2">
                <div className="flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium bg-vmm-accent/10 text-vmm-accent">
                  <Circle size={5} className="fill-current" />
                  {policyLabels[vs.uplink_policy] || vs.uplink_policy}
                </div>
                <div className="flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium bg-vmm-surface text-vmm-text-muted">
                  <Network size={11} /> {vs.uplink_count ?? 0} Uplinks
                </div>
                <div className="flex items-center gap-2 px-3 py-2 rounded-lg text-xs font-medium bg-vmm-surface text-vmm-text-muted">
                  <Server size={11} /> {vs.port_count ?? 0}/{vs.max_ports} Ports
                </div>
              </div>

              <div className="mt-3 text-[10px] text-vmm-text-muted">
                MTU {vs.mtu} &bull; {vs.enabled ? 'Enabled' : 'Disabled'}
              </div>
            </div>
          </Card>
        ))}
      </div>

      {switches.length === 0 && !showCreate && (
        <div className="text-center py-16">
          <Cable size={40} className="mx-auto mb-3 text-vmm-text-muted opacity-20" />
          <h3 className="text-vmm-text font-medium mb-1">No virtual switches</h3>
          <p className="text-sm text-vmm-text-muted">Create a viSwitch to manage uplink teaming, traffic types, and VM network connectivity.</p>
        </div>
      )}

      <ConfirmDialog
        open={!!deleteTarget}
        title="Delete viSwitch"
        message={`Permanently delete "${deleteTarget?.name}"? All connected VMs will be disconnected.`}
        danger
        onConfirm={handleDelete}
        onCancel={() => setDeleteTarget(null)}
      />
    </div>
  )
}
