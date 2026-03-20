import { create } from 'zustand'
import api from '../api/client'
import type { VmSummary, VmDetail } from '../api/types'

interface VmState {
  vms: VmSummary[]
  loading: boolean
  fetchVms: () => Promise<void>
  startVm: (id: string) => Promise<void>
  stopVm: (id: string) => Promise<void>
  forceStopVm: (id: string) => Promise<void>
  deleteVm: (id: string) => Promise<void>
}

export const useVmStore = create<VmState>((set, get) => ({
  vms: [],
  loading: false,

  fetchVms: async () => {
    set({ loading: true })
    try {
      const { data } = await api.get<VmSummary[]>('/api/vms')
      set({ vms: data })
    } finally {
      set({ loading: false })
    }
  },

  startVm: async (id) => {
    await api.post(`/api/vms/${id}/start`)
    await get().fetchVms()
  },

  stopVm: async (id) => {
    await api.post(`/api/vms/${id}/stop`)
    await get().fetchVms()
  },

  forceStopVm: async (id) => {
    await api.post(`/api/vms/${id}/force-stop`)
    await get().fetchVms()
  },

  deleteVm: async (id) => {
    await api.delete(`/api/vms/${id}`)
    await get().fetchVms()
  },
}))
