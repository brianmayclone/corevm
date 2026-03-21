import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Plus, Trash2, Shield, Zap, Activity } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import Card from '../components/Card'

export default function ClusterSettings() {
  const { clusters, fetchClusters, createCluster, deleteCluster } = useClusterStore()
  const [showCreate, setShowCreate] = useState(false)
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  const navigate = useNavigate()

  useEffect(() => { fetchClusters() }, [])

  const handleCreate = async (e: React.FormEvent) => {
    e.preventDefault()
    await createCluster(name, description)
    setName(''); setDescription(''); setShowCreate(false)
  }

  return (
    <div className="space-y-5">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Clusters</h1>
          <p className="text-sm text-vmm-text-muted mt-1">Manage logical clusters and their HA/DRS settings</p>
        </div>
        <button onClick={() => setShowCreate(true)}
          className="flex items-center gap-2 px-4 py-2 bg-vmm-accent hover:bg-vmm-accent-hover text-white rounded-lg text-sm font-medium">
          <Plus size={16} /> New Cluster
        </button>
      </div>

      {showCreate && (
        <Card>
          <form onSubmit={handleCreate} className="p-4 space-y-3">
            <input type="text" value={name} onChange={e => setName(e.target.value)} placeholder="Cluster name"
              className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" required />
            <input type="text" value={description} onChange={e => setDescription(e.target.value)} placeholder="Description (optional)"
              className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
            <div className="flex gap-2 justify-end">
              <button type="button" onClick={() => setShowCreate(false)}
                className="px-4 py-2 text-sm text-vmm-text-muted hover:text-vmm-text">Cancel</button>
              <button type="submit" className="px-4 py-2 bg-vmm-accent text-white rounded-lg text-sm font-medium">Create</button>
            </div>
          </form>
        </Card>
      )}

      <div className="space-y-3">
        {clusters.map(cluster => (
          <Card key={cluster.id}>
            <div className="p-4 cursor-pointer hover:bg-vmm-surface-hover rounded-lg transition-colors"
              onClick={() => navigate(`/cluster/detail/${cluster.id}`)}>
              <div className="flex items-center justify-between mb-2">
                <h3 className="font-semibold text-vmm-text">{cluster.name}</h3>
                <button onClick={() => { if (confirm(`Delete cluster "${cluster.name}"?`)) deleteCluster(cluster.id) }}
                  className="text-vmm-text-muted hover:text-vmm-danger"><Trash2 size={14} /></button>
              </div>
              {cluster.description && <p className="text-sm text-vmm-text-muted mb-3">{cluster.description}</p>}
              <div className="flex items-center gap-4 text-xs text-vmm-text-dim">
                <span>{cluster.host_count} hosts</span>
                <span>{cluster.vm_count} VMs</span>
                <span className="flex items-center gap-1">
                  <Shield size={11} className={cluster.ha_enabled ? 'text-vmm-success' : 'text-vmm-text-muted'} />
                  HA {cluster.ha_enabled ? 'On' : 'Off'}
                </span>
                <span className="flex items-center gap-1">
                  <Activity size={11} className={cluster.drs_enabled ? 'text-vmm-accent' : 'text-vmm-text-muted'} />
                  DRS {cluster.drs_enabled ? 'On' : 'Off'}
                </span>
              </div>
            </div>
          </Card>
        ))}
      </div>
    </div>
  )
}
