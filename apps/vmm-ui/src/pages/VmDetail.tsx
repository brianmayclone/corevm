import { useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { Power, RefreshCw, Square, Cpu, MemoryStick, HardDrive, Camera, Clock, FileText } from 'lucide-react'
import api from '../api/client'
import type { VmDetail as VmDetailType, AuditEntry } from '../api/types'
import StatusBadge from '../components/StatusBadge'
import Button from '../components/Button'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import SpecRow from '../components/SpecRow'
import ProgressBar from '../components/ProgressBar'
import TabBar from '../components/TabBar'
import ConsoleCanvas from '../components/ConsoleCanvas'
import ConsolePreview from '../components/ConsolePreview'
import ActivityCard from '../components/ActivityCard'
import QuickAction from '../components/QuickAction'
import OsIcon from '../components/OsIcon'
import { guestOsLabel, formatRam, formatBytes } from '../utils/format'
import { useVmStore } from '../stores/vmStore'

const tabs = [
  { id: 'general', label: 'General' },
  { id: 'storage', label: 'Storage' },
  { id: 'network', label: 'Network' },
  { id: 'snapshots', label: 'Snapshots' },
  { id: 'logs', label: 'Logs' },
]

export default function VmDetail() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const [vm, setVm] = useState<VmDetailType | null>(null)
  const [activeTab, setActiveTab] = useState('general')
  const [activities, setActivities] = useState<AuditEntry[]>([])
  const { startVm, stopVm, forceStopVm } = useVmStore()

  useEffect(() => {
    if (!id) return
    const load = () => {
      api.get<VmDetailType>(`/api/vms/${id}`).then(({ data }) => setVm(data))
      api.get<AuditEntry[]>(`/api/system/activity?limit=5&target_id=${id}`).then(({ data }) => setActivities(data)).catch(() => {})
    }
    load()
    const interval = setInterval(load, 3000)
    return () => clearInterval(interval)
  }, [id])

  if (!vm) return <div className="text-vmm-text-muted">Loading...</div>

  const isRunning = vm.state === 'running'
  const isStopped = vm.state === 'stopped'

  const handleStart = async () => { await startVm(vm.id); setVm({ ...vm, state: 'running' }) }
  const handleStop = async () => { await stopVm(vm.id); setVm({ ...vm, state: 'stopping' }) }
  const handleForceStop = async () => { await forceStopVm(vm.id); setVm({ ...vm, state: 'stopped' }) }

  // Compute total disk size and usage from real data
  const totalDiskBytes = vm.disks.reduce((sum, d) => sum + d.size_bytes, 0)
  const usedDiskBytes = vm.disks.reduce((sum, d) => sum + d.used_bytes, 0)
  const diskPercent = totalDiskBytes > 0 ? Math.round((usedDiskBytes / totalDiskBytes) * 100) : 0

  return (
    <div className="space-y-6">
      {/* ── VM Header ─────────────────────────────────────────────── */}
      <div className="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-3">
        <div className="flex items-center gap-3 sm:gap-4 min-w-0">
          <OsIcon guestOs={vm.config.guest_os} size={44} className="flex-shrink-0 sm:w-14 sm:h-14" />
          <div className="min-w-0">
            <div className="flex items-center gap-2 sm:gap-3 flex-wrap">
              <h1 className="text-lg sm:text-2xl font-bold text-vmm-text truncate">{vm.name}</h1>
              <StatusBadge state={vm.state} size="sm" />
            </div>
            <div className="flex items-center gap-2 text-xs sm:text-sm text-vmm-text-muted mt-0.5">
              <Clock size={12} /> Created {vm.created_at}
            </div>
          </div>
        </div>

        {/* Power controls */}
        <div className="flex items-center gap-2 flex-shrink-0">
          {isStopped ? (
            <Button variant="primary" size="sm" icon={<Power size={14} />} onClick={handleStart}>Power On</Button>
          ) : (
            <Button variant="outline" size="sm" icon={<Power size={14} />} onClick={handleStop}>Shutdown</Button>
          )}
          <Button variant="ghost" size="icon" onClick={() => {}} title="Reset">
            <RefreshCw size={14} />
          </Button>
          <Button variant="danger" size="icon" onClick={handleForceStop} title="Force Stop" disabled={isStopped}>
            <Square size={12} />
          </Button>
        </div>
      </div>

      {/* ── Tabs ──────────────────────────────────────────────────── */}
      <TabBar tabs={tabs} active={activeTab} onChange={setActiveTab} />

      {/* ── General Tab ────────────────────────────────────────────── */}
      {activeTab === 'general' && (
        <div className="grid grid-cols-1 lg:grid-cols-[1fr_300px] gap-6">
          <div className="space-y-6">
            {/* Console */}
            {isRunning ? (
              <ConsoleCanvas vmId={vm.id} />
            ) : (
              <ConsolePreview state="off" />
            )}

            {/* Recent Activity */}
            <div>
              <h2 className="text-base font-semibold text-vmm-text flex items-center gap-2 mb-3">
                <RefreshCw size={15} className="text-vmm-text-muted" /> Recent Activity
              </h2>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                {activities.length > 0 ? activities.slice(0, 4).map((a) => (
                  <ActivityCard
                    key={a.id}
                    icon={a.action.includes('snapshot') ? <Camera size={16} /> : <RefreshCw size={16} />}
                    title={a.action}
                    subtitle={`${a.created_at}${a.details ? ` — ${a.details}` : ''}`}
                  />
                )) : (
                  <>
                    <ActivityCard icon={<RefreshCw size={16} />} title="No activity yet" subtitle="" />
                  </>
                )}
              </div>
            </div>
          </div>

          {/* Right column */}
          <div className="space-y-5">
            <Card>
              <SectionLabel className="mb-4">System Specifications</SectionLabel>
              <SpecRow icon={<OsIcon guestOs={vm.config.guest_os} size={20} />} label="OS" value={guestOsLabel(vm.config.guest_os)} />
              <SpecRow icon={<Cpu size={16} />} label="Processors" value={`${vm.config.cpu_cores} vCPUs`} />
              <SpecRow icon={<MemoryStick size={16} />} label="Memory" value={formatRam(vm.config.ram_mb)} />
              <div className="border-t border-vmm-border pt-3 mt-3">
                <ProgressBar
                  label="Disk Usage"
                  detail={`${formatBytes(usedDiskBytes)} / ${formatBytes(totalDiskBytes)}`}
                  value={diskPercent}
                />
              </div>
            </Card>

            <Card padding={false}>
              <div className="px-5 py-3">
                <SectionLabel>Quick Actions</SectionLabel>
              </div>
              <QuickAction label="Take Snapshot" onClick={() => setActiveTab('snapshots')} />
              <QuickAction label="Edit Settings" onClick={() => {
                if (isStopped) navigate(`/vms/${vm.id}/settings`)
                else alert('VM must be stopped to edit settings.')
              }} />
              <QuickAction label="Clone Machine" onClick={() => {}} />
            </Card>
          </div>
        </div>
      )}

      {/* ── Storage Tab ────────────────────────────────────────────── */}
      {activeTab === 'storage' && (
        <div className="space-y-4">
          <Card>
            <SectionLabel className="mb-4">Attached Disks</SectionLabel>
            {vm.disks.length === 0 ? (
              <div className="text-vmm-text-muted text-sm">No disks attached</div>
            ) : (
              <div className="space-y-3">
                {vm.disks.map((disk, i) => {
                  const pct = disk.size_bytes > 0 ? Math.round((disk.used_bytes / disk.size_bytes) * 100) : 0
                  return (
                    <div key={i} className="bg-vmm-bg-alt rounded-lg p-4">
                      <div className="flex items-center justify-between mb-2">
                        <div className="flex items-center gap-2">
                          <HardDrive size={16} className="text-vmm-text-muted" />
                          <span className="text-sm font-semibold text-vmm-text">Disk {i}</span>
                          <span className="text-xs text-vmm-text-muted font-mono">({disk.path.split('/').pop()})</span>
                        </div>
                        <span className="text-xs text-vmm-text-muted">{formatBytes(disk.size_bytes)}</span>
                      </div>
                      <ProgressBar
                        label="Used"
                        detail={`${formatBytes(disk.used_bytes)} / ${formatBytes(disk.size_bytes)}`}
                        value={pct}
                      />
                      <div className="text-[10px] text-vmm-text-muted mt-1 font-mono truncate">{disk.path}</div>
                    </div>
                  )
                })}
              </div>
            )}
          </Card>

          {vm.config.iso_image && (
            <Card>
              <SectionLabel className="mb-3">CD-ROM / ISO</SectionLabel>
              <div className="flex items-center gap-2 text-sm text-vmm-text">
                <FileText size={14} className="text-vmm-text-muted" />
                <span className="font-mono text-xs truncate">{vm.config.iso_image}</span>
              </div>
            </Card>
          )}
        </div>
      )}

      {/* ── Network Tab ────────────────────────────────────────────── */}
      {activeTab === 'network' && (
        <Card>
          <SectionLabel className="mb-4">Network Configuration</SectionLabel>
          <div className="space-y-2 text-sm">
            <div className="flex justify-between"><span className="text-vmm-text-muted">Enabled</span><span className="text-vmm-text">{vm.config.net_enabled ? 'Yes' : 'No'}</span></div>
            <div className="flex justify-between"><span className="text-vmm-text-muted">NIC Model</span><span className="text-vmm-text font-mono">{vm.config.nic_model}</span></div>
            <div className="flex justify-between"><span className="text-vmm-text-muted">Mode</span><span className="text-vmm-text">{vm.config.net_mode}</span></div>
            <div className="flex justify-between"><span className="text-vmm-text-muted">MAC Address</span><span className="text-vmm-text font-mono">{vm.config.mac_address || 'Auto'}</span></div>
            {vm.config.net_host_nic && (
              <div className="flex justify-between"><span className="text-vmm-text-muted">Host NIC</span><span className="text-vmm-text font-mono">{vm.config.net_host_nic}</span></div>
            )}
          </div>
        </Card>
      )}

      {/* ── Snapshots Tab ──────────────────────────────────────────── */}
      {activeTab === 'snapshots' && (
        <Card>
          <div className="flex items-center justify-between mb-4">
            <SectionLabel>Snapshots</SectionLabel>
            <Button variant="primary" size="sm" icon={<Camera size={14} />} onClick={() => {}}>
              Take Snapshot
            </Button>
          </div>
          <div className="text-vmm-text-muted text-sm py-8 text-center">
            No snapshots yet. Take a snapshot to save the current VM state.
          </div>
        </Card>
      )}

      {/* ── Logs Tab ───────────────────────────────────────────────── */}
      {activeTab === 'logs' && (
        <Card>
          <SectionLabel className="mb-4">VM Activity Log</SectionLabel>
          {activities.length === 0 ? (
            <div className="text-vmm-text-muted text-sm py-8 text-center">No log entries</div>
          ) : (
            <div className="space-y-2">
              {activities.map((a) => (
                <div key={a.id} className="flex items-start gap-3 text-sm py-2 border-b border-vmm-border last:border-0">
                  <span className="text-[10px] text-vmm-text-muted font-mono whitespace-nowrap mt-0.5">{a.created_at}</span>
                  <span className="text-vmm-text">{a.action}</span>
                  {a.details && <span className="text-vmm-text-muted">— {a.details}</span>}
                </div>
              ))}
            </div>
          )}
        </Card>
      )}
    </div>
  )
}
