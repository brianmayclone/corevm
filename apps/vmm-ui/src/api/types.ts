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
  resource_group_id: number
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

export interface DiskInfoApi {
  path: string
  size_bytes: number
  used_bytes: number
}

export interface VmDetail {
  id: string
  name: string
  state: VmState
  config: VmConfig
  owner_id: number
  resource_group_id: number
  created_at: string
  disks: DiskInfoApi[]
}

export interface StoragePool {
  id: number | string
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
  vm_name: string | null
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

// ── Resource Groups ──────────────────────────────────────────────────────

export interface GroupPermission {
  id: number
  group_id: number
  group_name: string
  permissions: string[]
}

export interface ResourceGroup {
  id: number
  name: string
  description: string
  is_default: boolean
  vm_count: number
  created_at: string
  permissions: GroupPermission[]
}

export interface PermissionsList {
  permissions: string[]
  categories: Record<string, string[]>
}

// ── Settings ─────────────────────────────────────────────────────────────

export interface ServerSettings {
  bind: string
  port: number
  session_timeout_hours: number
  max_disk_size_gb: number
  log_level: string
  version: string
  uptime_secs: number
}

export interface TimeSettings {
  current_time: string
  timezone: string
  ntp_enabled: boolean
  ntp_servers: string[]
}

export interface SecuritySettings {
  max_login_attempts: number
  lockout_duration_secs: number
  password_min_length: number
  require_uppercase: boolean
  require_numbers: boolean
  api_keys_enabled: boolean
}

export interface Group {
  id: number
  name: string
  role: string
  description: string
  member_count: number
}

// ── Network ──────────────────────────────────────────────────────────────

export interface NetworkInterface {
  name: string
  kind: string
  mac: string
  ipv4: string | null
  ipv6: string | null
  mtu: number
  state: string
  speed_mbps: number | null
  rx_bytes: number
  tx_bytes: number
}

export interface NetworkStats {
  total_interfaces: number
  active_interfaces: number
  total_rx_bytes: number
  total_tx_bytes: number
}

// ── Cluster Mode ────────────────────────────────────────────────────────

export type BackendMode = 'standalone' | 'managed' | 'cluster'

/** Extended SystemInfo returned when connected to vmm-cluster or managed node. */
export interface SystemInfoExtended extends SystemInfo {
  mode: BackendMode
  hostname?: string
  cluster_url?: string
  cluster_name?: string
  total_hosts?: number
  online_hosts?: number
}

export interface Host {
  id: string
  hostname: string
  address: string
  cluster_id: string
  cpu_model: string
  cpu_cores: number
  cpu_threads: number
  total_ram_mb: number
  free_ram_mb: number
  cpu_usage_pct: number
  hw_virtualization: boolean
  status: 'online' | 'offline' | 'maintenance' | 'connecting' | 'error'
  maintenance_mode: boolean
  connection_state: string
  last_heartbeat: string | null
  version: string
  vm_count: number
  registered_at: string
  // CoreSAN auto-discovery (populated via heartbeat)
  san_enabled: boolean
  san_node_id: string
  san_address: string
  san_volumes: number
  san_peers: number
}

export interface Cluster {
  id: string
  name: string
  description: string
  drs_enabled: boolean
  ha_enabled: boolean
  ha_vm_restart_priority: string
  ha_admission_control: boolean
  ha_failover_hosts: number
  host_count: number
  vm_count: number
  total_ram_mb: number
  free_ram_mb: number
  created_at: string
}

export interface Datastore {
  id: string
  name: string
  store_type: string
  mount_source: string
  mount_opts: string
  mount_path: string
  cluster_id: string
  total_bytes: number
  free_bytes: number
  status: string
  host_mounts: DatastoreHostMount[]
  created_at: string
}

export interface DatastoreHostMount {
  host_id: string
  hostname: string
  mounted: boolean
  mount_status: string
  total_bytes: number
  free_bytes: number
}

export interface ClusterVmSummary extends VmSummary {
  host_id?: string
  host_name?: string
  cluster_id?: string
  ha_protected?: boolean
  ha_restart_priority?: string
  drs_automation?: string
}

export interface ClusterStats {
  total_hosts: number
  online_hosts: number
  maintenance_hosts: number
  offline_hosts: number
  total_vms: number
  running_vms: number
  stopped_vms: number
  total_ram_mb: number
  used_ram_mb: number
  total_disk_bytes: number
  used_disk_bytes: number
  ha_protected_vms: number
}

export interface Task {
  id: string
  task_type: string
  status: string
  progress_pct: number
  target_type: string
  target_id: string
  initiated_by: number | null
  error: string | null
  created_at: string
  started_at: string | null
  completed_at: string | null
}

export interface ClusterEvent {
  id: number
  severity: 'info' | 'warning' | 'error' | 'critical'
  category: string
  message: string
  target_type: string | null
  target_id: string | null
  host_id: string | null
  created_at: string
}

export interface DrsRecommendation {
  id: number
  cluster_id: string
  vm_id: string
  vm_name: string
  source_host_id: string
  source_host_name: string
  target_host_id: string
  target_host_name: string
  reason: string
  priority: string
  status: string
  created_at: string
}

export interface Alarm {
  id: number
  name: string
  target_type: string
  target_id: string
  condition_type: string
  threshold: number | null
  severity: string
  triggered: boolean
  acknowledged: boolean
  created_at: string
  triggered_at: string | null
}

// ── CoreSAN (Software-Defined Storage) ──────────────────────────────────

export type ResilienceMode = 'none' | 'mirror' | 'erasure'
export type SyncMode = 'sync' | 'async'

export interface CoreSanVolume {
  id: string
  name: string
  resilience_mode: ResilienceMode
  replica_count: number
  stripe_width: number
  sync_mode: SyncMode
  status: 'creating' | 'online' | 'degraded' | 'offline'
  total_bytes: number
  free_bytes: number
  backend_count: number
  created_at: string
}

export interface CoreSanBackend {
  id: string
  volume_id: string
  node_id: string
  path: string
  total_bytes: number
  free_bytes: number
  status: 'online' | 'degraded' | 'offline' | 'draining'
  last_check: string | null
}

export interface CoreSanPeer {
  node_id: string
  address: string
  peer_port: number
  hostname: string
  status: 'connecting' | 'online' | 'offline'
  last_heartbeat: string | null
}

export interface CoreSanStatus {
  running: boolean
  node_id: string
  hostname: string
  uptime_secs: number
  volumes: CoreSanVolumeStatus[]
  peer_count: number
  benchmark_summary: CoreSanBenchmarkSummary | null
}

export interface CoreSanVolumeStatus {
  volume_id: string
  volume_name: string
  resilience_mode: string
  replica_count: number
  total_bytes: number
  free_bytes: number
  status: string
  backend_count: number
  files_synced: number
  files_syncing: number
}

export interface CoreSanBenchmarkSummary {
  avg_bandwidth_mbps: number
  avg_latency_us: number
  worst_peer: string | null
  measured_at: string
}

export interface CoreSanBenchmarkResult {
  from_node_id: string
  to_node_id: string
  bandwidth_mbps: number
  latency_us: number
  jitter_us: number
  packet_loss_pct: number
  test_size_bytes: number
  measured_at: string
}

export interface CoreSanBenchmarkMatrix {
  node_ids: string[]
  entries: CoreSanBenchmarkResult[]
}

export interface CoreSanFile {
  rel_path: string
  size_bytes: number
  sha256: string
  created_at: string
  updated_at: string
  replica_count: number
  synced_count: number
}
