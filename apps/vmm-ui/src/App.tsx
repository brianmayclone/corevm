import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom'
import { useEffect } from 'react'
import { useAuthStore } from './stores/authStore'
import Layout from './components/Layout'
import Login from './pages/Login'
import Dashboard from './pages/Dashboard'
import VmDetail from './pages/VmDetail'
import VmCreate from './pages/VmCreate'
import VmConsole from './pages/VmConsole'
import Storage from './pages/Storage'
import StorageOverview from './pages/StorageOverview'
import StorageLocal from './pages/StorageLocal'
import StorageShared from './pages/StorageShared'
import StorageDisks from './pages/StorageDisks'
import StorageQos from './pages/StorageQos'
import Networks from './pages/Networks'
import NetworkOverview from './pages/NetworkOverview'
import NetworkNat from './pages/NetworkNat'
import NetworkHostOnly from './pages/NetworkHostOnly'
import NetworkAdapters from './pages/NetworkAdapters'
import NetworkVlans from './pages/NetworkVlans'

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated } = useAuthStore()
  if (!isAuthenticated) return <Navigate to="/login" replace />
  return <>{children}</>
}

export default function App() {
  const { loadFromStorage } = useAuthStore()
  useEffect(() => { loadFromStorage() }, [])

  return (
    <BrowserRouter>
      <Routes>
        <Route path="/login" element={<Login />} />
        <Route path="/" element={<ProtectedRoute><Layout /></ProtectedRoute>}>
          <Route index element={<Dashboard />} />
          <Route path="vms/create" element={<VmCreate />} />
          <Route path="vms/:id" element={<VmDetail />} />
          <Route path="vms/:id/console" element={<VmConsole />} />
          <Route path="vms/:id/settings" element={<VmCreate />} />
          <Route path="storage" element={<Storage />}>
            <Route path="overview" element={<StorageOverview />} />
            <Route path="local" element={<StorageLocal />} />
            <Route path="shared" element={<StorageShared />} />
            <Route path="disks" element={<StorageDisks />} />
            <Route path="qos" element={<StorageQos />} />
          </Route>
          <Route path="networks" element={<Networks />}>
            <Route path="overview" element={<NetworkOverview />} />
            <Route path="nat" element={<NetworkNat />} />
            <Route path="host-only" element={<NetworkHostOnly />} />
            <Route path="adapters" element={<NetworkAdapters />} />
            <Route path="vlans" element={<NetworkVlans />} />
          </Route>
          <Route path="settings" element={<Placeholder title="Settings" />} />
        </Route>
      </Routes>
    </BrowserRouter>
  )
}

function Placeholder({ title }: { title: string }) {
  return (
    <div className="text-vmm-text-muted text-sm py-12 text-center">
      {title} management — coming soon
    </div>
  )
}
