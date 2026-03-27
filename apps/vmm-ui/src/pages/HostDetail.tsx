import { useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { Server, Cpu, MemoryStick, HardDrive, Circle, Wrench, ArrowLeft, Trash2, Boxes, Check, X, Pencil, FileText } from 'lucide-react'
import HostEditDialog from '../components/HostEditDialog'
import HostLogs from '../components/HostLogs'
import { useClusterStore } from '../stores/clusterStore'
import { useVmStore } from '../stores/vmStore'
import api from '../api/client'
import Card from '../components/Card'
import SpecRow from '../components/SpecRow'
import TabBar from '../components/TabBar'
import VmPriorityCard from '../components/VmPriorityCard'
import MaintenanceDialog from '../components/MaintenanceDialog'
import { formatRam, formatBytes } from '../utils/format'
import type { Host } from '../api/types'

export default function HostDetail() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const { hosts, fetchHosts, setMaintenance, deregisterHost } = useClusterStore()
  const [maintenanceOpen, setMaintenanceOpen] = useState(false)
  const [editOpen, setEditOpen] = useState(false)
  const [activeTab, setActiveTab] = useState('overview')
  const { vms, fetchVms, startVm, stopVm } = useVmStore()
  const [host, setHost] = useState<Host | null>(null)

  useEffect(() => { fetchHosts(); fetchVms() }, [])
  useEffect(() => { setHost(hosts.find(h => h.id === id) || null) }, [hosts, id])

  if (!host) return <div className="text-vmm-text-muted p-8">Loading...</div>

  const hostVms = vms.filter((v: any) => v.host_id === id)
  const ramPct = host.total_ram_mb > 0
    ? Math.round(((host.total_ram_mb - host.free_ram_mb) / host.total_ram_mb) * 100) : 0

  return (
    <div className="space-y-5">
      <div className="flex items-center gap-3">
        <button onClick={() => navigate('/cluster/hosts')} className="text-vmm-text-muted hover:text-vmm-text">
          <ArrowLeft size={20} />
        </button>
        <div className="flex-1">
          <h1 className="text-2xl font-bold text-vmm-text">{host.hostname}</h1>
          <p className="text-sm text-vmm-text-muted">{host.address}</p>
        </div>
        <div className="flex gap-2">
          <button
            onClick={() => setEditOpen(true)}
            className="flex items-center gap-2 px-4 py-2 bg-vmm-surface hover:bg-vmm-surface-hover border border-vmm-border rounded-lg text-sm font-medium text-vmm-text transition-colors cursor-pointer"
          >
            <Pencil size={14} /> Edit
          </button>
          <button
            onClick={() => {
              if (host.maintenance_mode) {
                setMaintenance(host.id, false)
              } else {
                setMaintenanceOpen(true)
              }
            }}
            className={`flex items-center gap-2 px-4 py-2 rounded-lg text-sm font-medium transition-colors ${
              host.maintenance_mode
                ? 'bg-vmm-accent/10 text-vmm-accent hover:bg-vmm-accent/20'
                : 'bg-yellow-500/10 text-yellow-400 hover:bg-yellow-500/20'
            }`}
          >
            <Wrench size={14} />
            {host.maintenance_mode ? 'Exit Maintenance' : 'Enter Maintenance'}
          </button>
          <button
            onClick={async () => { if (confirm('Remove this host from the cluster?')) { await deregisterHost(host.id); navigate('/cluster/hosts') } }}
            className="flex items-center gap-2 px-4 py-2 bg-vmm-danger/10 text-vmm-danger hover:bg-vmm-danger/20 rounded-lg text-sm font-medium transition-colors"
          >
            <Trash2 size={14} /> Remove
          </button>
        </div>
      </div>

      <TabBar
        tabs={[
          { id: 'overview', label: 'Overview' },
          { id: 'logs', label: 'Logs' },
        ]}
        active={activeTab}
        onChange={setActiveTab}
      />

      {activeTab === 'logs' && <HostLogs hostId={host.id} />}

      {activeTab === 'overview' && <>
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        <Card>
          <div className="p-4">
            <h3 className="text-sm font-semibold text-vmm-text mb-3">Status</h3>
            <div className="space-y-2">
              <SpecRow icon={<Circle size={14} />} label="Status" value={host.status} />
              <SpecRow icon={<Server size={14} />} label="Version" value={`v${host.version}`} />
              <SpecRow icon={<Cpu size={14} />} label="CPU" value={`${host.cpu_model}`} />
              <SpecRow icon={<Cpu size={14} />} label="Cores / Threads" value={`${host.cpu_cores} / ${host.cpu_threads}`} />
              <SpecRow icon={<Cpu size={14} />} label="CPU Usage" value={`${host.cpu_usage_pct.toFixed(1)}%`} />
            </div>
          </div>
        </Card>

        <Card>
          <div className="p-4">
            <h3 className="text-sm font-semibold text-vmm-text mb-3">Memory</h3>
            <div className="text-2xl font-bold text-vmm-text mb-1">{ramPct}%</div>
            <div className="text-xs text-vmm-text-muted mb-3">
              {formatRam(host.total_ram_mb - host.free_ram_mb)} used of {formatRam(host.total_ram_mb)}
            </div>
            <div className="w-full bg-vmm-bg rounded-full h-2">
              <div
                className={`h-2 rounded-full ${ramPct > 90 ? 'bg-vmm-danger' : ramPct > 70 ? 'bg-yellow-400' : 'bg-vmm-accent'}`}
                style={{ width: `${ramPct}%` }}
              />
            </div>
          </div>
        </Card>

        <Card>
          <div className="p-4">
            <h3 className="text-sm font-semibold text-vmm-text mb-3">Virtual Machines</h3>
            <div className="text-2xl font-bold text-vmm-text mb-1">{hostVms.length}</div>
            <div className="text-xs text-vmm-text-muted">
              {hostVms.filter(v => v.state === 'running').length} running &bull; {' '}
              {hostVms.filter(v => v.state === 'stopped').length} stopped
            </div>
          </div>
        </Card>
      </div>

      {/* CoreSAN Status */}
      <Card>
        <div className="p-4">
          <div className="flex items-center gap-2 mb-3">
            <Boxes size={16} className={host.san_enabled ? 'text-vmm-accent' : 'text-vmm-text-muted'} />
            <h3 className="text-sm font-semibold text-vmm-text">CoreSAN</h3>
            {host.san_enabled ? (
              <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] font-bold bg-vmm-success/20 text-vmm-success border border-vmm-success/30">
                <Check size={8} /> ACTIVE
              </span>
            ) : (
              <span className="inline-flex items-center gap-1 px-2 py-0.5 rounded text-[10px] font-bold bg-vmm-text-muted/20 text-vmm-text-muted border border-vmm-border">
                <X size={8} /> NOT RUNNING
              </span>
            )}
          </div>
          {host.san_enabled ? (
            <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
              <SpecRow label="Node ID" value={host.san_node_id?.slice(0, 12) + '...'} />
              <SpecRow label="API Address" value={host.san_address || 'N/A'} />
              <SpecRow label="Volumes" value={`${host.san_volumes}`} />
              <SpecRow label="Peers" value={`${host.san_peers}`} />
            </div>
          ) : (
            <p className="text-xs text-vmm-text-muted">
              CoreSAN (vmm-san) is not running on this host. Start it to enable software-defined storage.
            </p>
          )}
        </div>
      </Card>

      {hostVms.length > 0 && (
        <div>
          <h3 className="text-sm font-semibold text-vmm-text mb-3">VMs on this Host</h3>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
            {hostVms.map(vm => (
              <VmPriorityCard key={vm.id} vm={vm}
                onStart={() => startVm(vm.id)} onStop={() => stopVm(vm.id)}
                onClick={() => navigate(`/vms/${vm.id}`)} />
            ))}
          </div>
        </div>
      )}

      </>}

      {/* Maintenance dialog */}
      <MaintenanceDialog
        open={maintenanceOpen}
        onClose={() => setMaintenanceOpen(false)}
        host={host}
        onConfirm={async (mode) => {
          await api.post(`/api/hosts/${host.id}/maintenance`, { mode })
          fetchHosts()
        }}
      />

      {/* Edit Host Dialog */}
      <HostEditDialog
        open={editOpen}
        onClose={() => setEditOpen(false)}
        host={host}
        onSaved={fetchHosts}
      />
    </div>
  )
}
