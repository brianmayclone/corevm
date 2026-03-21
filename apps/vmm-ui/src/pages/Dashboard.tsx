import { useEffect, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { Heart, Cpu, MemoryStick, HardDrive, Play, AlertTriangle, Activity, Upload, Layout, Network } from 'lucide-react'
import api from '../api/client'
import { useVmStore } from '../stores/vmStore'
import type { DashboardStats, AuditEntry, NetworkStats } from '../api/types'
import MetricCard from '../components/MetricCard'
import VmPriorityCard from '../components/VmPriorityCard'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import { formatBytes, formatRam } from '../utils/format'
import { formatJournalLine, getActionSeverity } from '../utils/auditLabels'
import { useUiStore } from '../stores/uiStore'

export default function Dashboard() {
  const { vms, fetchVms, startVm, stopVm } = useVmStore()
  const [stats, setStats] = useState<DashboardStats | null>(null)
  const [netStats, setNetStats] = useState<NetworkStats | null>(null)
  const [activities, setActivities] = useState<AuditEntry[]>([])
  const navigate = useNavigate()
  const { dashboardRefreshSecs } = useUiStore()

  const refresh = () => {
    fetchVms()
    api.get<DashboardStats>('/api/system/stats').then(({ data }) => setStats(data))
    api.get<NetworkStats>('/api/network/stats').then(({ data }) => setNetStats(data)).catch(() => {})
    api.get<AuditEntry[]>('/api/system/activity?limit=8').then(({ data }) => setActivities(data))
  }

  useEffect(() => {
    refresh()
    const interval = setInterval(refresh, dashboardRefreshSecs * 1000)
    return () => clearInterval(interval)
  }, [])

  const totalRamMb = stats?.total_ram_mb || 0
  const usedRamMb = stats?.used_ram_mb || 0
  const ramPercent = totalRamMb > 0 ? Math.round((usedRamMb / totalRamMb) * 100) : 0
  const totalDisk = stats?.total_disk_bytes || 0
  const usedDisk = stats?.used_disk_bytes || 0
  const runningVms = stats?.running_vms || 0
  const totalVms = stats?.total_vms || 0
  const healthPercent = totalVms > 0 ? Math.round((runningVms / totalVms) * 100) : 100

  return (
    <div className="flex flex-col lg:h-full lg:min-h-0 space-y-5 lg:space-y-0">
      {/* ── Top Metrics ─────────────────────────────────────────────── */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3 sm:gap-4 lg:mb-5 flex-shrink-0">
        <MetricCard label="System Performance" value={healthPercent > 80 ? 'Optimal State' : `${healthPercent}%`}
          subtitle={`${formatRam(totalRamMb)} Total RAM • ${runningVms}/${totalVms} Active`}
          icon={<Heart size={20} />}
          progress={healthPercent}
          progressColor={healthPercent > 80 ? 'bg-vmm-success' : 'bg-vmm-warning'} />
        <MetricCard label="Network Traffic" value={formatBytes(netStats?.total_rx_bytes || 0)}
          subtitle="Total ingress"
          icon={<Network size={20} />}
          progress={42} />
        <MetricCard label="Storage Pools" value={formatBytes(totalDisk)}
          subtitle={`${formatBytes(usedDisk)} used`}
          icon={<HardDrive size={20} />}
          progress={totalDisk > 0 ? Math.round((usedDisk / totalDisk) * 100) : 0} />
      </div>

      {/* ── Instances ───────────────────────────────────────────────── */}
      <div className="lg:flex-1 lg:min-h-0 lg:mb-5">
        <div className="flex items-center justify-between mb-3 flex-shrink-0">
          <h2 className="text-lg font-bold text-vmm-text">
            Instances
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
          <div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-3 lg:overflow-y-auto lg:max-h-[calc(100vh-480px)]">
            {vms.map((vm) => (
              <VmPriorityCard
                key={vm.id}
                name={vm.name}
                guestOs={vm.guest_os}
                state={vm.state}
                tag={`${vm.cpu_cores} vCPU • ${formatRam(vm.ram_mb)}`}
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

      {/* ── System Journal + Quick Deploy (bottom) ──────────────────── */}
      <div className="grid grid-cols-1 lg:grid-cols-[1fr_300px] gap-5 lg:flex-shrink-0">
        {/* System Journal */}
        <Card padding={false}>
          <div className="flex items-center justify-between px-5 py-3 border-b border-vmm-border">
            <div className="flex items-center gap-2">
              <SectionLabel>System Journal</SectionLabel>
              <span className="flex items-center gap-1.5 text-[10px] text-vmm-success">
                <span className="w-1.5 h-1.5 rounded-full bg-vmm-success animate-pulse" />
                Live monitoring enabled
              </span>
            </div>
            <button className="text-xs text-vmm-text-muted hover:text-vmm-text cursor-pointer">Clear Logs</button>
          </div>
          <div className="px-5 py-3 font-mono text-xs leading-6 max-h-[180px] overflow-y-auto text-vmm-text-dim">
            {activities.length > 0 ? activities.map((a) => {
              const sev = getActionSeverity(a.action)
              const cls = sev === 'danger' ? 'text-vmm-danger font-bold'
                : sev === 'warning' ? 'text-vmm-warning font-bold'
                : sev === 'success' ? 'text-vmm-success' : ''
              return (
                <div key={a.id} className={cls}>
                  {formatJournalLine(a)}
                </div>
              )
            }) : (
              <div className="text-vmm-text-muted">No log entries yet.</div>
            )}
          </div>
        </Card>

        {/* Quick Deploy */}
        <Card>
          <SectionLabel className="mb-3">Quick Deploy</SectionLabel>
          <div className="grid grid-cols-2 gap-3 mb-3">
            <button
              onClick={() => navigate('/storage')}
              className="flex flex-col items-center gap-2 p-4 bg-vmm-bg-alt rounded-lg hover:bg-vmm-surface-hover transition-colors cursor-pointer"
            >
              <Upload size={20} className="text-vmm-accent" />
              <span className="text-xs text-vmm-text-dim">Upload ISO</span>
            </button>
            <button
              onClick={() => navigate('/vms/create')}
              className="flex flex-col items-center gap-2 p-4 bg-vmm-bg-alt rounded-lg hover:bg-vmm-surface-hover transition-colors cursor-pointer"
            >
              <Layout size={20} className="text-vmm-accent" />
              <span className="text-xs text-vmm-text-dim">Templates</span>
            </button>
          </div>
          <div className="bg-vmm-accent/5 border border-vmm-accent/20 rounded-lg p-3">
            <p className="text-[11px] text-vmm-accent italic">
              "Pro-tip: Use snapshots before major kernel upgrades to ensure 100% recovery."
            </p>
          </div>
        </Card>
      </div>
    </div>
  )
}
