import { useEffect } from 'react'
import { Outlet, useLocation, useNavigate } from 'react-router-dom'

export default function Settings() {
  const location = useLocation()
  const navigate = useNavigate()

  useEffect(() => {
    if (location.pathname === '/settings') navigate('/settings/ui', { replace: true })
  }, [location.pathname, navigate])

  return <Outlet />
}
