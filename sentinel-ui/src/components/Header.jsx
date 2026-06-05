import { useState, useEffect } from 'react'
import { useAppStore, MISSIONS } from '../stores/useAppStore'
import { useEventStore } from '../stores/useEventStore'

const MISSION_LABELS = {
  dashboard: 'DASHBOARD',
  survey:    'SURVEY',
  hunt:      'HUNT',
  drone:     'DRONE',
  tscm:      'TSCM',
  report:    'REPORT',
}

const MISSION_COLORS = {
  dashboard: 'var(--text-secondary)',
  survey:    'var(--survey)',
  hunt:      'var(--hunt)',
  drone:     'var(--drone)',
  tscm:      'var(--tscm)',
  report:    'var(--report)',
}

export default function Header() {
  const { mission, setMission } = useAppStore()
  const { events } = useEventStore()
  const alertCount = events.filter(e => e.severity === 'CRITICAL' || e.type === 'anomaly').length

  return (
    <header style={{
      height: 48,
      background: 'var(--bg-surface)',
      borderBottom: '1px solid var(--border)',
      display: 'flex',
      alignItems: 'center',
      padding: '0 16px',
      gap: 8,
      flexShrink: 0,
    }}>
      {/* Wordmark */}
      <span className="mono" style={{ color: 'var(--danger)', fontWeight: 700, fontSize: 15, marginRight: 16, letterSpacing: 1 }}>
        RF-SENTINEL
      </span>

      {/* Mission pills */}
      <nav style={{ display: 'flex', gap: 4 }}>
        {MISSIONS.map(m => {
          const active = mission === m
          const color = MISSION_COLORS[m]
          return (
            <button
              key={m}
              onClick={() => setMission(m)}
              style={{
                background: active ? `${color}22` : 'transparent',
                color: active ? color : 'var(--text-secondary)',
                border: `1px solid ${active ? color : 'transparent'}`,
                borderRadius: 4,
                padding: '3px 10px',
                fontSize: 12,
                fontWeight: active ? 600 : 400,
                letterSpacing: 0.5,
              }}
            >
              {MISSION_LABELS[m]}
            </button>
          )
        })}
      </nav>

      {/* Spacer */}
      <div style={{ flex: 1 }} />

      {/* Alert badge */}
      {alertCount > 0 && (
        <span style={{
          background: 'var(--danger)',
          color: '#fff',
          borderRadius: 10,
          padding: '2px 8px',
          fontSize: 11,
          fontWeight: 700,
        }}>
          {alertCount} ALERT{alertCount !== 1 ? 'S' : ''}
        </span>
      )}

      {/* Clock */}
      <Clock />
    </header>
  )
}

function Clock() {
  const [time, setTime] = useState(new Date())
  useEffect(() => {
    const t = setInterval(() => setTime(new Date()), 1000)
    return () => clearInterval(t)
  }, [])
  return (
    <span className="mono" style={{ color: 'var(--text-secondary)', fontSize: 12 }}>
      {time.toUTCString().slice(17, 25)} UTC
    </span>
  )
}

