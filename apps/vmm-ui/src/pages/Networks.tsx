import { useEffect, useState } from 'react'
import { Outlet, useLocation, useNavigate } from 'react-router-dom'
import { Network, Globe, AlertCircle, CheckCircle, Activity } from 'lucide-react'
import api from '../api/client'
import type { NetworkStats } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import SpecRow from '../components/SpecRow'
import { formatBytes } from '../utils/format'

export default function Networks() {
  const [stats, setStats] = useState<NetworkStats | null>(null)
  const location = useLocation()
  const navigate = useNavigate()

  useEffect(() => {
    api.get<NetworkStats>('/api/network/stats').then(({ data }) => setStats(data))
    const interval = setInterval(() => {
      api.get<NetworkStats>('/api/network/stats').then(({ data }) => setStats(data))
    }, 5000)
    return () => clearInterval(interval)
  }, [])

  // Redirect /networks to /networks/nat
  useEffect(() => {
    if (location.pathname === '/networks') navigate('/networks/overview', { replace: true })
  }, [location.pathname, navigate])

  return (
    <div className="space-y-6">
      {/* Content from sub-route */}
      <Outlet context={{ stats }} />
    </div>
  )
}
