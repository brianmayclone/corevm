import { useEffect, useState } from 'react'

/** Detect mobile device in landscape orientation. */
export function useMobileLandscape(): boolean {
  const [isLandscape, setIsLandscape] = useState(false)

  useEffect(() => {
    const check = () => {
      const isMobile = window.innerWidth < 1024 && ('ontouchstart' in window || navigator.maxTouchPoints > 0)
      const landscape = window.innerWidth > window.innerHeight
      setIsLandscape(isMobile && landscape)
    }

    check()
    window.addEventListener('resize', check)
    window.addEventListener('orientationchange', check)

    // Also listen to screen orientation API if available
    const mql = window.matchMedia('(orientation: landscape)')
    mql.addEventListener('change', check)

    return () => {
      window.removeEventListener('resize', check)
      window.removeEventListener('orientationchange', check)
      mql.removeEventListener('change', check)
    }
  }, [])

  return isLandscape
}
