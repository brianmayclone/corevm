import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom'
import { useEffect } from 'react'
import { useAuthStore } from './stores/authStore'
import { useUiStore } from './stores/uiStore'
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
import Settings from './pages/Settings'
import SettingsUi from './pages/SettingsUi'
import SettingsUsers from './pages/SettingsUsers'
import SettingsGroups from './pages/SettingsGroups'
import SettingsTime from './pages/SettingsTime'
import SettingsServer from './pages/SettingsServer'

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated } = useAuthStore()
  if (!isAuthenticated) return <Navigate to="/login" replace />
  return <>{children}</>
}

export default function App() {
  const { loadFromStorage } = useAuthStore()
  const { loadFromStorage: loadUi } = useUiStore()
  useEffect(() => { loadFromStorage(); loadUi() }, [])

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
          <Route path="settings" element={<Settings />}>
            <Route path="ui" element={<SettingsUi />} />
            <Route path="users" element={<SettingsUsers />} />
            <Route path="groups" element={<SettingsGroups />} />
            <Route path="time" element={<SettingsTime />} />
            <Route path="server" element={<SettingsServer />} />
          </Route>
        </Route>
      </Routes>
    </BrowserRouter>
  )
}

