/** QoS Policies — I/O throttling rules per VM or pool. */
import { useState } from 'react'
import { Plus, Gauge, Zap, Shield, Trash2 } from 'lucide-react'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import Button from '../components/Button'
import ContextMenu from '../components/ContextMenu'
import ConfirmDialog from '../components/ConfirmDialog'

interface QosPolicy {
  id: number
  name: string
  scope: 'vm' | 'pool' | 'global'
  max_iops_read: number | null
  max_iops_write: number | null
  max_bw_read_mbps: number | null
  max_bw_write_mbps: number | null
  max_latency_ms: number | null
  priority: 'high' | 'normal' | 'low'
  enabled: boolean
}

// Demo policies — will be replaced with API calls
const demoPolices: QosPolicy[] = []

export default function StorageQos() {
  const [policies, setPolicies] = useState<QosPolicy[]>(demoPolices)
  const [deletePolicy, setDeletePolicy] = useState<QosPolicy | null>(null)
  const [showCreate, setShowCreate] = useState(false)

  // Create form state
  const [form, setForm] = useState({
    name: '', scope: 'vm' as const, priority: 'normal' as const,
    max_iops_read: '', max_iops_write: '', max_bw_read_mbps: '', max_bw_write_mbps: '',
  })

  const handleCreate = () => {
    if (!form.name.trim()) return
    const newPolicy: QosPolicy = {
      id: Date.now(),
      name: form.name,
      scope: form.scope,
      max_iops_read: form.max_iops_read ? parseInt(form.max_iops_read) : null,
      max_iops_write: form.max_iops_write ? parseInt(form.max_iops_write) : null,
      max_bw_read_mbps: form.max_bw_read_mbps ? parseInt(form.max_bw_read_mbps) : null,
      max_bw_write_mbps: form.max_bw_write_mbps ? parseInt(form.max_bw_write_mbps) : null,
      max_latency_ms: null,
      priority: form.priority,
      enabled: true,
    }
    setPolicies([...policies, newPolicy])
    setForm({ name: '', scope: 'vm', priority: 'normal', max_iops_read: '', max_iops_write: '', max_bw_read_mbps: '', max_bw_write_mbps: '' })
    setShowCreate(false)
  }

  const priorityColors = {
    high: 'bg-vmm-danger/20 text-vmm-danger border-vmm-danger/30',
    normal: 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30',
    low: 'bg-vmm-text-muted/20 text-vmm-text-muted border-vmm-text-muted/30',
  }

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">QoS Policies</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            I/O throttling and bandwidth limits for virtual machines and storage pools
          </p>
        </div>
        <Button variant="primary" icon={<Plus size={14} />} onClick={() => setShowCreate(true)}>
          Create Policy
        </Button>
      </div>

      {/* Explanation cards */}
      <div className="grid grid-cols-3 gap-4">
        <Card>
          <div className="flex items-center gap-2 mb-2">
            <Gauge size={18} className="text-vmm-accent" />
            <span className="text-sm font-semibold text-vmm-text">IOPS Limits</span>
          </div>
          <p className="text-xs text-vmm-text-muted">
            Cap read/write I/O operations per second to prevent noisy-neighbor effects.
          </p>
        </Card>
        <Card>
          <div className="flex items-center gap-2 mb-2">
            <Zap size={18} className="text-vmm-warning" />
            <span className="text-sm font-semibold text-vmm-text">Bandwidth</span>
          </div>
          <p className="text-xs text-vmm-text-muted">
            Limit throughput (MB/s) per VM to ensure fair resource distribution.
          </p>
        </Card>
        <Card>
          <div className="flex items-center gap-2 mb-2">
            <Shield size={18} className="text-vmm-success" />
            <span className="text-sm font-semibold text-vmm-text">Priority Tiers</span>
          </div>
          <p className="text-xs text-vmm-text-muted">
            Assign priority levels so critical VMs get I/O preference during contention.
          </p>
        </Card>
      </div>

      {/* Create form */}
      {showCreate && (
        <Card>
          <SectionLabel className="mb-4">New QoS Policy</SectionLabel>
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Policy Name</label>
              <input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })}
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none"
                placeholder="e.g. Production Tier" />
            </div>
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Scope</label>
              <select value={form.scope} onChange={(e) => setForm({ ...form, scope: e.target.value as any })}
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none">
                <option value="vm">Per VM</option>
                <option value="pool">Per Pool</option>
                <option value="global">Global</option>
              </select>
            </div>
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Max IOPS Read</label>
              <input value={form.max_iops_read} onChange={(e) => setForm({ ...form, max_iops_read: e.target.value })}
                type="number" placeholder="Unlimited"
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none" />
            </div>
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Max IOPS Write</label>
              <input value={form.max_iops_write} onChange={(e) => setForm({ ...form, max_iops_write: e.target.value })}
                type="number" placeholder="Unlimited"
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none" />
            </div>
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Max BW Read (MB/s)</label>
              <input value={form.max_bw_read_mbps} onChange={(e) => setForm({ ...form, max_bw_read_mbps: e.target.value })}
                type="number" placeholder="Unlimited"
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none" />
            </div>
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Max BW Write (MB/s)</label>
              <input value={form.max_bw_write_mbps} onChange={(e) => setForm({ ...form, max_bw_write_mbps: e.target.value })}
                type="number" placeholder="Unlimited"
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none" />
            </div>
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Priority</label>
              <select value={form.priority} onChange={(e) => setForm({ ...form, priority: e.target.value as any })}
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none">
                <option value="high">High</option>
                <option value="normal">Normal</option>
                <option value="low">Low</option>
              </select>
            </div>
          </div>
          <div className="flex items-center justify-end gap-3 mt-4">
            <Button variant="ghost" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button variant="primary" onClick={handleCreate}>Create Policy</Button>
          </div>
        </Card>
      )}

      {/* Policy list */}
      {policies.length === 0 && !showCreate ? (
        <Card>
          <div className="flex flex-col items-center justify-center py-12 text-center">
            <div className="w-16 h-16 rounded-2xl bg-vmm-bg-alt flex items-center justify-center mb-4">
              <Gauge size={28} className="text-vmm-text-muted" />
            </div>
            <h3 className="text-lg font-semibold text-vmm-text mb-2">No QoS Policies</h3>
            <p className="text-sm text-vmm-text-muted max-w-md">
              Create I/O throttling policies to prevent resource contention and ensure
              predictable performance for critical workloads.
            </p>
          </div>
        </Card>
      ) : policies.length > 0 && (
        <Card padding={false}>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-vmm-border text-[10px] text-vmm-text-muted uppercase tracking-wider">
                <th className="text-left px-5 py-3">Policy Name</th>
                <th className="text-left px-5 py-3">Scope</th>
                <th className="text-left px-5 py-3">Priority</th>
                <th className="text-left px-5 py-3">IOPS (R/W)</th>
                <th className="text-left px-5 py-3">BW (R/W)</th>
                <th className="text-left px-5 py-3">Status</th>
                <th className="text-right px-5 py-3 w-12"></th>
              </tr>
            </thead>
            <tbody>
              {policies.map((p) => (
                <tr key={p.id} className="border-b border-vmm-border last:border-b-0 hover:bg-vmm-surface-hover/50">
                  <td className="px-5 py-3 text-vmm-text font-medium">{p.name}</td>
                  <td className="px-5 py-3 text-vmm-text-dim uppercase text-xs">{p.scope}</td>
                  <td className="px-5 py-3">
                    <span className={`px-1.5 py-0.5 text-[10px] font-bold tracking-wider rounded border ${priorityColors[p.priority]}`}>
                      {p.priority.toUpperCase()}
                    </span>
                  </td>
                  <td className="px-5 py-3 text-vmm-text-dim font-mono text-xs">
                    {p.max_iops_read ?? '∞'} / {p.max_iops_write ?? '∞'}
                  </td>
                  <td className="px-5 py-3 text-vmm-text-dim font-mono text-xs">
                    {p.max_bw_read_mbps ? `${p.max_bw_read_mbps} MB/s` : '∞'} / {p.max_bw_write_mbps ? `${p.max_bw_write_mbps} MB/s` : '∞'}
                  </td>
                  <td className="px-5 py-3">
                    <span className={`px-1.5 py-0.5 text-[10px] font-bold tracking-wider rounded border
                      ${p.enabled ? 'bg-vmm-success/20 text-vmm-success border-vmm-success/30' : 'bg-vmm-text-muted/20 text-vmm-text-muted border-vmm-text-muted/30'}`}>
                      {p.enabled ? 'ACTIVE' : 'DISABLED'}
                    </span>
                  </td>
                  <td className="px-5 py-3 text-right">
                    <ContextMenu items={[
                      { label: 'Delete', icon: <Trash2 size={14} />, danger: true, onClick: () => setDeletePolicy(p) },
                    ]} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}

      <ConfirmDialog
        open={!!deletePolicy}
        title="Delete QoS Policy"
        message={`Delete policy "${deletePolicy?.name}"? VMs using this policy will revert to unlimited I/O.`}
        confirmLabel="Delete"
        danger
        onConfirm={() => { setPolicies(policies.filter(p => p.id !== deletePolicy?.id)); setDeletePolicy(null) }}
        onCancel={() => setDeletePolicy(null)}
      />
    </div>
  )
}
