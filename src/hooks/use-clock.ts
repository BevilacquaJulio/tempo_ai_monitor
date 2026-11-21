import { useEffect, useState } from 'react'

export function useClock(enabled: boolean, interval = 1000) {
  const [now, setNow] = useState(Date.now)
  useEffect(() => {
    if (!enabled) return
    let timer: number | undefined
    const update = () => setNow(Date.now())
    const restart = () => {
      window.clearInterval(timer)
      if (document.hidden) return
      update()
      timer = window.setInterval(update, interval)
    }
    restart()
    document.addEventListener('visibilitychange', restart)
    window.addEventListener('focus', update)
    return () => {
      window.clearInterval(timer)
      document.removeEventListener('visibilitychange', restart)
      window.removeEventListener('focus', update)
    }
  }, [enabled, interval])
  return now
}
