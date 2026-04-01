import { useState, useEffect } from 'react'
import { Trash2, Plus } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import api from '../api/client'
import type { CoreSanVolume, IscsiAcl, IscsiTarget } from '../api/types'
import { formatBytes } from '../utils/format'
import Button from '../components/Button'
import Card from '../components/Card'
import CreateIscsiAclDialog from '../components/coresan/CreateIscsiAclDialog'
import ConfirmDialog from '../components/ConfirmDialog'

export default function StorageBlockStorage() {
  const { backendMode } = useClusterStore()
  const isCluster = backendMode === 'cluster'

  const [volumes, setVolumes] = useState<CoreSanVolume[]>([])
  const [acls, setAcls] = useState<IscsiAcl[]>([])
  const [targets, setTargets] = useState<IscsiTarget[]>([])
  const [tab, setTab] = useState<'volumes' | 'acls' | 'connect'>('volumes')
  const [showCreateAcl, setShowCreateAcl] = useState(false)
  const [deleteAclId, setDeleteAclId] = useState<string | null>(null)
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
      setVolumes(vols.filter(v => v.access_protocols?.includes('iscsi')))

      let aclList: IscsiAcl[] = []
      try {
        if (isCluster) {
          const { data } = await api.get<IscsiAcl[]>('/api/san/iscsi/acls')
          aclList = Array.isArray(data) ? data : []
        } else {
          const resp = await fetch(`${sanBase}/api/iscsi/acls`)
          if (resp.ok) { const d = await resp.json(); aclList = Array.isArray(d) ? d : [] }
        }
      } catch {}
      setAcls(aclList)

      let tgtList: IscsiTarget[] = []
      try {
        if (isCluster) {
          const { data } = await api.get<IscsiTarget[]>('/api/san/iscsi/targets')
          tgtList = Array.isArray(data) ? data : []
        } else {
          const resp = await fetch(`${sanBase}/api/iscsi/targets`)
          if (resp.ok) { const d = await resp.json(); tgtList = Array.isArray(d) ? d : [] }
        }
      } catch {}
      setTargets(tgtList)
    } catch (e: any) {
      setError(e.message || 'Failed to load data')
    }
  }

  useEffect(() => { fetchData() }, [isCluster])

  const handleDeleteAcl = async () => {
    if (!deleteAclId) return
    try {
      if (isCluster) {
        await api.delete(`/api/san/iscsi/acls/${deleteAclId}`)
      } else {
        await fetch(`${sanBase}/api/iscsi/acls/${deleteAclId}`, { method: 'DELETE' })
      }
      setDeleteAclId(null)
      fetchData()
    } catch (e: any) {
      setError(e.message || 'Failed to delete ACL')
    }
  }

  const aluaLabel = (state: string) => {
    switch (state) {
      case 'active_optimized': return 'Active/Optimized'
      case 'active_non_optimized': return 'Active/Non-Optimized'
      case 'standby': return 'Standby'
      default: return state
    }
  }

  const aluaColor = (state: string) => {
    switch (state) {
      case 'active_optimized': return 'bg-green-500/10 text-green-400'
      case 'active_non_optimized': return 'bg-yellow-500/10 text-yellow-400'
      case 'standby': return 'bg-blue-500/10 text-blue-400'
      default: return 'bg-red-500/10 text-red-400'
    }
  }

  const tabs = [
    { key: 'volumes' as const, label: 'iSCSI Volumes' },
    { key: 'acls' as const, label: 'Access Control (ACLs)' },
    { key: 'connect' as const, label: 'Connection Info' },
  ]

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-vmm-text">Block Storage</h1>
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
              Volumes with iSCSI access protocol enabled. Manage volumes in the CoreSAN page.
            </p>
            {volumes.length === 0 ? (
              <p className="text-vmm-muted text-sm py-8 text-center">
                No iSCSI-enabled volumes. Create a volume with iSCSI protocol in CoreSAN, or enable iSCSI on an existing volume.
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
                    <th className="pb-2 font-medium">ALUA</th>
                  </tr>
                </thead>
                <tbody>
                  {volumes.map(v => {
                    const target = targets.find(t => t.volume_id === v.id)
                    return (
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
                        <td className="py-2">
                          {target && (
                            <span className={`px-2 py-0.5 rounded-full text-xs font-medium ${aluaColor(target.alua_state)}`}>
                              {aluaLabel(target.alua_state)}
                            </span>
                          )}
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            )}
          </div>
        </Card>
      )}

      {tab === 'acls' && (
        <Card>
          <div className="p-4">
            <div className="flex items-center justify-between mb-4">
              <p className="text-sm text-vmm-muted">iSCSI initiator access control per volume.</p>
              <Button size="sm" onClick={() => setShowCreateAcl(true)}>
                <Plus size={14} className="mr-1" /> Add Initiator
              </Button>
            </div>
            {acls.length === 0 ? (
              <p className="text-vmm-muted text-sm py-8 text-center">
                No ACLs configured. Add an initiator IQN to allow iSCSI access to a volume.
              </p>
            ) : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="text-left text-vmm-muted border-b border-vmm-border">
                    <th className="pb-2 font-medium">Volume</th>
                    <th className="pb-2 font-medium">Initiator IQN</th>
                    <th className="pb-2 font-medium">Comment</th>
                    <th className="pb-2 font-medium">Created</th>
                    <th className="pb-2 font-medium w-16"></th>
                  </tr>
                </thead>
                <tbody>
                  {acls.map(a => (
                    <tr key={a.id} className="border-b border-vmm-border/50 hover:bg-vmm-hover">
                      <td className="py-2 font-medium text-vmm-text">{a.volume_name}</td>
                      <td className="py-2 font-mono text-xs text-vmm-text">{a.initiator_iqn}</td>
                      <td className="py-2 text-vmm-muted">{a.comment || '\u2014'}</td>
                      <td className="py-2 text-vmm-muted text-xs">{a.created_at}</td>
                      <td className="py-2">
                        <button onClick={() => setDeleteAclId(a.id)}
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
              Use standard iSCSI initiators to connect to block storage volumes.
            </p>
            <div className="space-y-3">
              <div>
                <h3 className="text-sm font-medium text-vmm-text mb-1">Target Portal</h3>
                <code className="block bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text">
                  &lt;host&gt;:3260
                </code>
              </div>
              <div>
                <h3 className="text-sm font-medium text-vmm-text mb-1">Linux Initiator (iscsiadm)</h3>
                <pre className="bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text whitespace-pre overflow-x-auto">{`# Discovery
iscsiadm -m discovery -t sendtargets -p <host>:3260

# Login to a target
iscsiadm -m node -T iqn.2026-04.io.corevm:<volume> -p <host>:3260 --login

# Verify block device
lsblk | grep sd`}</pre>
              </div>
              <div>
                <h3 className="text-sm font-medium text-vmm-text mb-1">Multipath Setup</h3>
                <pre className="bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text whitespace-pre overflow-x-auto">{`apt install multipath-tools
cat >> /etc/multipath.conf <<EOF
devices {
    device {
        vendor  "CoreVM"
        product "CoreSAN"
        path_grouping_policy  group_by_prio
        prio    alua
        failback immediate
    }
}
EOF
systemctl restart multipathd
multipath -ll`}</pre>
              </div>
            </div>
          </div>
        </Card>
      )}

      <CreateIscsiAclDialog
        open={showCreateAcl}
        onClose={() => setShowCreateAcl(false)}
        onCreated={() => { setShowCreateAcl(false); fetchData() }}
        isCluster={isCluster}
        sanBase={sanBase}
        volumes={volumes}
      />

      <ConfirmDialog
        open={!!deleteAclId}
        title="Delete iSCSI ACL"
        message="This will immediately revoke iSCSI access for this initiator. Active sessions may be disconnected."
        confirmLabel="Delete"
        danger
        onConfirm={handleDeleteAcl}
        onCancel={() => setDeleteAclId(null)}
      />
    </div>
  )
}
