import { useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { ArrowLeft, Server, Monitor, Shield, Activity, HardDrive, Circle, MemoryStick, Cpu } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import { useVmStore } from '../stores/vmStore'
import Card from '../components/Card'
import MetricCard from '../components/MetricCard'
import VmPriorityCard from '../components/VmPriorityCard'
import SectionLabel from '../components/SectionLabel'
import { formatRam, formatBytes } from '../utils/format'
import type { Cluster, Host } from '../api/types'

export default function ClusterDetail() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const { clusters, fetchClusters, hosts, fetchHosts, datastores, fetchDatastores } = useClusterStore()
  const { vms, fetchVms, startVm, stopVm } = useVmStore()
  const [cluster, setCluster] = useState<Cluster | null>(null)

  useEffect(() => { fetchClusters(); fetchHosts(); fetchDatastores(); fetchVms() }, [])
  useEffect(() => { setCluster(clusters.find(c => c.id === id) || null) }, [clusters, id])

  if (!cluster) return <div className="text-vmm-text-muted p-8">Loading...</div>

  const clusterHosts = hosts.filter(h => h.cluster_id === id)
  const clusterVms = vms.filter((v: any) => v.cluster_id === id)
  const clusterDatastores = datastores.filter(d => d.cluster_id === id)
  const onlineHosts = clusterHosts.filter(h => h.status === 'online')
  const runningVms = clusterVms.filter(v => v.state === 'running')
  const totalRam = clusterHosts.reduce((s, h) => s + h.total_ram_mb, 0)
  const freeRam = clusterHosts.reduce((s, h) => s + h.free_ram_mb, 0)
  const usedRam = totalRam - freeRam
  const ramPct = totalRam > 0 ? Math.round((usedRam / totalRam) * 100) : 0
  const avgCpu = clusterHosts.length > 0 ? clusterHosts.reduce((s, h) => s + h.cpu_usage_pct, 0) / clusterHosts.length : 0
  const totalDisk = clusterDatastores.reduce((s, d) => s + d.total_bytes, 0)
  const freeDisk = clusterDatastores.reduce((s, d) => s + d.free_bytes, 0)
  const diskPct = totalDisk > 0 ? Math.round(((totalDisk - freeDisk) / totalDisk) * 100) : 0

  return (
    <div className="space-y-5">
      <div className="flex items-center gap-3">
        <button onClick={() => navigate('/cluster/settings')} className="text-vmm-text-muted hover:text-vmm-text">
          <ArrowLeft size={20} />
        </button>
        <div className="flex-1">
          <h1 className="text-2xl font-bold text-vmm-text">{cluster.name}</h1>
          {cluster.description && <p className="text-sm text-vmm-text-muted">{cluster.description}</p>}
        </div>
        <div className="flex items-center gap-3 text-xs">
          <span className="flex items-center gap-1 px-2 py-1 rounded-full bg-vmm-surface">
            <Shield size={11} className={cluster.ha_enabled ? 'text-vmm-success' : 'text-vmm-text-muted'} />
            HA {cluster.ha_enabled ? 'On' : 'Off'}
          </span>
          <span className="flex items-center gap-1 px-2 py-1 rounded-full bg-vmm-surface">
            <Activity size={11} className={cluster.drs_enabled ? 'text-vmm-accent' : 'text-vmm-text-muted'} />
            DRS {cluster.drs_enabled ? 'On' : 'Off'}
          </span>
        </div>
      </div>

      {/* Cluster metrics */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3">
        <MetricCard label="Hosts" value={`${onlineHosts.length}/${clusterHosts.length}`}
          subtitle="Online" icon={<Server size={18} />}
          percent={clusterHosts.length > 0 ? Math.round((onlineHosts.length / clusterHosts.length) * 100) : 100}
          color="accent" />
        <MetricCard label="VMs" value={`${runningVms.length} Running`}
          subtitle={`${clusterVms.length} total`} icon={<Monitor size={18} />}
          percent={clusterVms.length > 0 ? Math.round((runningVms.length / clusterVms.length) * 100) : 100}
          color="accent" />
        <MetricCard label="CPU" value={`${avgCpu.toFixed(0)}% Avg`}
          subtitle={`${clusterHosts.reduce((s, h) => s + h.cpu_cores, 0)} total cores`}
          icon={<Cpu size={18} />} percent={Math.round(avgCpu)}
          color={avgCpu > 80 ? 'danger' : avgCpu > 60 ? 'warning' : 'accent'} />
        <MetricCard label="Memory" value={`${ramPct}%`}
          subtitle={`${formatRam(usedRam)} / ${formatRam(totalRam)}`}
          icon={<MemoryStick size={18} />} percent={ramPct}
          color={ramPct > 90 ? 'danger' : ramPct > 70 ? 'warning' : 'accent'} />
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-5">
        {/* Hosts in this cluster */}
        <div>
          <SectionLabel>Hosts ({clusterHosts.length})</SectionLabel>
          <div className="space-y-2 mt-2">
            {clusterHosts.map(host => {
              const hRamPct = host.total_ram_mb > 0 ? Math.round(((host.total_ram_mb - host.free_ram_mb) / host.total_ram_mb) * 100) : 0
              return (
                <Card key={host.id}>
                  <div onClick={() => navigate(`/cluster/hosts/${host.id}`)}
                    className="p-3 flex items-center gap-3 cursor-pointer hover:bg-vmm-surface-hover rounded-lg">
                    <Circle size={6} className={host.status === 'online' ? 'fill-vmm-success text-vmm-success' : host.status === 'maintenance' ? 'fill-yellow-400 text-yellow-400' : 'fill-vmm-danger text-vmm-danger'} />
                    <div className="flex-1">
                      <div className="text-sm font-medium text-vmm-text">{host.hostname}</div>
                      <div className="text-[10px] text-vmm-text-muted">{host.vm_count} VMs &bull; CPU {host.cpu_usage_pct.toFixed(0)}% &bull; RAM {hRamPct}%</div>
                    </div>
                    <div className="w-20">
                      <div className="w-full bg-vmm-bg rounded-full h-1.5">
                        <div className={`h-1.5 rounded-full ${hRamPct > 90 ? 'bg-vmm-danger' : 'bg-vmm-accent'}`} style={{ width: `${hRamPct}%` }} />
                      </div>
                    </div>
                  </div>
                </Card>
              )
            })}
          </div>
        </div>

        {/* Datastores in this cluster */}
        <div>
          <SectionLabel>Datastores ({clusterDatastores.length})</SectionLabel>
          <div className="space-y-2 mt-2">
            {clusterDatastores.map(ds => {
              const dsPct = ds.total_bytes > 0 ? Math.round(((ds.total_bytes - ds.free_bytes) / ds.total_bytes) * 100) : 0
              return (
                <Card key={ds.id}>
                  <div className="p-3">
                    <div className="flex items-center justify-between mb-1">
                      <div className="flex items-center gap-2">
                        <HardDrive size={12} className="text-vmm-text-muted" />
                        <span className="text-sm font-medium text-vmm-text">{ds.name}</span>
                        <span className="text-[10px] px-1.5 py-0.5 rounded bg-vmm-surface text-vmm-text-muted">{ds.store_type}</span>
                      </div>
                      <span className="text-[10px] text-vmm-text-muted">{ds.host_mounts.filter(m => m.mounted).length}/{ds.host_mounts.length} mounted</span>
                    </div>
                    <div className="w-full bg-vmm-bg rounded-full h-1.5 mt-1">
                      <div className={`h-1.5 rounded-full ${dsPct > 90 ? 'bg-vmm-danger' : 'bg-vmm-accent'}`} style={{ width: `${dsPct}%` }} />
                    </div>
                    <div className="text-[10px] text-vmm-text-muted mt-1">{formatBytes(ds.total_bytes - ds.free_bytes)} / {formatBytes(ds.total_bytes)}</div>
                  </div>
                </Card>
              )
            })}
            {clusterDatastores.length === 0 && (
              <div className="text-xs text-vmm-text-muted text-center py-6">No datastores in this cluster</div>
            )}
          </div>
        </div>
      </div>

      {/* VMs in this cluster */}
      {clusterVms.length > 0 && (
        <div>
          <SectionLabel>Virtual Machines ({clusterVms.length})</SectionLabel>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3 mt-2">
            {clusterVms.map(vm => (
              <VmPriorityCard key={vm.id} vm={vm}
                onStart={() => startVm(vm.id)} onStop={() => stopVm(vm.id)}
                onClick={() => navigate(`/vms/${vm.id}`)} />
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
