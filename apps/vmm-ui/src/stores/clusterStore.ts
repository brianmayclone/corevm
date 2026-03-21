import { create } from 'zustand'
import api from '../api/client'
import type {
  BackendMode, SystemInfoExtended, Host, Cluster, Datastore,
  ClusterStats, Task, ClusterEvent, DrsRecommendation, Alarm,
} from '../api/types'

interface ClusterState {
  /** Backend mode detected from /api/system/info */
  backendMode: BackendMode
  /** URL of managing cluster (when mode === 'managed') */
  clusterUrl: string | null
  /** Cluster name (when mode === 'cluster') */
  clusterName: string | null

  hosts: Host[]
  clusters: Cluster[]
  datastores: Datastore[]
  clusterStats: ClusterStats | null
  tasks: Task[]
  events: ClusterEvent[]
  drsRecommendations: DrsRecommendation[]
  alarms: Alarm[]

  /** Detect backend mode on app startup */
  detectBackend: () => Promise<void>

  /** Cluster-mode data fetchers */
  fetchHosts: () => Promise<void>
  fetchClusters: () => Promise<void>
  fetchDatastores: () => Promise<void>
  fetchClusterStats: () => Promise<void>
  fetchTasks: () => Promise<void>
  fetchEvents: () => Promise<void>
  fetchDrsRecommendations: () => Promise<void>
  fetchAlarms: () => Promise<void>

  /** Cluster-mode actions */
  registerHost: (address: string, clusterId: string, adminUser: string, adminPass: string) => Promise<void>
  deregisterHost: (hostId: string) => Promise<void>
  setMaintenance: (hostId: string, enabled: boolean) => Promise<void>
  createCluster: (name: string, description: string) => Promise<string>
  deleteCluster: (id: string) => Promise<void>
  createDatastore: (data: { name: string; store_type: string; mount_source: string; mount_opts: string; mount_path: string; cluster_id: string }) => Promise<void>
  migrateVm: (vmId: string, targetHostId: string) => Promise<void>
  applyDrsRecommendation: (id: number) => Promise<void>
  dismissDrsRecommendation: (id: number) => Promise<void>
  acknowledgeAlarm: (id: number) => Promise<void>
}

export const useClusterStore = create<ClusterState>((set, get) => ({
  backendMode: 'standalone',
  clusterUrl: null,
  clusterName: null,
  hosts: [],
  clusters: [],
  datastores: [],
  clusterStats: null,
  tasks: [],
  events: [],
  drsRecommendations: [],
  alarms: [],

  detectBackend: async () => {
    try {
      const { data } = await api.get<SystemInfoExtended>('/api/system/info')
      const mode = data.mode || 'standalone'
      set({
        backendMode: mode,
        clusterUrl: data.cluster_url || null,
        clusterName: data.cluster_name || null,
      })
    } catch {
      set({ backendMode: 'standalone' })
    }
  },

  fetchHosts: async () => {
    const { data } = await api.get<Host[]>('/api/hosts')
    set({ hosts: data })
  },

  fetchClusters: async () => {
    const { data } = await api.get<Cluster[]>('/api/clusters')
    set({ clusters: data })
  },

  fetchDatastores: async () => {
    const { data } = await api.get<Datastore[]>('/api/storage/datastores')
    set({ datastores: data })
  },

  fetchClusterStats: async () => {
    const { data } = await api.get<ClusterStats>('/api/system/stats')
    set({ clusterStats: data })
  },

  fetchTasks: async () => {
    const { data } = await api.get<Task[]>('/api/tasks')
    set({ tasks: data })
  },

  fetchEvents: async () => {
    const { data } = await api.get<ClusterEvent[]>('/api/events')
    set({ events: data })
  },

  fetchDrsRecommendations: async () => {
    const { data } = await api.get<DrsRecommendation[]>('/api/drs/recommendations')
    set({ drsRecommendations: data })
  },

  fetchAlarms: async () => {
    const { data } = await api.get<Alarm[]>('/api/alarms')
    set({ alarms: data })
  },

  registerHost: async (address, clusterId, adminUser, adminPass) => {
    await api.post('/api/hosts', { address, cluster_id: clusterId, admin_username: adminUser, admin_password: adminPass })
    await get().fetchHosts()
  },

  deregisterHost: async (hostId) => {
    await api.delete(`/api/hosts/${hostId}`)
    await get().fetchHosts()
  },

  setMaintenance: async (hostId, enabled) => {
    const endpoint = enabled ? 'maintenance' : 'activate'
    await api.post(`/api/hosts/${hostId}/${endpoint}`)
    await get().fetchHosts()
  },

  createCluster: async (name, description) => {
    const { data } = await api.post('/api/clusters', { name, description })
    await get().fetchClusters()
    return data.id
  },

  deleteCluster: async (id) => {
    await api.delete(`/api/clusters/${id}`)
    await get().fetchClusters()
  },

  createDatastore: async (data) => {
    await api.post('/api/storage/datastores', data)
    await get().fetchDatastores()
  },

  migrateVm: async (vmId, targetHostId) => {
    await api.post(`/api/vms/${vmId}/migrate`, { target_host_id: targetHostId })
  },

  applyDrsRecommendation: async (id) => {
    await api.post(`/api/drs/${id}/apply`)
    await get().fetchDrsRecommendations()
  },

  dismissDrsRecommendation: async (id) => {
    await api.post(`/api/drs/${id}/dismiss`)
    await get().fetchDrsRecommendations()
  },

  acknowledgeAlarm: async (id) => {
    await api.post(`/api/alarms/${id}/acknowledge`)
    await get().fetchAlarms()
  },
}))
