import { useEffect } from 'react'
import { useNavigate } from 'react-router-dom'
import { Server, Monitor, HardDrive, Shield, Activity, AlertTriangle, Circle, Workflow, Bell } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import { useVmStore } from '../stores/vmStore'
import { useUiStore } from '../stores/uiStore'
import MetricCard from '../components/MetricCard'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import { formatBytes, formatRam } from '../utils/format'
import EventFeed from '../components/EventFeed'

export default function ClusterDashboard() {
  const navigate = useNavigate()
  const { dashboardRefreshSecs } = useUiStore()
  const {
    clusterStats, fetchClusterStats,
    hosts, fetchHosts,
    clusters, fetchClusters,
    datastores, fetchDatastores,
    events, fetchEvents,
    alarms, fetchAlarms,
    drsRecommendations, fetchDrsRecommendations,
  } = useClusterStore()
  const { vms, fetchVms } = useVmStore()

  const refresh = () => {
    fetchClusterStats(); fetchHosts(); fetchClusters()
    fetchDatastores(); fetchVms(); fetchEvents()
    fetchAlarms(); fetchDrsRecommendations()
  }

  useEffect(() => {
    refresh()
    const interval = setInterval(refresh, dashboardRefreshSecs * 1000)
    return () => clearInterval(interval)
  }, [])

  const s = clusterStats
  const totalRam = s?.total_ram_mb || 0
  const usedRam = s?.used_ram_mb || 0
  const ramPct = totalRam > 0 ? Math.round((usedRam / totalRam) * 100) : 0
  const totalDisk = s?.total_disk_bytes || 0
  const usedDisk = s?.used_disk_bytes || 0
  const diskPct = totalDisk > 0 ? Math.round((usedDisk / totalDisk) * 100) : 0
  const onlineHosts = s?.online_hosts || 0
  const totalHosts = s?.total_hosts || 0
  const runningVms = s?.running_vms || 0
  const totalVms = s?.total_vms || 0
  const triggeredAlarms = alarms.filter(a => a.triggered && !a.acknowledged)
  const pendingDrs = drsRecommendations.length
  const recentEvents = events.slice(0, 8)

  return (
    <div className="space-y-5">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">Cluster Overview</h1>
        <p className="text-sm text-vmm-text-muted mt-1">{clusters.length} clusters &bull; {totalHosts} hosts &bull; {totalVms} VMs</p>
      </div>

      {/* Top metrics */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-3">
        <MetricCard label="Hosts" value={`${onlineHosts}/${totalHosts} Online`}
          subtitle={`${s?.maintenance_hosts || 0} maintenance, ${s?.offline_hosts || 0} offline`}
          icon={<Server size={18} />} percent={totalHosts > 0 ? Math.round((onlineHosts / totalHosts) * 100) : 100}
          color={onlineHosts === totalHosts ? 'accent' : 'warning'} />
        <MetricCard label="Virtual Machines" value={`${runningVms} Running`}
          subtitle={`${totalVms} total, ${s?.stopped_vms || 0} stopped`}
          icon={<Monitor size={18} />} percent={totalVms > 0 ? Math.round((runningVms / totalVms) * 100) : 100}
          color="accent" />
        <MetricCard label="Memory" value={`${ramPct}% Used`}
          subtitle={`${formatRam(usedRam)} / ${formatRam(totalRam)}`}
          icon={<Activity size={18} />} percent={ramPct}
          color={ramPct > 90 ? 'danger' : ramPct > 70 ? 'warning' : 'accent'} />
        <MetricCard label="Storage" value={`${diskPct}% Used`}
          subtitle={`${formatBytes(usedDisk)} / ${formatBytes(totalDisk)}`}
          icon={<HardDrive size={18} />} percent={diskPct}
          color={diskPct > 90 ? 'danger' : diskPct > 70 ? 'warning' : 'accent'} />
      </div>

      {/* Warnings / Alerts */}
      {(triggeredAlarms.length > 0 || pendingDrs > 0) && (
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
          {triggeredAlarms.length > 0 && (
            <Card>
              <div onClick={() => navigate('/operations/alarms')} className="p-4 cursor-pointer hover:bg-vmm-surface-hover rounded-lg">
                <div className="flex items-center gap-2 text-vmm-danger mb-2">
                  <AlertTriangle size={16} />
                  <span className="text-sm font-semibold">{triggeredAlarms.length} Active Alarms</span>
                </div>
                {triggeredAlarms.slice(0, 3).map(a => (
                  <div key={a.id} className="text-xs text-vmm-text-muted py-0.5">{a.name} ({a.severity})</div>
                ))}
              </div>
            </Card>
          )}
          {pendingDrs > 0 && (
            <Card>
              <div onClick={() => navigate('/cluster/drs')} className="p-4 cursor-pointer hover:bg-vmm-surface-hover rounded-lg">
                <div className="flex items-center gap-2 text-vmm-accent mb-2">
                  <Activity size={16} />
                  <span className="text-sm font-semibold">{pendingDrs} DRS Recommendations</span>
                </div>
                <div className="text-xs text-vmm-text-muted">Resource rebalancing suggestions available</div>
              </div>
            </Card>
          )}
        </div>
      )}

      <div className="grid grid-cols-1 lg:grid-cols-[1fr_340px] gap-5">
        {/* Hosts overview */}
        <div>
          <SectionLabel>Hosts</SectionLabel>
          <div className="space-y-2 mt-2">
            {hosts.map(host => {
              const hostRamPct = host.total_ram_mb > 0
                ? Math.round(((host.total_ram_mb - host.free_ram_mb) / host.total_ram_mb) * 100) : 0
              return (
                <Card key={host.id}>
                  <div onClick={() => navigate(`/cluster/hosts/${host.id}`)}
                    className="p-3 flex items-center gap-3 cursor-pointer hover:bg-vmm-surface-hover rounded-lg">
                    <Circle size={6} className={
                      host.status === 'online' ? 'fill-vmm-success text-vmm-success' :
                      host.status === 'maintenance' ? 'fill-yellow-400 text-yellow-400' :
                      'fill-vmm-danger text-vmm-danger'
                    } />
                    <div className="flex-1 min-w-0">
                      <div className="text-sm font-medium text-vmm-text">{host.hostname}</div>
                      <div className="text-[10px] text-vmm-text-muted">{host.vm_count} VMs &bull; CPU {host.cpu_usage_pct.toFixed(0)}%</div>
                    </div>
                    <div className="w-24">
                      <div className="text-[10px] text-vmm-text-muted text-right mb-0.5">RAM {hostRamPct}%</div>
                      <div className="w-full bg-vmm-bg rounded-full h-1.5">
                        <div className={`h-1.5 rounded-full ${hostRamPct > 90 ? 'bg-vmm-danger' : 'bg-vmm-accent'}`}
                          style={{ width: `${hostRamPct}%` }} />
                      </div>
                    </div>
                  </div>
                </Card>
              )
            })}
          </div>
        </div>

        {/* Recent events */}
        <div>
          <SectionLabel>Recent Events</SectionLabel>
          <Card>
            <div className="divide-y divide-vmm-border">
              {recentEvents.length === 0 ? (
                <div className="text-xs text-vmm-text-muted py-6 text-center">No recent events</div>
              ) : recentEvents.map(event => (
                <div key={event.id} className="px-3 py-2">
                  <div className="flex items-start gap-2">
                    <Circle size={5} className={`mt-1.5 flex-shrink-0 ${
                      event.severity === 'critical' || event.severity === 'error' ? 'fill-vmm-danger text-vmm-danger' :
                      event.severity === 'warning' ? 'fill-yellow-400 text-yellow-400' :
                      'fill-vmm-accent text-vmm-accent'
                    }`} />
                    <div className="min-w-0">
                      <div className="text-xs text-vmm-text leading-relaxed">{event.message}</div>
                      <div className="text-[10px] text-vmm-text-muted mt-0.5">
                        {new Date(event.created_at).toLocaleTimeString()}
                      </div>
                    </div>
                  </div>
                </div>
              ))}
            </div>
          </Card>

          <div className="mt-4">
            <SectionLabel>Datastores</SectionLabel>
            <div className="space-y-2 mt-2">
              {datastores.map(ds => {
                const dsPct = ds.total_bytes > 0 ? Math.round(((ds.total_bytes - ds.free_bytes) / ds.total_bytes) * 100) : 0
                return (
                  <Card key={ds.id}>
                    <div className="p-3">
                      <div className="flex items-center justify-between mb-1">
                        <span className="text-xs font-medium text-vmm-text">{ds.name}</span>
                        <span className="text-[10px] text-vmm-text-muted">{ds.store_type}</span>
                      </div>
                      <div className="w-full bg-vmm-bg rounded-full h-1.5">
                        <div className={`h-1.5 rounded-full ${dsPct > 90 ? 'bg-vmm-danger' : 'bg-vmm-accent'}`}
                          style={{ width: `${dsPct}%` }} />
                      </div>
                      <div className="text-[10px] text-vmm-text-muted mt-1">
                        {formatBytes(ds.total_bytes - ds.free_bytes)} / {formatBytes(ds.total_bytes)}
                      </div>
                    </div>
                  </Card>
                )
              })}
            </div>
          </div>
        </div>
      </div>

      {/* Recent cluster events */}
      <EventFeed limit={20} title="Recent Cluster Events" />
    </div>
  )
}
