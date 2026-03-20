import { useEffect, useState } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { Power, RotateCcw, Square, Monitor as MonitorIcon, Cpu, MemoryStick, HardDrive, RefreshCw, Camera } from 'lucide-react'
import api from '../api/client'
import type { VmDetail as VmDetailType } from '../api/types'
import StatusBadge from '../components/StatusBadge'
import Button from '../components/Button'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import SpecRow from '../components/SpecRow'
import ProgressBar from '../components/ProgressBar'
import TabBar from '../components/TabBar'
import ConsolePreview from '../components/ConsolePreview'
import ConsoleCanvas from '../components/ConsoleCanvas'
import ActivityCard from '../components/ActivityCard'
import QuickAction from '../components/QuickAction'
import { guestOsLabel, formatRam } from '../utils/format'
import { useVmStore } from '../stores/vmStore'

const tabs = [
  { id: 'general', label: 'General' },
  { id: 'hardware', label: 'Hardware' },
  { id: 'storage', label: 'Storage' },
  { id: 'network', label: 'Network' },
  { id: 'snapshots', label: 'Snapshots' },
]

export default function VmDetail() {
  const { id } = useParams<{ id: string }>()
  const navigate = useNavigate()
  const [vm, setVm] = useState<VmDetailType | null>(null)
  const [activeTab, setActiveTab] = useState('general')
  const { startVm, stopVm, forceStopVm } = useVmStore()

  useEffect(() => {
    if (!id) return
    const load = () => api.get<VmDetailType>(`/api/vms/${id}`).then(({ data }) => setVm(data))
    load()
    // Poll every 2s to detect state changes (e.g. VM exited)
    const interval = setInterval(load, 2000)
    return () => clearInterval(interval)
  }, [id])

  if (!vm) return <div className="text-vmm-text-muted">Loading...</div>

  const isRunning = vm.state === 'running'
  const isStopped = vm.state === 'stopped'

  const handleStart = async () => { await startVm(vm.id); setVm({ ...vm, state: 'running' }) }
  const handleStop = async () => { await stopVm(vm.id); setVm({ ...vm, state: 'stopping' }) }
  const handleForceStop = async () => { await forceStopVm(vm.id); setVm({ ...vm, state: 'stopped' }) }

  return (
    <div className="space-y-6">
      {/* ── VM Header ─────────────────────────────────────────────── */}
      <div className="flex items-start justify-between">
        <div className="flex items-center gap-4">
          {/* OS icon */}
          <div className="w-14 h-14 rounded-xl bg-vmm-danger/10 flex items-center justify-center">
            <MonitorIcon size={24} className="text-vmm-danger" />
          </div>
          <div>
            <div className="flex items-center gap-3">
              <h1 className="text-2xl font-bold text-vmm-text">{vm.name}</h1>
              <StatusBadge state={vm.state} />
            </div>
            <div className="flex items-center gap-2 text-sm text-vmm-text-muted mt-0.5">
              <MonitorIcon size={13} /> Node-Alpha-04 &bull; Uptime: 0d 0h 0m
            </div>
          </div>
        </div>

        {/* Power controls */}
        <div className="flex items-center gap-2">
          {isStopped ? (
            <Button variant="primary" size="lg" icon={<Power size={16} />} onClick={handleStart}>Power On</Button>
          ) : (
            <Button variant="outline" size="lg" icon={<Power size={16} />} onClick={handleStop}>Shutdown</Button>
          )}
          <Button variant="ghost" size="icon" onClick={() => {}} title="Reset">
            <RefreshCw size={16} />
          </Button>
          <Button variant="danger" size="icon" onClick={handleForceStop} title="Force Stop" disabled={isStopped}>
            <Square size={14} />
          </Button>
        </div>
      </div>

      {/* ── Tabs ──────────────────────────────────────────────────── */}
      <TabBar tabs={tabs} active={activeTab} onChange={setActiveTab} />

      {/* ── Content: General Tab ──────────────────────────────────── */}
      {activeTab === 'general' && (
        <div className="grid grid-cols-[1fr_300px] gap-6">
          {/* Left column */}
          <div className="space-y-6">
            {/* Console — live when running, placeholder when stopped */}
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
              <div className="grid grid-cols-2 gap-3">
                <ActivityCard icon={<RefreshCw size={16} />} title="System Shutdown" subtitle="2 hours ago by admin_user" />
                <ActivityCard icon={<Camera size={16} />} title="Snapshot: Pre-Update-Dev" subtitle="Yesterday at 14:22" />
              </div>
            </div>
          </div>

          {/* Right column — Specs + Quick Actions */}
          <div className="space-y-5">
            <Card>
              <SectionLabel className="mb-4">System Specifications</SectionLabel>
              <SpecRow icon={<MonitorIcon size={16} />} label="OS" value={guestOsLabel(vm.config.guest_os)} />
              <SpecRow icon={<Cpu size={16} />} label="Processors" value={`${vm.config.cpu_cores} vCPUs`} />
              <SpecRow icon={<MemoryStick size={16} />} label="Memory" value={formatRam(vm.config.ram_mb)} />
              <div className="border-t border-vmm-border pt-3 mt-3">
                <ProgressBar label="Disk Usage" detail="124 GB / 512 GB" value={24} />
              </div>
            </Card>

            <Card padding={false}>
              <div className="px-5 py-3">
                <SectionLabel>Quick Actions</SectionLabel>
              </div>
              <QuickAction label="Export OVF" onClick={() => {}} />
              <QuickAction label="Clone Machine" onClick={() => {}} />
            </Card>
          </div>
        </div>
      )}

      {/* Other tabs — placeholder */}
      {activeTab !== 'general' && (
        <Card>
          <div className="text-vmm-text-muted text-sm py-8 text-center">
            {activeTab.charAt(0).toUpperCase() + activeTab.slice(1)} configuration — coming soon
          </div>
        </Card>
      )}
    </div>
  )
}
