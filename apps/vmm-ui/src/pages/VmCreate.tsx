import { useEffect, useState } from 'react'
import { useNavigate, useParams } from 'react-router-dom'
import { Info, Monitor, Zap, SlidersHorizontal, HardDrive, Cpu, MemoryStick, Clock, Search, Plus, Server, Workflow, Boxes } from 'lucide-react'
import api from '../api/client'
import type { VmConfig, VmDetail, Cluster, Host } from '../api/types'
import { useClusterStore } from '../stores/clusterStore'
import TabBar from '../components/TabBar'
import SectionCard from '../components/SectionCard'
import FormField from '../components/FormField'
import TextInput from '../components/TextInput'
import TextArea from '../components/TextArea'
import Select from '../components/Select'
import Toggle from '../components/Toggle'
import Button from '../components/Button'
import Card from '../components/Card'
import PoolBrowser from '../components/PoolBrowser'
import CreateDiskDialog from '../components/CreateDiskDialog'
import CoreSanFilePicker from '../components/CoreSanFilePicker'

const tabs = [
  { id: 'general', label: 'General' },
  { id: 'hardware', label: 'Hardware' },
  { id: 'network', label: 'Network' },
  { id: 'storage', label: 'Storage' },
  { id: 'snapshots', label: 'Snapshots' },
]

const guestOsOptions = [
  { value: 'other', label: 'Other' },
  { value: 'win11', label: 'Windows 11' }, { value: 'win10', label: 'Windows 10' },
  { value: 'win8', label: 'Windows 8' }, { value: 'win7', label: 'Windows 7' },
  { value: 'winserver2022', label: 'Windows Server 2022' },
  { value: 'winserver2019', label: 'Windows Server 2019' },
  { value: 'ubuntu', label: 'Ubuntu' }, { value: 'debian', label: 'Debian' },
  { value: 'fedora', label: 'Fedora' }, { value: 'arch', label: 'Arch Linux' },
  { value: 'linux', label: 'Linux (Other)' }, { value: 'freebsd', label: 'FreeBSD' },
  { value: 'dos', label: 'DOS / FreeDOS' },
]

const biosOptions = [
  { value: 'seabios', label: 'SeaBIOS (Legacy BIOS)' },
  { value: 'uefi', label: 'UEFI' },
  { value: 'corevm', label: 'CoreVM BIOS' },
]

const bootOptions = [
  { value: 'diskfirst', label: 'Disk (SATA 0)' },
  { value: 'cdfirst', label: 'CD-ROM' },
  { value: 'floppyfirst', label: 'Floppy' },
]

const gpuOptions = [
  { value: 'stdvga', label: 'Standard VGA (Bochs VBE)' },
  { value: 'virtiogpu', label: 'VirtIO GPU' },
  { value: 'intelhd', label: 'Intel HD Graphics (native driver)' },
]

const nicOptions = [
  { value: 'e1000', label: 'Intel E1000' },
  { value: 'virtionet', label: 'VirtIO Net' },
]

const netModeOptions = [
  { value: 'usermode', label: 'User Mode (NAT/SLIRP)' },
  { value: 'bridge', label: 'Bridged' },
  { value: 'disconnected', label: 'Disconnected' },
]

const cacheOptions = [
  { value: 'none', label: 'None' },
  { value: 'writeback', label: 'Write Back' },
  { value: 'writethrough', label: 'Write Through' },
]

