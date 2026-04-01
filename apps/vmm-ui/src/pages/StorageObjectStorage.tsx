import { useState, useEffect } from 'react'
import { Trash2, Plus } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import api from '../api/client'
import type { CoreSanVolume, S3Credential } from '../api/types'
import { formatBytes } from '../utils/format'
import Button from '../components/Button'
import Card from '../components/Card'
import CreateS3CredentialDialog from '../components/coresan/CreateS3CredentialDialog'
import ConfirmDialog from '../components/ConfirmDialog'

export default function StorageObjectStorage() {
  const { backendMode } = useClusterStore()
  const isCluster = backendMode === 'cluster'

  const [volumes, setVolumes] = useState<CoreSanVolume[]>([])
  const [credentials, setCredentials] = useState<S3Credential[]>([])
  const [tab, setTab] = useState<'volumes' | 'credentials' | 'connect'>('volumes')
  const [showCreateCred, setShowCreateCred] = useState(false)
  const [deleteCredId, setDeleteCredId] = useState<string | null>(null)
  const [error, setError] = useState('')

  const sanBase = 'http://localhost:7443'

  const fetchData = async () => {
    try {
      let vols: CoreSanVolume[] = []
      try {
        if (isCluster) {
          const { data } = await api.get<CoreSanVolume[]>('/api/san/volumes')
          vols = Array.isArray(data) ? data : []
        } else {
          const resp = await fetch(`${sanBase}/api/volumes`)
          if (resp.ok) { const d = await resp.json(); vols = Array.isArray(d) ? d : [] }
        }
      } catch {}
      setVolumes(vols.filter(v => v.access_protocols?.includes('s3')))

      let creds: S3Credential[] = []
      try {
        if (isCluster) {
          const { data } = await api.get<S3Credential[]>('/api/san/s3/credentials')
          creds = Array.isArray(data) ? data : []
        } else {
          const resp = await fetch(`${sanBase}/api/s3/credentials`)
          if (resp.ok) { const d = await resp.json(); creds = Array.isArray(d) ? d : [] }
        }
      } catch {}
      setCredentials(creds)
    } catch (e: any) {
      setError(e.message || 'Failed to load data')
    }
  }

  useEffect(() => { fetchData() }, [isCluster])

  const handleDeleteCred = async () => {
    if (!deleteCredId) return
    try {
      if (isCluster) {
        await api.delete(`/api/san/s3/credentials/${deleteCredId}`)
      } else {
        await fetch(`${sanBase}/api/s3/credentials/${deleteCredId}`, { method: 'DELETE' })
      }
      setDeleteCredId(null)
      fetchData()
    } catch (e: any) {
      setError(e.message || 'Failed to delete credential')
    }
  }

  const tabs = [
    { key: 'volumes' as const, label: 'S3 Volumes' },
    { key: 'credentials' as const, label: 'Credentials' },
    { key: 'connect' as const, label: 'Connection Info' },
  ]

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-vmm-text">Object Storage</h1>
      </div>

      {error && (
        <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">{error}</div>
      )}

      <div className="flex gap-1 border-b border-vmm-border">
        {tabs.map(t => (
          <button key={t.key} onClick={() => setTab(t.key)}
            className={`px-4 py-2 text-sm font-medium border-b-2 transition-colors ${
              tab === t.key ? 'border-vmm-accent text-vmm-accent' : 'border-transparent text-vmm-muted hover:text-vmm-text'
            }`}>
            {t.label}
          </button>
        ))}
      </div>

      {tab === 'volumes' && (
        <Card>
          <div className="p-4">
            <p className="text-sm text-vmm-muted mb-4">
              Volumes with S3 access protocol enabled. Manage volumes in the CoreSAN page.
            </p>
            {volumes.length === 0 ? (
              <p className="text-vmm-muted text-sm py-8 text-center">
                No S3-enabled volumes. Create a volume with S3 protocol in CoreSAN, or enable S3 on an existing volume.
              </p>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-vmm-muted border-b border-vmm-border">
                    <th className="pb-2 font-medium">Name</th>
                    <th className="pb-2 font-medium">Status</th>
                    <th className="pb-2 font-medium">Size</th>
                    <th className="pb-2 font-medium">Used</th>
                    <th className="pb-2 font-medium">Protocols</th>
                    <th className="pb-2 font-medium">FTT</th>
                  </tr>
                </thead>
                <tbody>
                  {volumes.map(v => (
                    <tr key={v.id} className="border-b border-vmm-border/50 hover:bg-vmm-hover">
                      <td className="py-2 font-medium text-vmm-text">{v.name}</td>
                      <td className="py-2">
                        <span className={`px-2 py-0.5 rounded-full text-xs font-medium ${
                          v.status === 'online' ? 'bg-green-500/10 text-green-400' :
                          v.status === 'degraded' ? 'bg-yellow-500/10 text-yellow-400' :
                          'bg-red-500/10 text-red-400'
                        }`}>{v.status}</span>
                      </td>
                      <td className="py-2 text-vmm-muted">{formatBytes(v.max_size_bytes)}</td>
                      <td className="py-2 text-vmm-muted">{formatBytes(v.total_bytes)}</td>
                      <td className="py-2">
                        {v.access_protocols?.map(p => (
                          <span key={p} className="px-1.5 py-0.5 rounded text-xs bg-vmm-accent/10 text-vmm-accent mr-1">{p}</span>
                        ))}
                      </td>
                      <td className="py-2 text-vmm-muted">{v.ftt}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </Card>
      )}

      {tab === 'credentials' && (
        <Card>
          <div className="p-4">
            <div className="flex items-center justify-between mb-4">
              <p className="text-sm text-vmm-muted">S3 access keys for external client access.</p>
              <Button size="sm" onClick={() => setShowCreateCred(true)}>
                <Plus size={14} className="mr-1" /> Create Key
              </Button>
            </div>
            {credentials.length === 0 ? (
              <p className="text-vmm-muted text-sm py-8 text-center">
                No S3 credentials. Create one to access object storage via S3 API.
              </p>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-vmm-muted border-b border-vmm-border">
                    <th className="pb-2 font-medium">Access Key</th>
                    <th className="pb-2 font-medium">User</th>
                    <th className="pb-2 font-medium">Name</th>
                    <th className="pb-2 font-medium">Status</th>
                    <th className="pb-2 font-medium">Created</th>
                    <th className="pb-2 font-medium w-16"></th>
                  </tr>
                </thead>
                <tbody>
                  {credentials.map(c => (
                    <tr key={c.id} className="border-b border-vmm-border/50 hover:bg-vmm-hover">
                      <td className="py-2 font-mono text-xs text-vmm-text">{c.access_key}</td>
                      <td className="py-2 text-vmm-muted">{c.user_id}</td>
                      <td className="py-2 text-vmm-muted">{c.display_name || '\u2014'}</td>
                      <td className="py-2">
                        <span className={`px-2 py-0.5 rounded-full text-xs font-medium ${
                          c.status === 'active' ? 'bg-green-500/10 text-green-400' : 'bg-red-500/10 text-red-400'
                        }`}>{c.status}</span>
                      </td>
                      <td className="py-2 text-vmm-muted text-xs">{c.created_at}</td>
                      <td className="py-2">
                        <button onClick={() => setDeleteCredId(c.id)}
                          className="p-1 rounded hover:bg-vmm-danger/10 text-vmm-muted hover:text-vmm-danger">
                          <Trash2 size={14} />
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </Card>
      )}

      {tab === 'connect' && (
        <Card>
          <div className="p-4 space-y-4">
            <p className="text-sm text-vmm-muted">
              Use any S3-compatible client to access your object storage volumes.
            </p>
            <div className="space-y-3">
              <div>
                <h3 className="text-sm font-medium text-vmm-text mb-1">Endpoint</h3>
                <code className="block bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text">
                  http://&lt;host&gt;:9000
                </code>
              </div>
              <div>
                <h3 className="text-sm font-medium text-vmm-text mb-1">AWS CLI</h3>
                <pre className="bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text whitespace-pre overflow-x-auto">{`aws configure
# Access Key: <your access key>
# Secret Key: <your secret key>
# Region: us-east-1

aws s3 ls --endpoint-url http://<host>:9000
aws s3 cp myfile.txt s3://<bucket>/myfile.txt --endpoint-url http://<host>:9000`}</pre>
              </div>
              <div>
                <h3 className="text-sm font-medium text-vmm-text mb-1">MinIO Client (mc)</h3>
                <pre className="bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text whitespace-pre overflow-x-auto">{`mc alias set coresan http://<host>:9000 <access_key> <secret_key>
mc ls coresan
mc cp myfile.txt coresan/<bucket>/`}</pre>
              </div>
            </div>
          </div>
        </Card>
      )}

      <CreateS3CredentialDialog
        open={showCreateCred}
        onClose={() => setShowCreateCred(false)}
        onCreated={() => { setShowCreateCred(false); fetchData() }}
        isCluster={isCluster}
        sanBase={sanBase}
      />

      <ConfirmDialog
        open={!!deleteCredId}
        title="Delete S3 Credential"
        message="This will immediately revoke access for any client using this key. This action cannot be undone."
        confirmLabel="Delete"
        danger
        onConfirm={handleDeleteCred}
        onCancel={() => setDeleteCredId(null)}
      />
    </div>
  )
}
