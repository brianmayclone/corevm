import { create } from 'zustand'
import api from '../api/client'
import type { User } from '../api/types'

interface AuthState {
  user: User | null
  token: string | null
  isAuthenticated: boolean
  login: (username: string, password: string) => Promise<void>
  logout: () => void
  loadFromStorage: () => void
}

export const useAuthStore = create<AuthState>((set) => ({
  user: null,
  token: localStorage.getItem('vmm_token'),
  isAuthenticated: !!localStorage.getItem('vmm_token'),

  login: async (username, password) => {
    const { data } = await api.post('/api/auth/login', { username, password })
    localStorage.setItem('vmm_token', data.access_token)
    set({ user: data.user, token: data.access_token, isAuthenticated: true })
  },

  logout: () => {
    localStorage.removeItem('vmm_token')
    set({ user: null, token: null, isAuthenticated: false })
  },

  loadFromStorage: () => {
    const token = localStorage.getItem('vmm_token')
    if (token) {
      api.get('/api/auth/me').then(({ data }) => {
        set({ user: data, token, isAuthenticated: true })
      }).catch(() => {
        localStorage.removeItem('vmm_token')
        set({ user: null, token: null, isAuthenticated: false })
      })
    }
  },
}))
