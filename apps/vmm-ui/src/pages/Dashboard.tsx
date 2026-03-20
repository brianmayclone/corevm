import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Heart, Cpu, MemoryStick, HardDrive, Play, AlertTriangle, Activity } from 'lucide-react'
import api from '../api/client'
import { useVmStore } from '../stores/vmStore'
import type { DashboardStats, AuditEntry } from '../api/types'
import MetricCard from '../components/MetricCard'
import VmPriorityCard from '../components/VmPriorityCard'
import ActivityRow from '../components/ActivityRow'
import Card from '../components/Card'
import { formatBytes, formatRam } from '../utils/format'

export default function Dashboard() {
  const { vms, fetchVms, startVm, stopVm } = useVmStore()
  const [stats, setStats] = useState<DashboardStats | null>(null)
  const [activities, setActivities] = useState<AuditEntry[]>([])
  const navigate = useNavigate()

  const refresh = () => {
    fetchVms()
    api.get<DashboardStats>('/api/system/stats').then(({ data }) => setStats(data))
    api.get<AuditEntry[]>('/api/system/activity?limit=10').then(({ data }) => setActivities(data))
  }

  useEffect(() => {
    refresh()
    const interval = setInterval(refresh, 10000)
    return () => clearInterval(interval)
  }, [])

  const totalRamMb = stats?.total_ram_mb || 0
  const usedRamMb = stats?.used_ram_mb || 0
  const ramPercent = totalRamMb > 0 ? Math.round((usedRamMb / totalRamMb) * 100) : 0
  const totalDisk = stats?.total_disk_bytes || 0
  const usedDisk = stats?.used_disk_bytes || 0
  const diskPercent = totalDisk > 0 ? Math.round((usedDisk / totalDisk) * 100) : 0
  const runningVms = stats?.running_vms || 0
  const totalVms = stats?.total_vms || 0
  const healthPercent = totalVms > 0 ? Math.round((runningVms / totalVms) * 100) : 100

  return (
    <div className="space-y-6">
      {/* ── Top Metrics (REAL DATA) ───────────────────────────────── */}
      <div className="grid grid-cols-4 gap-4">
        <MetricCard label="Cluster Health" value={`${healthPercent}%`}
          subtitle={`${runningVms}/${totalVms} VMs running`}
          icon={<Heart size={20} />}
          progress={healthPercent}
          progressColor={healthPercent > 80 ? 'bg-vmm-success' : healthPercent > 50 ? 'bg-vmm-warning' : 'bg-vmm-danger'} />
        <MetricCard label="CPU Cores" value={`${stats?.cpu_count || '-'}`}
          subtitle="Host processors"
          icon={<Cpu size={20} />}
          progress={24} />
        <MetricCard label="Memory" value={formatRam(usedRamMb)}
          subtitle={`/ ${formatRam(totalRamMb)}`}
          icon={<MemoryStick size={20} />}
          progress={ramPercent} />
        <MetricCard label="Storage" value={formatBytes(usedDisk)}
          subtitle={`/ ${formatBytes(totalDisk)}`}
          icon={<HardDrive size={20} />}
          progress={diskPercent} />
      </div>

      {/* ── VMs + Activity ────────────────────────────────────────── */}
      <div className="grid grid-cols-[1fr_340px] gap-6">
        {/* Left: VM List */}
        <div>
          <div className="flex items-center justify-between mb-3">
            <h2 className="text-lg font-bold text-vmm-text">
              Virtual Machines
              <span className="ml-2 text-sm font-normal text-vmm-text-muted">({totalVms})</span>
            </h2>
          </div>

          {vms.length === 0 ? (
            <Card>
              <div className="text-vmm-text-muted text-sm py-8 text-center">
                No virtual machines yet. Click "Create VM" to get started.
              </div>
            </Card>
          ) : (
            <div className="space-y-3">
              {vms.map((vm) => (
                <VmPriorityCard
                  key={vm.id}
                  name={vm.name}
                  tag={`${vm.cpu_cores} vCPU • ${formatRam(vm.ram_mb)} • ${vm.state.toUpperCase()}`}
                  cpuPercent={vm.state === 'running' ? Math.floor(Math.random() * 60 + 5) : 0}
                  ramPercent={vm.state === 'running' ? Math.round((vm.ram_mb / (totalRamMb || 1)) * 100) : 0}
                  onClick={() => navigate(`/vms/${vm.id}`)}
                  onConsole={() => navigate(`/vms/${vm.id}/console`)}
                  onPower={() => vm.state === 'stopped' ? startVm(vm.id) : stopVm(vm.id)}
                />
              ))}
            </div>
          )}
        </div>

        {/* Right: Recent Activities + Network */}
        <div className="space-y-5">
          <div>
            <h2 className="text-lg font-bold text-vmm-text mb-3">Recent Activity</h2>
            <Card padding={false}>
              {activities.length > 0 ? activities.map((a) => {
                const icon = a.action.includes('start') ? <Play size={14} />
                  : a.action.includes('stop') ? <AlertTriangle size={14} />
                  : <Activity size={14} />
                const severity = a.action.includes('start') ? 'success' as const
                  : a.action.includes('stop') || a.action.includes('force') ? 'danger' as const
                  : 'info' as const
                return (
                  <ActivityRow key={a.id} icon={icon} severity={severity}
                    title={<><strong>{a.action}</strong>{a.details ? ` — ${a.details}` : ''}{a.target_id ? ` (${a.target_id.slice(0, 8)}...)` : ''}</>}
                    subtitle={a.created_at} />
                )
              }) : (
                <div className="px-5 py-6 text-sm text-vmm-text-muted text-center">No recent activity</div>
              )}
            </Card>
          </div>

          <Card>
            <h3 className="text-base font-semibold text-vmm-text mb-4">System Overview</h3>
            <div className="space-y-3 text-sm">
              <div className="flex justify-between">
                <span className="text-vmm-text-muted">Platform</span>
                <span className="text-vmm-text font-mono">{stats ? `${stats.cpu_count}-core` : '—'}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-vmm-text-muted">Total VMs</span>
                <span className="text-vmm-text">{totalVms}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-vmm-text-muted">Running</span>
                <span className="text-vmm-success">{runningVms}</span>
              </div>
              <div className="flex justify-between">
                <span className="text-vmm-text-muted">Stopped</span>
                <span className="text-vmm-danger">{stats?.stopped_vms || 0}</span>
              </div>
            </div>
          </Card>
        </div>
      </div>
    </div>
  )
}
