import { useEffect } from 'react'
import { Outlet, useLocation, useNavigate } from 'react-router-dom'

export default function Storage() {
  const location = useLocation()
  const navigate = useNavigate()

  useEffect(() => {
    if (location.pathname === '/storage') navigate('/storage/overview', { replace: true })
  }, [location.pathname, navigate])

  return <Outlet />
}
