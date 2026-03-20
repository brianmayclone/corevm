import { BrowserRouter, Routes, Route, Navigate } from 'react-router-dom'
import { useEffect } from 'react'
import { useAuthStore } from './stores/authStore'
import Layout from './components/Layout'
import Login from './pages/Login'
import Dashboard from './pages/Dashboard'
import VmDetail from './pages/VmDetail'
import VmCreate from './pages/VmCreate'

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
          <Route path="vms/:id/settings" element={<VmCreate />} />
          <Route path="storage" element={<Placeholder title="Storage" />} />
          <Route path="networks" element={<Placeholder title="Networks" />} />
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
