// ── API response types ───────────────────────────────────────────────────

export interface User {
  id: number
  username: string
  role: 'admin' | 'operator' | 'viewer'
  created_at: string
}

export interface LoginResponse {
  access_token: string
  user: User
}

export type VmState = 'stopped' | 'running' | 'paused' | 'stopping'

export interface VmSummary {
  id: string
  name: string
  state: VmState
  guest_os: string
  ram_mb: number
  cpu_cores: number
  owner_id: number
}

export interface VmConfig {
  uuid: string
  name: string
  guest_os: string
  guest_arch: string
  ram_mb: number
  cpu_cores: number
  disk_images: string[]
  iso_image: string
  boot_order: string
  bios_type: string
  gpu_model: string
  vram_mb: number
  nic_model: string
  net_enabled: boolean
  net_mode: string
  net_host_nic: string
  mac_mode: string
  mac_address: string
  audio_enabled: boolean
  usb_tablet: boolean
  ram_alloc: string
  diagnostics: boolean
  disk_cache_mb: number
  disk_cache_mode: string
}

export interface VmDetail {
  id: string
  name: string
  state: VmState
  config: VmConfig
  owner_id: number
  created_at: string
}

export interface StoragePool {
  id: number
  name: string
  path: string
  pool_type: string
  shared: boolean
  mount_source: string | null
  mount_opts: string | null
  total_bytes: number
  free_bytes: number
}

export interface DiskImage {
  id: number
  name: string
  path: string
  size_bytes: number
  format: string
  pool_id: number | null
  vm_id: string | null
  created_at: string
}

export interface Iso {
  id: number
  name: string
  path: string
  size_bytes: number
  uploaded_at: string
}

export interface SystemInfo {
  version: string
  platform: string
  arch: string
  hw_virtualization: boolean
  cpu_count: number
  total_ram_mb: number
  free_ram_mb: number
  total_disk_bytes: number
  free_disk_bytes: number
}

export interface DashboardStats {
  total_vms: number
  running_vms: number
  stopped_vms: number
  cpu_count: number
  total_ram_mb: number
  used_ram_mb: number
  total_disk_bytes: number
  used_disk_bytes: number
}

export interface StorageStats {
  total_pools: number
  online_pools: number
  total_bytes: number
  used_bytes: number
  free_bytes: number
  vm_disk_bytes: number
  total_images: number
  total_isos: number
  orphaned_images: number
}

export interface AuditEntry {
  id: number
  user_id: number | null
  action: string
  target_type: string | null
  target_id: string | null
  details: string | null
  created_at: string
}

export interface PoolFile {
  name: string
  path: string
  size_bytes: number
  is_dir: boolean
}
