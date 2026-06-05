import { useEffect, useState } from 'react'
import { api } from '../api/client'
import { useEventStore } from '../stores/useEventStore'
import TabBar from '../components/TabBar'
import Placeholder from '../components/Placeholder'

const TABS = ['AIRSPACE', 'DETECTIONS', 'SIGNATURES', 'REMOTE-ID', 'HISTORY']

export default function Drone() {
  const [tab, setTab] = useState('AIRSPACE')
  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <TabBar tabs={TABS} active={tab} color="var(--drone)" onChange={setTab} />
      <div style={{ flex: 1, overflow: 'auto', padding: 16 }}>
        {tab === 'AIRSPACE'   && <AirspaceTab />}
        {tab === 'DETECTIONS' && <DetectionsTab />}
        {tab === 'SIGNATURES' && <SignaturesTab />}
        {tab === 'REMOTE-ID'  && <Placeholder title="Remote ID Decode" subtitle="FAA ASTM F3411 message feed from ESP32 sensors (Phase 2)" color="var(--drone)" />}
        {tab === 'HISTORY'    && <Placeholder title="Drone History" subtitle="Historical drone activity log and repeat visitor analysis" color="var(--drone)" />}
      </div>
    </div>
  )
}

function AirspaceTab() {
  const [tracks, setTracks] = useState([])
  const { events } = useEventStore()

  useEffect(() => {
    api.drones.tracks().then(setTracks).catch(() => {})
    const t = setInterval(() => api.drones.tracks().then(setTracks).catch(() => {}), 3000)
    return () => clearInterval(t)
  }, [])

  const now = Date.now() / 1000
  const activeTracks = tracks.filter(t => now - t.last_seen < 120)
  const threatLevel = activeTracks.length === 0 ? 'CLEAR' : activeTracks.length === 1 ? 'ACTIVE' : 'MULTIPLE'
  const threatColor = { CLEAR: 'var(--success)', ACTIVE: 'var(--drone)', MULTIPLE: 'var(--danger)' }[threatLevel]

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {/* Threat level banner */}
      <div className="card" style={{ borderLeft: `4px solid ${threatColor}`, display: 'flex', alignItems: 'center', gap: 16 }}>
        <div>
          <div style={{ fontSize: 10, color: 'var(--text-muted)', letterSpacing: 1 }}>AIRSPACE STATUS</div>
          <div className="mono" style={{ fontSize: 22, color: threatColor, fontWeight: 700 }}>{threatLevel}</div>
        </div>
        <div style={{ color: 'var(--text-secondary)', fontSize: 13 }}>
          {activeTracks.length} active drone{activeTracks.length !== 1 ? 's' : ''} detected
        </div>
      </div>

      {/* Active drone cards */}
      {activeTracks.length === 0 && (
        <div style={{ color: 'var(--text-muted)', fontSize: 13, textAlign: 'center', padding: 32 }}>
          No drones detected in the last 2 minutes
        </div>
      )}
      {activeTracks.map(d => (
        <div key={d.id} className="card" style={{ borderLeft: `3px solid ${d.whitelisted ? 'var(--text-muted)' : 'var(--drone)'}` }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
            <div>
              <span style={{ fontSize: 14, color: d.whitelisted ? 'var(--text-secondary)' : 'var(--drone)', fontWeight: 600 }}>
                ✈ {d.manufacturer || 'Unknown manufacturer'} {d.model || ''}
              </span>
              {d.whitelisted && <span style={{ marginLeft: 8, fontSize: 11, color: 'var(--success)' }}>WHITELISTED</span>}
            </div>
            <span className="mono" style={{ fontSize: 11, color: 'var(--text-muted)' }}>
              {new Date(d.last_seen * 1000).toLocaleTimeString()}
            </span>
          </div>
          <div style={{ marginTop: 6, fontSize: 12, color: 'var(--text-secondary)' }}>
            Methods: {d.detection_methods || 'RF'}
            {d.serial_number && ` · SN: ${d.serial_number}`}
          </div>
        </div>
      ))}
    </div>
  )
}

function DetectionsTab() {
  const [detections, setDetections] = useState([])
  useEffect(() => {
    api.drones.detections().then(setDetections).catch(() => {})
  }, [])

  const METHOD_COLORS = {
    RfSignature: 'var(--drone)',
    RemoteId: 'var(--survey)',
    WifiSsid: 'var(--tscm)',
    BleMac: 'var(--hunt)',
    WifiMac: 'var(--tscm)',
  }

  return (
    <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
      <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 12 }}>
        <thead>
          <tr style={{ background: 'var(--bg-elevated)' }}>
            {['TIME', 'METHOD', 'MANUFACTURER', 'MODEL', 'CONF', 'SIGNAL', 'FREQ'].map(h => (
              <th key={h} style={{ padding: '8px 12px', textAlign: 'left', color: 'var(--text-muted)', fontWeight: 500, fontSize: 10, letterSpacing: 0.5 }}>{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {detections.length === 0 && (
            <tr><td colSpan={7} style={{ padding: 16, color: 'var(--text-muted)', textAlign: 'center' }}>No detections yet</td></tr>
          )}
          {detections.map(d => (
            <tr key={d.id} style={{ borderBottom: '1px solid var(--border)' }}
              onMouseEnter={e => e.currentTarget.style.background = 'var(--bg-elevated)'}
              onMouseLeave={e => e.currentTarget.style.background = ''}>
              <td className="mono" style={{ padding: '7px 12px', fontSize: 11, color: 'var(--text-muted)' }}>{new Date(d.detected_at * 1000).toLocaleTimeString()}</td>
              <td style={{ padding: '7px 12px' }}>
                <span style={{ color: METHOD_COLORS[d.detection_method] || 'var(--text-secondary)', fontSize: 11 }}>{d.detection_method}</span>
              </td>
              <td style={{ padding: '7px 12px' }}>{d.manufacturer || '—'}</td>
              <td style={{ padding: '7px 12px', color: 'var(--text-secondary)' }}>{d.model || '—'}</td>
              <td className="mono" style={{ padding: '7px 12px' }}>{d.confidence ? `${(d.confidence * 100).toFixed(0)}%` : '—'}</td>
              <td className="mono" style={{ padding: '7px 12px', color: 'var(--text-secondary)' }}>{d.signal_dbm ? `${d.signal_dbm.toFixed(0)} dBm` : '—'}</td>
              <td className="mono" style={{ padding: '7px 12px', color: 'var(--text-secondary)' }}>{d.freq_mhz ? `${d.freq_mhz.toFixed(1)}` : '—'}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function SignaturesTab() {
  const [sigs, setSigs] = useState([])
  useEffect(() => {
    api.drones.signatures().then(setSigs).catch(() => {})
  }, [])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      {sigs.map(s => (
        <div key={s.id} className="card" style={{ borderLeft: '3px solid var(--drone)' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span style={{ fontWeight: 600, color: 'var(--drone)' }}>{s.manufacturer}</span>
            <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>{s.builtin ? 'BUILT-IN' : 'CUSTOM'}</span>
          </div>
          <div style={{ fontSize: 13, marginTop: 2 }}>{s.model}</div>
          <div style={{ fontSize: 11, color: 'var(--text-secondary)', marginTop: 4 }}>
            BW: {s.bandwidth_mhz} MHz
            {s.notes && ` · ${s.notes}`}
          </div>
        </div>
      ))}
    </div>
  )
}
