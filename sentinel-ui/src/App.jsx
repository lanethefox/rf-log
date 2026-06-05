import { useEffect } from 'react'
import { useAppStore } from './stores/useAppStore'
import { useSpectrumStore } from './stores/useSpectrumStore'
import { useEventStore } from './stores/useEventStore'
import { connectSpectrumWs, connectEventsWs } from './api/client'
import Header from './components/Header'
import Footer from './components/Footer'
import Dashboard from './missions/Dashboard'
import Survey from './missions/Survey'
import Hunt from './missions/Hunt'
import Drone from './missions/Drone'
import Tscm from './missions/Tscm'
import Report from './missions/Report'

export default function App() {
  const { mission } = useAppStore()
  const { setFrame } = useSpectrumStore()
  const { addEvent } = useEventStore()

  useEffect(() => {
    const closeSpectrum = connectSpectrumWs(setFrame)
    const closeEvents = connectEventsWs(addEvent)
    return () => { closeSpectrum(); closeEvents() }
  }, [])

  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      height: '100vh',
      background: 'var(--bg-base)',
      color: 'var(--text-primary)',
      fontFamily: 'var(--font-sans)',
      overflow: 'hidden',
    }}>
      <Header />
      <main style={{ flex: 1, overflow: 'hidden', display: 'flex', flexDirection: 'column' }}>
        {mission === 'dashboard' && <Dashboard />}
        {mission === 'survey'    && <Survey />}
        {mission === 'hunt'      && <Hunt />}
        {mission === 'drone'     && <Drone />}
        {mission === 'tscm'      && <Tscm />}
        {mission === 'report'    && <Report />}
      </main>
      <Footer />
    </div>
  )
}
