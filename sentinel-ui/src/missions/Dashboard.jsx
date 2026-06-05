import { useEffect, useState } from 'react'
import { useSpectrumStore } from '../stores/useSpectrumStore'
import { useEventStore } from '../stores/useEventStore'
import { useAppStore } from '../stores/useAppStore'
import { api } from '../api/client'
import SpectrumCanvas from '../canvas/SpectrumCanvas'

export default function Dashboard() {
  const { frame } = useSpectrumStore()
  const { events } = useEventStore()
  const { setMission } = useAppStore()
  const [emitters, setEmitters] = useState([])
  const [droneTracks, setDroneTracks] = useState([])
  const [baselines, setBaselines] = useState([])

  useEffect(() => {
    api.emitters.list().then(setEmitters).catch(() => {})
    api.drones.tracks().then(setDroneTracks).catch(() => {})
    api.baselines.list().then(setBaselines).catch(() => {})
    const interval = setInterval(() => {
      api.emitters.list().then(setEmitters).catch(() => {})
      api.drones.tracks().then(setDroneTracks).catch(() => {})
    }, 5000)
    return () => clearInterval(interval)
  }, [])

  const known   = emitters.filter(e => e.status === 'KNOWN').length
  const unknown = emitters.filter(e => e.status === 'UNKNOWN').length
  const newCount = emitters.filter(e => e.status === 'NEW').length
  const activeDrones = droneTracks.filter(d => {
    const age = Date.now() / 1000 - d.last_seen
    return age < 120
  })
  const anomalies = events.filter(e => e.type === 'anomaly').slice(0, 8)
  const droneEvents = events.filter(e => e.type === 'drone').slice(0, 3)
  const allAlerts = [...anomalies, ...droneEvents]
    .sort((a, b) => (b.detected_at || 0) - (a.detected_at || 0))
    .slice(0, 8)

  return (
    <div style={{ display: 'grid', gridTemplateRows: 'auto 1fr', gap: 12, padding: 16, height: '100%', overflowY: 'auto' }}>
      {/* Top row: spectrum mini */}
      <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
        <div style={{ padding: '8px 12px', borderBottom: '1px solid var(--border)', fontSize: 11, color: 'var(--text-secondary)', letterSpacing: 0.5 }}>
          LIVE SPECTRUM
        </div>
        <SpectrumCanvas frame={frame} height={140} />
      </div>

      {/* Bottom row: 4 cards */}
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr 1fr 1fr', gap: 12, alignContent: 'start' }}>

        {/* Emitter Summary */}
        <DashCard title="EMITTERS" accent="var(--hunt)" onClick={() => setMission('hunt')}>
          <Stat label="Known"   value={known}    color="var(--success)" />
          <Stat label="Unknown" value={unknown}  color="var(--warning)" />
          <Stat label="New"     value={newCount} color="var(--danger)" />
          <Stat label="Total"   value={emitters.length} color="var(--text-secondary)" />
        </DashCard>

        {/* Drone Status */}
        <DashCard title="DRONE STATUS" accent="var(--drone)" onClick={() => setMission('drone')}>
          <Stat
            label="Active"
            value={activeDrones.length}
            color={activeDrones.length > 0 ? 'var(--drone)' : 'var(--text-muted)'}
          />
          <Stat label="Tracked" value={droneTracks.length} color="var(--text-secondary)" />
          {activeDrones.length === 0 && (
            <div style={{ marginTop: 8, fontSize: 11, color: 'var(--success)' }}>AIRSPACE CLEAR</div>
          )}
          {activeDrones.map(d => (
            <div key={d.id} style={{ fontSize: 11, color: 'var(--drone)', marginTop: 4 }}>
              ✈ {d.manufacturer || 'Unknown'} {d.model || ''}
            </div>
          ))}
        </DashCard>

        {/* Baseline Status */}
        <DashCard title="BASELINE" accent="var(--survey)" onClick={() => setMission('survey')}>
          <Stat label="Baselines" value={baselines.length} color="var(--survey)" />
          {baselines[0] ? (
            <>
              <div style={{ fontSize: 11, color: 'var(--text-secondary)', marginTop: 6 }}>
                Latest: {baselines[0].name}
              </div>
              <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 2 }}>
                {new Date(baselines[0].captured_at * 1000).toLocaleDateString()}
              </div>
            </>
          ) : (
            <div style={{ marginTop: 8, fontSize: 11, color: 'var(--warning)' }}>No baseline captured</div>
          )}
        </DashCard>

        {/* Anomaly Feed */}
        <DashCard title="ANOMALY FEED" accent="var(--danger)" onClick={() => setMission('hunt')}>
          {allAlerts.length === 0 ? (
            <div style={{ fontSize: 11, color: 'var(--success)', marginTop: 4 }}>✓ Environment stable</div>
          ) : (
            allAlerts.map((evt, i) => (
              <div key={i} style={{ fontSize: 11, marginTop: 4, color: evt.type === 'drone' ? 'var(--drone)' : 'var(--warning)' }}>
                ⚠ {evt.type === 'drone' ? `Drone ${evt.manufacturer || ''}` : `${evt.freq_mhz?.toFixed(1)} MHz`}
              </div>
            ))
          )}
        </DashCard>

        {/* Quick Actions — spans full width */}
        <div className="card" style={{ gridColumn: '1 / -1', display: 'flex', gap: 10, alignItems: 'center' }}>
          <span style={{ fontSize: 11, color: 'var(--text-secondary)', letterSpacing: 0.5, marginRight: 8 }}>QUICK ACTIONS</span>
          <button onClick={() => setMission('survey')}>Start Survey</button>
          <button onClick={() => setMission('survey')}>Capture Baseline</button>
          <button onClick={() => setMission('drone')} style={{ color: 'var(--drone)' }}>Drone Watch</button>
          <button onClick={() => setMission('tscm')} style={{ color: 'var(--tscm)' }}>TSCM Check</button>
          <button onClick={() => setMission('report')} style={{ color: 'var(--report)' }}>Export Report</button>
        </div>
      </div>
    </div>
  )
}

function DashCard({ title, accent, onClick, children }) {
  return (
    <div
      className="card"
      onClick={onClick}
      style={{ cursor: 'pointer', borderTop: `2px solid ${accent}`, transition: 'background 0.15s' }}
      onMouseEnter={e => e.currentTarget.style.background = 'var(--bg-elevated)'}
      onMouseLeave={e => e.currentTarget.style.background = 'var(--bg-surface)'}
    >
      <div style={{ fontSize: 10, color: 'var(--text-muted)', letterSpacing: 1, marginBottom: 8 }}>{title}</div>
      {children}
    </div>
  )
}

function Stat({ label, value, color }) {
  return (
    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 4 }}>
      <span style={{ fontSize: 12, color: 'var(--text-secondary)' }}>{label}</span>
      <span className="mono" style={{ fontSize: 16, fontWeight: 700, color }}>{value}</span>
    </div>
  )
}