export default function VmCreate() {
  const { id } = useParams<{ id: string }>()
  const isEdit = !!id
  const navigate = useNavigate()
  const { backendMode } = useClusterStore()
  const isCluster = backendMode === 'cluster'
  const [activeTab, setActiveTab] = useState('general')
  const [saving, setSaving] = useState(false)
  const [loading, setLoading] = useState(isEdit)
  const [error, setError] = useState('')
  const [isoBrowserOpen, setIsoBrowserOpen] = useState(false)
  const [diskBrowserOpen, setDiskBrowserOpen] = useState(false)
  const [createDiskOpen, setCreateDiskOpen] = useState(false)
  const [sanIsoBrowserOpen, setSanIsoBrowserOpen] = useState(false)
  const [sanDiskBrowserOpen, setSanDiskBrowserOpen] = useState(false)

  // Cluster-mode: cluster and host selection
  const [clusters, setClusters] = useState<Cluster[]>([])
  const [hosts, setHosts] = useState<Host[]>([])
  const [selectedClusterId, setSelectedClusterId] = useState('')
  const [selectedHostId, setSelectedHostId] = useState('')  // '' = auto-placement

  // SDN network selection (cluster mode)
  interface SdnNetwork { id: number; cluster_id: string; name: string; subnet: string; gateway: string }
  const [sdnNetworks, setSdnNetworks] = useState<SdnNetwork[]>([])
  const [selectedNetworkId, setSelectedNetworkId] = useState<number | null>(null)

  const [form, setForm] = useState<VmConfig>({
    uuid: '', name: '', guest_os: 'other', guest_arch: 'x64',
    ram_mb: 2048, cpu_cores: 2, disk_images: [], iso_image: '',
    boot_order: 'cdfirst', bios_type: 'seabios', gpu_model: 'stdvga',
    vram_mb: 16, nic_model: 'e1000', net_enabled: true, net_mode: 'usermode',
    net_host_nic: '', mac_mode: 'dynamic', mac_address: '',
    audio_enabled: true, usb_tablet: true, ram_alloc: 'ondemand',
    diagnostics: false, disk_cache_mb: 0, disk_cache_mode: 'none',
  })

  // Load existing VM config for edit mode
  useEffect(() => {
    if (isEdit) {
      api.get<VmDetail>(`/api/vms/${id}`).then(({ data }) => {
        setForm(data.config)
        setLoading(false)
      }).catch(() => {
        setError('Failed to load VM config')
        setLoading(false)
      })
    }
  }, [id])

  // Load clusters and hosts in cluster mode
  useEffect(() => {
    if (isCluster) {
      api.get<Cluster[]>('/api/clusters').then(({ data }) => {
        setClusters(data)
        if (data.length > 0 && !selectedClusterId) setSelectedClusterId(data[0].id)
      })
      api.get<Host[]>('/api/hosts').then(({ data }) => setHosts(data))
      api.get<SdnNetwork[]>('/api/networks').then(({ data }) => setSdnNetworks(data))
    }
  }, [isCluster])

  // Filter hosts by selected cluster
  const clusterHosts = hosts.filter(h => h.cluster_id === selectedClusterId && h.status === 'online' && !h.maintenance_mode)

  const set = <K extends keyof VmConfig>(key: K, val: VmConfig[K]) =>
    setForm((f) => ({ ...f, [key]: val }))

  const handleSave = async () => {
    if (!form.name.trim()) { setError('Machine name is required'); return }
    if (isCluster && !selectedClusterId) { setError('Please select a cluster'); return }
    setSaving(true)
    setError('')
    try {
      if (isEdit) {
        await api.put(`/api/vms/${id}`, form)
        navigate(`/vms/${id}`)
      } else if (isCluster) {
        // Cluster mode: send cluster_id, host_id, and config separately
        const { data } = await api.post('/api/vms', {
          name: form.name,
          description: '',
          cluster_id: selectedClusterId,
          host_id: selectedHostId || undefined,
          config: { ...form, uuid: '' },
          network_id: selectedNetworkId || undefined,
        })
        navigate(`/vms/${data.id}`)
      } else {
        const { data } = await api.post('/api/vms', { ...form, uuid: '' })
        navigate(`/vms/${data.id}`)
      }
    } catch (e: any) {
      setError(e.response?.data?.error || 'Failed to save VM')
    } finally {
      setSaving(false)
    }
  }

  if (loading) return <div className="text-vmm-text-muted py-12 text-center">Loading configuration...</div>

  return (
    <div className="space-y-5">
      {/* Page header */}
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">{isEdit ? 'VM Settings' : 'Create VM'}</h1>
        <div className="flex items-center gap-2 mt-1">
          <span className={`w-2 h-2 rounded-full ${isEdit ? 'bg-vmm-accent' : 'bg-vmm-warning'}`} />
          <span className="text-sm text-vmm-text-muted">{form.name || 'New VM'}</span>
        </div>
      </div>

      <TabBar tabs={tabs} active={activeTab} onChange={setActiveTab} />

      {/* ── General ───────────────────────────────────────────────── */}
      {activeTab === 'general' && (
        <div className="space-y-5">
          {/* Cluster/Host placement — only in cluster mode */}
          {isCluster && !isEdit && (
            <SectionCard icon={<Workflow size={18} />} title="Placement">
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-6">
                <FormField label="Cluster">
                  <Select
                    options={clusters.map(c => ({ value: c.id, label: `${c.name} (${c.host_count} hosts, ${c.vm_count} VMs)` }))}
                    value={selectedClusterId}
                    onChange={(e) => { setSelectedClusterId(e.target.value); setSelectedHostId('') }}
                  />
                </FormField>
                <FormField label="Target Host">
                  <Select
                    options={[
                      { value: '', label: 'Auto (Best Fit)' },
                      ...clusterHosts.map(h => ({
                        value: h.id,
                        label: `${h.hostname} (${h.free_ram_mb} MB free)`
                      }))
                    ]}
                    value={selectedHostId}
                    onChange={(e) => setSelectedHostId(e.target.value)}
                  />
                  <p className="text-xs text-vmm-text-muted mt-1">
                    {selectedHostId ? 'VM will be placed on the selected host' : 'Scheduler will choose the host with the most free resources'}
                  </p>
                </FormField>
              </div>
              {selectedClusterId && clusterHosts.length === 0 && (
                <div className="mt-3 bg-yellow-500/10 border border-yellow-500/30 text-yellow-400 rounded-lg p-3 text-xs">
                  No online hosts in this cluster. Add hosts before creating VMs.
                </div>
              )}
            </SectionCard>
          )}

          <div className="grid grid-cols-1 lg:grid-cols-[1fr_340px] gap-5">
            <SectionCard icon={<Info size={18} />} title="Basic Information">
              <div className="space-y-5">
                <FormField label="Machine Name">
                  <TextInput value={form.name} onChange={(e) => set('name', e.target.value)} placeholder="ubuntu-prod-01" />
                </FormField>
                <FormField label="Guest OS">
                  <Select options={guestOsOptions} value={form.guest_os} onChange={(e) => set('guest_os', e.target.value)} />
                </FormField>
                <FormField label="Architecture">
                  <Select options={[{ value: 'x64', label: '64-bit (x86_64)' }, { value: 'x86', label: '32-bit (x86)' }]}
                    value={form.guest_arch} onChange={(e) => set('guest_arch', e.target.value)} />
                </FormField>
              </div>
            </SectionCard>
            <SectionCard icon={<Monitor size={18} />} title="Firmware">
              <FormField label="BIOS Type">
                <Select options={biosOptions} value={form.bios_type} onChange={(e) => set('bios_type', e.target.value)} />
              </FormField>
              <p className="text-xs text-vmm-text-muted mt-3 leading-relaxed">
                {form.bios_type === 'uefi'
                  ? 'Modern 64-bit firmware. Supports large disks and secure boot. Recommended for Windows 11.'
                  : form.bios_type === 'seabios'
                  ? 'Traditional BIOS. Compatible with all guest operating systems.'
                  : 'CoreVM minimal BIOS. Fast boot, limited compatibility.'}
              </p>
            </SectionCard>
          </div>
          <SectionCard icon={<SlidersHorizontal size={18} />} title="Boot Options">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-8">
              <FormField label="Boot Order">
                <div className="space-y-2">
                  {bootOptions.map((opt) => (
                    <div key={opt.value} onClick={() => set('boot_order', opt.value)}
                      className={`flex items-center gap-3 px-4 py-3 rounded-lg border cursor-pointer transition-colors
                        ${form.boot_order === opt.value ? 'border-vmm-accent/50 bg-vmm-accent/5' : 'border-vmm-border bg-vmm-bg-alt hover:border-vmm-border-light'}`}>
                      <HardDrive size={14} className="text-vmm-text-muted" />
                      <span className="text-sm text-vmm-text">{opt.label}</span>
                      {form.boot_order === opt.value && (
                        <span className="ml-auto px-2 py-0.5 bg-vmm-accent/20 text-vmm-accent text-[10px] font-bold rounded">1ST</span>
                      )}
                    </div>
                  ))}
                </div>
              </FormField>
              <FormField label="ISO Image">
                <div className="flex gap-2">
                  <TextInput value={form.iso_image} onChange={(e) => set('iso_image', e.target.value)}
                    placeholder="/path/to/image.iso" className="flex-1" />
                  <Button variant="outline" size="md" icon={<Search size={14} />}
                    onClick={() => setIsoBrowserOpen(true)}>Local</Button>
                  <Button variant="outline" size="md" icon={<Boxes size={14} />}
                    onClick={() => setSanIsoBrowserOpen(true)}>SAN</Button>
                </div>
              </FormField>
            </div>
          </SectionCard>
        </div>
      )}

      {/* ── Hardware ──────────────────────────────────────────────── */}
      {activeTab === 'hardware' && (
        <div className="space-y-5">
          <SectionCard icon={<Cpu size={18} />} title="Compute Resources">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-6">
              <FormField label="CPU Cores">
                <TextInput type="number" min={1} max={32} value={form.cpu_cores} onChange={(e) => set('cpu_cores', parseInt(e.target.value) || 1)} />
              </FormField>
              <FormField label="Memory (MB)">
                <TextInput type="number" min={64} step={256} value={form.ram_mb} onChange={(e) => set('ram_mb', parseInt(e.target.value) || 256)} />
              </FormField>
            </div>
          </SectionCard>
          <SectionCard icon={<Monitor size={18} />} title="Display">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-6">
              <FormField label="GPU Model">
                <Select options={gpuOptions} value={form.gpu_model} onChange={(e) => set('gpu_model', e.target.value)} />
              </FormField>
              <FormField label="Video RAM (MB)">
                <TextInput type="number" min={8} max={256} value={form.vram_mb} onChange={(e) => set('vram_mb', parseInt(e.target.value) || 16)} />
              </FormField>
            </div>
          </SectionCard>
          <SectionCard icon={<MemoryStick size={18} />} title="Peripherals">
            <Toggle label="Audio (AC97)" description="Enable virtual sound card." enabled={form.audio_enabled} onChange={(v) => set('audio_enabled', v)} />
            <Toggle label="USB Tablet" description="Absolute mouse positioning (recommended for web console)." enabled={form.usb_tablet} onChange={(v) => set('usb_tablet', v)} />
            <Toggle label="Diagnostics" description="Enable detailed logging for troubleshooting." enabled={form.diagnostics} onChange={(v) => set('diagnostics', v)} />
          </SectionCard>
        </div>
      )}

      {/* ── Network ──────────────────────────────────────────────── */}
      {activeTab === 'network' && (
        <div className="space-y-5">
          <SectionCard icon={<Monitor size={18} />} title="Network Adapter">
            <Toggle label="Enable Networking" description="Connect a virtual NIC to the VM." enabled={form.net_enabled} onChange={(v) => set('net_enabled', v)} />
            {form.net_enabled && (
              <div className="mt-4 space-y-4">
                {/* SDN network selector — cluster mode only */}
                {isCluster && (
                  <FormField label="Virtual Network (SDN)">
                    <Select
                      options={[
                        { value: '', label: 'Default (NAT/SLIRP — 10.0.2.0/24)' },
                        ...sdnNetworks
                          .filter(n => !selectedClusterId || n.cluster_id === selectedClusterId)
                          .map(n => ({ value: String(n.id), label: `${n.name} (${n.subnet})` }))
                      ]}
                      value={selectedNetworkId ? String(selectedNetworkId) : ''}
                      onChange={(e) => {
                        const id = e.target.value ? parseInt(e.target.value) : null
                        setSelectedNetworkId(id)
                        // SDN networks use bridge mode — cluster auto-configures the bridge
                        if (id) {
                          set('net_mode', 'bridge')
                          set('net_enabled', true)
                        }
                      }}
                    />
                    {selectedNetworkId && (
                      <p className="text-xs text-vmm-text-muted mt-1">
                        The VM will be bridged into this network and receive its IP via DHCP. Cross-host communication is handled via VXLAN overlay.
                      </p>
                    )}
                  </FormField>
                )}

                <div className="grid grid-cols-1 sm:grid-cols-2 gap-6">
                  <FormField label="NIC Model">
                    <Select options={nicOptions} value={form.nic_model} onChange={(e) => set('nic_model', e.target.value)} />
                  </FormField>
                  {/* Hide network mode in cluster mode when SDN is selected */}
                  {(!isCluster || !selectedNetworkId) && (
                    <FormField label="Network Mode">
                      <Select options={netModeOptions} value={form.net_mode} onChange={(e) => set('net_mode', e.target.value)} />
                    </FormField>
                  )}
                </div>
                {form.net_mode === 'bridge' && !selectedNetworkId && (
                  <FormField label="Host Bridge Interface">
                    <TextInput value={form.net_host_nic} onChange={(e) => set('net_host_nic', e.target.value)} placeholder="br0" />
                  </FormField>
                )}
                <FormField label="MAC Address">
                  <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                    <Select options={[{ value: 'dynamic', label: 'Dynamic (auto)' }, { value: 'static', label: 'Static' }]}
                      value={form.mac_mode} onChange={(e) => set('mac_mode', e.target.value)} />
                    {form.mac_mode === 'static' && (
                      <TextInput value={form.mac_address} onChange={(e) => set('mac_address', e.target.value)} placeholder="52:54:00:XX:XX:XX" />
                    )}
                  </div>
                </FormField>
              </div>
            )}
          </SectionCard>
        </div>
      )}

      {/* ── Storage ──────────────────────────────────────────────── */}
      {activeTab === 'storage' && (
        <SectionCard icon={<HardDrive size={18} />} title="Disk Images">
          <div className="space-y-3">
            {form.disk_images.length === 0 ? (
              <div className="text-sm text-vmm-text-muted py-4 text-center border border-dashed border-vmm-border rounded-lg">
                No disks attached. Create a new disk or browse an existing one.
              </div>
            ) : form.disk_images.map((disk, i) => (
              <div key={i} className="flex items-center gap-2 bg-vmm-bg-alt border border-vmm-border rounded-lg px-4 py-3">
                <HardDrive size={14} className="text-vmm-text-muted flex-shrink-0" />
                <span className="text-sm text-vmm-text font-mono flex-1 truncate">{disk}</span>
                <Button variant="danger" size="sm" onClick={() => {
                  set('disk_images', form.disk_images.filter((_, j) => j !== i))
                }}>Remove</Button>
              </div>
            ))}
            <div className="flex gap-2">
              <Button variant="primary" size="sm" icon={<Plus size={14} />}
                onClick={() => setCreateDiskOpen(true)}>
                Create New Disk
              </Button>
              <Button variant="outline" size="sm" icon={<Search size={14} />}
                onClick={() => setDiskBrowserOpen(true)}>
                Browse Local
              </Button>
              <Button variant="outline" size="sm" icon={<Boxes size={14} />}
                onClick={() => setSanDiskBrowserOpen(true)}>
                Browse SAN
              </Button>
            </div>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-6 mt-5 pt-5 border-t border-vmm-border">
            <FormField label="Disk Cache (MB)">
              <TextInput type="number" min={0} value={form.disk_cache_mb} onChange={(e) => set('disk_cache_mb', parseInt(e.target.value) || 0)} />
            </FormField>
            <FormField label="Cache Mode">
              <Select options={cacheOptions} value={form.disk_cache_mode} onChange={(e) => set('disk_cache_mode', e.target.value)} />
            </FormField>
          </div>
        </SectionCard>
      )}

      {/* ── Snapshots ─────────────────────────────────────────────── */}
      {activeTab === 'snapshots' && (
        <Card>
          <div className="text-vmm-text-muted text-sm py-8 text-center">
            Snapshot management — available after VM is created and has disk images
          </div>
        </Card>
      )}

      {/* ── Footer ────────────────────────────────────────────────── */}
      <div className="flex items-center justify-between bg-vmm-surface border border-vmm-border rounded-xl px-6 py-4">
        <div className="flex items-center gap-2 text-xs text-vmm-text-muted">
          <Clock size={13} />
          <span className="italic">{isEdit ? `Editing ${form.name}` : 'New configuration — not yet saved'}</span>
        </div>
        <div className="flex items-center gap-3">
          {error && <span className="text-xs text-vmm-danger">{error}</span>}
          <Button variant="ghost" onClick={() => navigate(-1)}>Discard Changes</Button>
          <Button variant="primary" onClick={handleSave} disabled={saving}>
            {saving ? 'Saving...' : 'Save Configuration'}
          </Button>
        </div>
      </div>

      {/* ── Dialogs ───────────────────────────────────────────────── */}
      <PoolBrowser open={isoBrowserOpen} onClose={() => setIsoBrowserOpen(false)}
        filterExt=".iso" title="Select ISO Image"
        clusterId={isCluster ? selectedClusterId : undefined}
        onSelect={(path) => set('iso_image', path)} />

      <PoolBrowser open={diskBrowserOpen} onClose={() => setDiskBrowserOpen(false)}
        filterExt=".raw" title="Select Disk Image"
        clusterId={isCluster ? selectedClusterId : undefined}
        onSelect={(path) => set('disk_images', [...form.disk_images, path])} />

      <CreateDiskDialog open={createDiskOpen} onClose={() => setCreateDiskOpen(false)}
        vmName={form.name || 'new-vm'} vmId={form.uuid}
        clusterId={isCluster ? selectedClusterId : undefined}
        onCreated={(path) => set('disk_images', [...form.disk_images, path])} />

      {/* CoreSAN File Pickers */}
      <CoreSanFilePicker open={sanIsoBrowserOpen} onClose={() => setSanIsoBrowserOpen(false)}
        title="Select ISO from CoreSAN" filterExt=".iso"
        onSelect={(path) => set('iso_image', path)} />

      <CoreSanFilePicker open={sanDiskBrowserOpen} onClose={() => setSanDiskBrowserOpen(false)}
        title="Select Disk from CoreSAN" filterExt=".raw"
        onSelect={(path) => set('disk_images', [...form.disk_images, path])} />
    </div>
  )
}
