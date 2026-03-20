import { Outlet } from 'react-router-dom'
import Sidebar from './Sidebar'
import Header from './Header'
import StatusBar from './StatusBar'

export default function Layout() {
  return (
    <div className="h-screen flex flex-col bg-vmm-bg">
      <Header />
      <div className="flex flex-1 overflow-hidden">
        <Sidebar />
        <main className="flex-1 overflow-y-auto p-6">
          <Outlet />
        </main>
      </div>
      <StatusBar />
    </div>
  )
}
