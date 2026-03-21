import { useEffect } from 'react'
import { Outlet, useLocation, useNavigate } from 'react-router-dom'

export default function Machines() {
  const location = useLocation()
  const navigate = useNavigate()

  useEffect(() => {
    if (location.pathname === '/machines') navigate('/machines/overview', { replace: true })
  }, [location.pathname, navigate])

  return <Outlet />
}
