import { useEffect, useState } from 'react'
import { HardDrive, Plus, Circle, Server, Trash2 } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'
import { formatBytes } from '../utils/format'

export default function DatastoresList() {
  const { datastores, fetchDatastores, clusters, fetchClusters, createDatastore } = useClusterStore()
  const [showCreate, setShowCreate] = useState(false)
  const [form, setForm] = useState({ name: '', store_type: 'nfs', mount_source: '', mount_opts: '', mount_path: '', cluster_id: '' })

  useEffect(() => { fetchDatastores(); fetchClusters() }, [])
  useEffect(() => {
    if (clusters.length > 0 && !form.cluster_id) setForm(f => ({ ...f, cluster_id: clusters[0].id }))
  }, [clusters])

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault()
    await createDatastore(form)
    setShowCreate(false)
    setForm({ name: '', store_type: 'nfs', mount_source: '', mount_opts: '', mount_path: '', cluster_id: clusters[0]?.id || '' })
  }

  return (
    <div className="space-y-5">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Datastores</h1>
          <p className="text-sm text-vmm-text-muted mt-1">Cluster-wide shared storage</p>
        </div>
        <button onClick={() => setShowCreate(true)}
          className="flex items-center gap-2 px-4 py-2 bg-vmm-accent hover:bg-vmm-accent-hover text-white rounded-lg text-sm font-medium">
          <Plus size={16} /> New Datastore
        </button>
      </div>

      {showCreate && (
        <Card>
          <form onSubmit={handleCreate} className="p-4 space-y-3">
            <div className="grid grid-cols-2 gap-3">
              <input type="text" value={form.name} onChange={e => setForm({ ...form, name: e.target.value })}
                placeholder="Datastore name" className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" required />
              <select value={form.store_type} onChange={e => setForm({ ...form, store_type: e.target.value })}
                className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                <option value="nfs">NFS</option>
                <option value="cephfs">CephFS</option>
                <option value="glusterfs">GlusterFS</option>
              </select>
              <input type="text" value={form.mount_source} onChange={e => setForm({ ...form, mount_source: e.target.value })}
                placeholder="Mount source (e.g. server:/export)" className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" required />
              <input type="text" value={form.mount_path} onChange={e => setForm({ ...form, mount_path: e.target.value })}
                placeholder="Mount path (e.g. /vmm/datastores/ds1)" className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" required />
              <input type="text" value={form.mount_opts} onChange={e => setForm({ ...form, mount_opts: e.target.value })}
                placeholder="Mount options (optional)" className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
              <select value={form.cluster_id} onChange={e => setForm({ ...form, cluster_id: e.target.value })}
                className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                {clusters.map(c => <option key={c.id} value={c.id}>{c.name}</option>)}
              </select>
            </div>
            <div className="flex gap-2 justify-end">
              <button type="button" onClick={() => setShowCreate(false)} className="px-4 py-2 text-sm text-vmm-text-muted">Cancel</button>
              <button type="submit" className="px-4 py-2 bg-vmm-accent text-white rounded-lg text-sm font-medium">Create</button>
            </div>
          </form>
        </Card>
      )}

      <div className="space-y-3">
        {datastores.map(ds => {
          const usedPct = ds.total_bytes > 0 ? Math.round(((ds.total_bytes - ds.free_bytes) / ds.total_bytes) * 100) : 0
          return (
            <Card key={ds.id}>
              <div className="p-4">
                <div className="flex items-center justify-between mb-2">
                  <div className="flex items-center gap-2">
                    <HardDrive size={16} className="text-vmm-text-muted" />
                    <span className="font-semibold text-vmm-text">{ds.name}</span>
                    <span className="text-xs px-2 py-0.5 bg-vmm-surface rounded-full text-vmm-text-muted uppercase">{ds.store_type}</span>
                  </div>
                  <div className="flex items-center gap-1.5">
                    <Circle size={6} className={ds.status === 'online' ? 'fill-vmm-success text-vmm-success' : 'fill-vmm-danger text-vmm-danger'} />
                    <span className="text-xs text-vmm-text-muted">{ds.status}</span>
                  </div>
                </div>
                <div className="text-xs text-vmm-text-muted mb-2">{ds.mount_source} &rarr; {ds.mount_path}</div>
                <div className="w-full bg-vmm-bg rounded-full h-1.5 mb-2">
                  <div className={`h-1.5 rounded-full ${usedPct > 90 ? 'bg-vmm-danger' : 'bg-vmm-accent'}`} style={{ width: `${usedPct}%` }} />
                </div>
                <div className="flex items-center justify-between text-xs text-vmm-text-muted">
                  <span>{formatBytes(ds.total_bytes - ds.free_bytes)} / {formatBytes(ds.total_bytes)}</span>
                  <span className="flex items-center gap-1">
                    <Server size={10} /> {ds.host_mounts.filter(m => m.mounted).length}/{ds.host_mounts.length} hosts mounted
                  </span>
                </div>
              </div>
            </Card>
          )
        })}
      </div>
    </div>
  )
}
