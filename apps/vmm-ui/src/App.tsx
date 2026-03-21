import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom'
import { useEffect } from 'react'
import { useAuthStore } from './stores/authStore'
import { useUiStore } from './stores/uiStore'
import { useClusterStore } from './stores/clusterStore'
import Layout from './components/Layout'
import Login from './pages/Login'
import Dashboard from './pages/Dashboard'
import Machines from './pages/Machines'
import MachinesList from './pages/MachinesList'
import ResourceGroups from './pages/ResourceGroups'
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
import TerminalPage from './pages/TerminalPage'
import ManagedModePage from './pages/ManagedModePage'
// Cluster-mode pages
import ClusterDashboard from './pages/ClusterDashboard'
import ClusterDetail from './pages/ClusterDetail'
import HostsList from './pages/HostsList'
import HostDetail from './pages/HostDetail'
import AddHost from './pages/AddHost'
import ClusterSettings from './pages/ClusterSettings'
import DatastoresList from './pages/DatastoresList'
import TasksList from './pages/TasksList'
import EventsList from './pages/EventsList'
import DrsPage from './pages/DrsPage'
import AlarmsList from './pages/AlarmsList'
import NotificationsPage from './pages/NotificationsPage'
import SdnNetworks from './pages/SdnNetworks'
import SdnNetworkDetail from './pages/SdnNetworkDetail'

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const { isAuthenticated } = useAuthStore()
  if (!isAuthenticated) return <Navigate to="/login" replace />
  return <>{children}</>
}

export default function App() {
  const { loadFromStorage } = useAuthStore()
  const { loadFromStorage: loadUi } = useUiStore()
  const { backendMode, detectBackend } = useClusterStore()

  useEffect(() => {
    loadFromStorage()
    loadUi()
    detectBackend()
  }, [])

  // If this node is managed by a cluster, show full-screen notice
  if (backendMode === 'managed') {
    return (
      <BrowserRouter>
        <Routes>
          <Route path="/login" element={<Login />} />
          <Route path="*" element={<ManagedModePage />} />
        </Routes>
      </BrowserRouter>
    )
  }

  return (
    <BrowserRouter>
      <Routes>
        <Route path="/login" element={<Login />} />
        <Route path="/" element={<ProtectedRoute><Layout /></ProtectedRoute>}>
          <Route index element={backendMode === 'cluster' ? <ClusterDashboard /> : <Dashboard />} />
          <Route path="machines" element={<Machines />}>
            <Route path="overview" element={<Dashboard />} />
            <Route path="list" element={<MachinesList />} />
            <Route path="resource-groups" element={<ResourceGroups />} />
          </Route>
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
          <Route path="terminal" element={<TerminalPage />} />
          <Route path="settings" element={<Settings />}>
            <Route path="ui" element={<SettingsUi />} />
            <Route path="users" element={<SettingsUsers />} />
            <Route path="groups" element={<SettingsGroups />} />
            <Route path="time" element={<SettingsTime />} />
            <Route path="server" element={<SettingsServer />} />
          </Route>
          {/* ── Cluster-mode routes (only functional when mode === 'cluster') ── */}
          <Route path="cluster/hosts" element={<HostsList />} />
          <Route path="cluster/hosts/add" element={<AddHost />} />
          <Route path="cluster/hosts/:id" element={<HostDetail />} />
          <Route path="cluster/settings" element={<ClusterSettings />} />
          <Route path="cluster/detail/:id" element={<ClusterDetail />} />
          <Route path="cluster/datastores" element={<DatastoresList />} />
          <Route path="cluster/drs" element={<DrsPage />} />
          <Route path="operations/tasks" element={<TasksList />} />
          <Route path="operations/events" element={<EventsList />} />
          <Route path="operations/alarms" element={<AlarmsList />} />
          <Route path="operations/notifications" element={<NotificationsPage />} />
          <Route path="cluster/networks" element={<SdnNetworks />} />
          <Route path="cluster/networks/:id" element={<SdnNetworkDetail />} />
          {/* SDN network routes (accessible from Networks sidebar section) */}
          <Route path="networks/overview" element={<SdnNetworks />} />
          <Route path="networks/dhcp" element={<SdnNetworks />} />
          <Route path="networks/dns" element={<SdnNetworks />} />
          <Route path="networks/pxe" element={<SdnNetworks />} />
        </Route>
      </Routes>
    </BrowserRouter>
  )
}
