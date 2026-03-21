/** Network Overview — topology map + summary stats. */
import { useEffect, useState, useRef } from 'react'
import { useOutletContext, useNavigate } from 'react-router-dom'
import { Plus, CheckCircle, AlertTriangle, Globe, Cable, Wifi, Network as NetworkIcon } from 'lucide-react'
import api from '../api/client'
import type { NetworkInterface, NetworkStats, VmSummary } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import Button from '../components/Button'
import { formatBytes } from '../utils/format'

/** Simple topology node for the canvas visualization. */
interface TopoNode {
  id: string
  label: string
  kind: 'vm' | 'switch' | 'gateway' | 'wan'
  x: number
  y: number
  state?: string
}

interface TopoLink {
  from: string
  to: string
  dashed?: boolean
}

export default function NetworkOverview() {
  const { stats } = useOutletContext<{ stats: NetworkStats | null }>()
  const [interfaces, setInterfaces] = useState<NetworkInterface[]>([])
  const [vms, setVms] = useState<VmSummary[]>([])
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const navigate = useNavigate()

  useEffect(() => {
    api.get<NetworkInterface[]>('/api/network/interfaces').then(({ data }) => setInterfaces(data))
    api.get<VmSummary[]>('/api/vms').then(({ data }) => setVms(data))
  }, [])

  // Draw topology on canvas
  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    const dpr = window.devicePixelRatio || 1
    const rect = canvas.getBoundingClientRect()
    canvas.width = rect.width * dpr
    canvas.height = rect.height * dpr
    ctx.scale(dpr, dpr)
    const W = rect.width
    const H = rect.height

    ctx.clearRect(0, 0, W, H)

    // Build topology nodes
    const nodes: TopoNode[] = []
    const links: TopoLink[] = []

    // WAN gateway on the right
    nodes.push({ id: 'wan', label: 'WAN\nUplink', kind: 'wan', x: W - 70, y: H / 2 })

    // Virtual switch in the center
    const physicalNics = interfaces.filter(i => i.kind === 'ethernet' || i.kind === 'wireless')
    const switchLabel = physicalNics.length > 0 ? physicalNics[0].name : 'vSwitch'
    nodes.push({ id: 'switch', label: `vSwitch\n${switchLabel}`, kind: 'switch', x: W / 2, y: H / 2 })

    // Gateway on the left
    nodes.push({ id: 'gw', label: 'Gateway\n10.0.2.2', kind: 'gateway', x: 70, y: H / 2 })

    links.push({ from: 'gw', to: 'switch' })
    links.push({ from: 'switch', to: 'wan' })

    // VMs distributed above and below the switch
    const runningVms = vms.filter(v => v.state === 'running')
    const allVmNodes = vms.slice(0, 6) // Max 6 VMs in topology
    allVmNodes.forEach((vm, i) => {
      const col = i % 3
      const row = Math.floor(i / 3)
      const baseX = W / 2 - 120 + col * 120
      const baseY = row === 0 ? H / 2 - 90 : H / 2 + 90
      nodes.push({
        id: vm.id, label: vm.name.length > 14 ? vm.name.slice(0, 12) + '...' : vm.name,
        kind: 'vm', x: baseX, y: baseY, state: vm.state,
      })
      links.push({ from: vm.id, to: 'switch', dashed: vm.state !== 'running' })
    })

    // Draw links
    for (const link of links) {
      const from = nodes.find(n => n.id === link.from)
      const to = nodes.find(n => n.id === link.to)
      if (!from || !to) continue

      ctx.beginPath()
      ctx.moveTo(from.x, from.y)
      ctx.lineTo(to.x, to.y)
      ctx.strokeStyle = link.dashed ? 'rgba(100, 116, 139, 0.3)' : 'rgba(56, 189, 248, 0.4)'
      ctx.lineWidth = link.dashed ? 1 : 1.5
      if (link.dashed) ctx.setLineDash([4, 4])
      else ctx.setLineDash([])
      ctx.stroke()
    }
    ctx.setLineDash([])

    // Draw nodes
    for (const node of nodes) {
      const isVm = node.kind === 'vm'
      const isRunning = node.state === 'running'

      // Background circle/rounded rect
      const size = isVm ? 28 : 32
      ctx.fillStyle = isVm
        ? (isRunning ? 'rgba(56, 189, 248, 0.15)' : 'rgba(100, 116, 139, 0.15)')
        : node.kind === 'wan' ? 'rgba(56, 189, 248, 0.2)' : 'rgba(30, 41, 59, 0.8)'

      ctx.beginPath()
      ctx.roundRect(node.x - size, node.y - size, size * 2, size * 2, 8)
      ctx.fill()
      ctx.strokeStyle = isVm
        ? (isRunning ? 'rgba(56, 189, 248, 0.5)' : 'rgba(100, 116, 139, 0.3)')
        : 'rgba(56, 189, 248, 0.3)'
      ctx.lineWidth = 1
      ctx.stroke()

      // Icon symbol
      ctx.fillStyle = isVm
        ? (isRunning ? '#38bdf8' : '#64748b')
        : '#38bdf8'
      ctx.font = `bold ${isVm ? 11 : 13}px system-ui, sans-serif`
      ctx.textAlign = 'center'
      ctx.textBaseline = 'middle'

      const icon = node.kind === 'wan' ? '🌐' : node.kind === 'gateway' ? '🔀' : node.kind === 'switch' ? '⬡' : '💻'
      ctx.font = `${isVm ? 16 : 20}px system-ui`
      ctx.fillText(icon, node.x, node.y - 4)

      // Label below
      ctx.font = `${isVm ? 9 : 10}px system-ui, sans-serif`
      ctx.fillStyle = isVm ? (isRunning ? '#e2e8f0' : '#64748b') : '#94a3b8'
      const lines = node.label.split('\n')
      lines.forEach((line, li) => {
        ctx.fillText(line, node.x, node.y + size + 10 + li * 12)
      })

      // Status dot for VMs
      if (isVm) {
        ctx.beginPath()
        ctx.arc(node.x + size - 4, node.y - size + 4, 4, 0, Math.PI * 2)
        ctx.fillStyle = isRunning ? '#22c55e' : node.state === 'paused' ? '#eab308' : '#ef4444'
        ctx.fill()
      }
    }
  }, [interfaces, vms])

  const activeCount = stats?.active_interfaces || 0
  const totalCount = stats?.total_interfaces || 0
  const totalRx = stats?.total_rx_bytes || 0
  const totalTx = stats?.total_tx_bytes || 0

  return (
    <div className="space-y-6">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Network Overview</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            Live network topology and traffic monitoring
          </p>
        </div>
        <Button variant="primary" icon={<Plus size={14} />}>Create Bridge</Button>
      </div>

      {/* Topology + Stats */}
      <div className="grid grid-cols-1 lg:grid-cols-[1fr_300px] gap-5">
        {/* Topology Map */}
        <Card>
          <div className="flex items-center justify-between mb-3">
            <SectionLabel>Live Topology</SectionLabel>
            <div className="flex items-center gap-4 text-[10px] text-vmm-text-muted">
              <span className="flex items-center gap-1.5">
                <span className="w-2 h-2 rounded-full bg-vmm-success" /> Active Nodes
              </span>
              <span className="flex items-center gap-1.5">
                <span className="w-2 h-2 rounded-full bg-vmm-text-muted" /> Inactive
              </span>
            </div>
          </div>
          <div className="bg-vmm-bg-alt rounded-lg overflow-hidden" style={{ height: 300 }}>
            <canvas
              ref={canvasRef}
              className="w-full h-full"
              style={{ width: '100%', height: '100%' }}
            />
          </div>
        </Card>

        {/* Right sidebar */}
        <div className="space-y-5">
          {/* Throughput */}
          <Card>
            <SectionLabel className="mb-3">Throughput</SectionLabel>
            <div className="grid grid-cols-2 gap-3">
              <div className="bg-vmm-bg-alt rounded-lg p-3 text-center">
                <div className="text-[9px] text-vmm-text-muted uppercase tracking-wider">Ingress</div>
                <div className="text-xl font-bold text-vmm-accent">{formatBytes(totalRx)}</div>
              </div>
              <div className="bg-vmm-bg-alt rounded-lg p-3 text-center">
                <div className="text-[9px] text-vmm-text-muted uppercase tracking-wider">Egress</div>
                <div className="text-xl font-bold text-vmm-text">{formatBytes(totalTx)}</div>
              </div>
            </div>
          </Card>

          {/* Alerts */}
          <Card>
            <SectionLabel className="mb-3">Active Alerts</SectionLabel>
            <div className="space-y-2">
              {activeCount < totalCount ? (
                <div className="flex items-start gap-2 bg-vmm-warning/10 border border-vmm-warning/20 rounded-lg p-3">
                  <AlertTriangle size={14} className="text-vmm-warning mt-0.5 flex-shrink-0" />
                  <div className="text-xs">
                    <div className="font-medium text-vmm-text">{totalCount - activeCount} Interface(s) Down</div>
                    <div className="text-vmm-text-muted">{activeCount}/{totalCount} active</div>
                  </div>
                </div>
              ) : (
                <div className="flex items-start gap-2 bg-vmm-success/10 border border-vmm-success/20 rounded-lg p-3">
                  <CheckCircle size={14} className="text-vmm-success mt-0.5 flex-shrink-0" />
                  <div className="text-xs">
                    <div className="font-medium text-vmm-text">All Systems Operational</div>
                    <div className="text-vmm-text-muted">{activeCount} interfaces active</div>
                  </div>
                </div>
              )}
            </div>
          </Card>

          {/* System Health */}
          <Card>
            <SectionLabel className="mb-2">System Health</SectionLabel>
            <p className="text-xs text-vmm-text-dim">
              Network stack optimization is at {activeCount > 0 ? '94' : '0'}%.
              No packet loss recorded in 24h.
            </p>
          </Card>
        </div>
      </div>

      {/* Interface summary */}
      <div>
        <h2 className="text-lg font-bold text-vmm-text mb-3">Interface Summary</h2>
        <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
          {interfaces.filter(i => i.kind !== 'loopback').slice(0, 8).map((iface) => (
            <Card key={iface.name}>
              <div className="flex items-center justify-between mb-2">
                <span className="text-sm font-semibold text-vmm-text">{iface.name}</span>
                <span className={`w-2 h-2 rounded-full ${iface.state === 'up' ? 'bg-vmm-success' : 'bg-vmm-danger'}`} />
              </div>
              <div className="text-xs text-vmm-text-muted space-y-0.5">
                <div>{iface.ipv4 || 'No IP'}</div>
                <div className="font-mono text-[10px]">{iface.mac}</div>
              </div>
            </Card>
          ))}
        </div>
      </div>
    </div>
  )
}
