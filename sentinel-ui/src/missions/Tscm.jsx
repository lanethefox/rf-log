import { useEffect, useRef, useState } from 'react'
import { api } from '../api/client'
import { useSpectrumStore } from '../stores/useSpectrumStore'
import TabBar from '../components/TabBar'
import Placeholder from '../components/Placeholder'

const TABS = ['CHECKLIST', 'NEAR-FIELD', 'CELLULAR', 'WIRELESS', 'HARMONICS']

const CHECKLIST_STEPS = [
  { id: 'baseline',  label: 'Capture room baseline (empty room, known clean)' },
  { id: 'sweep',     label: 'Broadband sweep (25 MHz – 6 GHz)' },
  { id: 'nearfield', label: 'Near-field scan (walk room perimeter)' },
  { id: 'cellular',  label: 'Cellular anomaly check (unexpected base stations)' },
  { id: 'wireless',  label: 'WiFi/BT audit (unknown access points, BLE beacons)' },
  { id: 'drone',     label: 'Drone sweep (2.4/5.8 GHz control links, Remote ID, video downlinks)' },
  { id: 'harmonics', label: 'Harmonic scan (clock leakage from hidden electronics)' },
  { id: 'compare',   label: 'Compare to baseline (what\'s new?)' },
  { id: 'report',    label: 'Generate report' },
]

export default function Tscm() {
  const [tab, setTab] = useState('CHECKLIST')
  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <TabBar tabs={TABS} active={tab} color="var(--tscm)" onChange={setTab} />
      <div style={{ flex: 1, overflow: 'auto', padding: 16 }}>
        {tab === 'CHECKLIST' && <ChecklistTab />}
        {tab === 'NEAR-FIELD' && <NearFieldTab />}
        {tab === 'CELLULAR'   && <CellularTab />}
        {tab === 'WIRELESS'   && <Placeholder title="WiFi / BT Audit" subtitle="Connected AP and BLE device inventory (Phase 2 adds ESP32 distributed nodes)" color="var(--tscm)" />}
        {tab === 'HARMONICS'  && <HarmonicsTab />}
      </div>
    </div>
  )
}

// ── CHECKLIST ─────────────────────────────────────────────────────────────────

function ChecklistTab() {
  const [done, setDone] = useState({})
  const completed = Object.values(done).filter(Boolean).length
  const pct = Math.round((completed / CHECKLIST_STEPS.length) * 100)
  const toggle = id => setDone(d => ({ ...d, [id]: !d[id] }))

  return (
    <div style={{ maxWidth: 600 }}>
      <div className="card" style={{ marginBottom: 16 }}>
        <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 8 }}>
          <span style={{ fontSize: 12, color: 'var(--text-secondary)' }}>Sweep Progress</span>
          <span className="mono" style={{ fontSize: 12, color: 'var(--tscm)' }}>{pct}%</span>
        </div>
        <div style={{ height: 6, background: 'var(--bg-elevated)', borderRadius: 3, overflow: 'hidden' }}>
          <div style={{ height: '100%', width: `${pct}%`, background: 'var(--tscm)', borderRadius: 3, transition: 'width 0.3s' }} />
        </div>
      </div>

      {CHECKLIST_STEPS.map((step, i) => (
        <div key={step.id} className="card" onClick={() => toggle(step.id)}
          style={{ marginBottom: 8, cursor: 'pointer', display: 'flex', alignItems: 'center', gap: 12,
            opacity: done[step.id] ? 0.6 : 1, borderLeft: `3px solid ${done[step.id] ? 'var(--tscm)' : 'var(--border)'}` }}>
          <div style={{ width: 20, height: 20, borderRadius: '50%',
            border: `2px solid ${done[step.id] ? 'var(--tscm)' : 'var(--border)'}`,
            background: done[step.id] ? 'var(--tscm)' : 'transparent',
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            flexShrink: 0, fontSize: 11, color: '#fff' }}>
            {done[step.id] ? '✓' : i + 1}
          </div>
          <span style={{ fontSize: 13, textDecoration: done[step.id] ? 'line-through' : 'none',
            color: step.id === 'drone' ? 'var(--drone)' : undefined }}>
            {step.label}
          </span>
        </div>
      ))}

      {pct === 100 && (
        <div className="card" style={{ marginTop: 16, borderColor: 'var(--tscm)', textAlign: 'center', color: 'var(--tscm)' }}>
          ✓ Sweep complete — proceed to REPORT to generate documentation
        </div>
      )}
    </div>
  )
}

// ── NEAR-FIELD ────────────────────────────────────────────────────────────────

function NearFieldTab() {
  const { frame } = useSpectrumStore()
  const [hotspots, setHotspots] = useState([])
  const [threshold, setThreshold] = useState(-75)

  // Find peak from current spectrum frame
  const peak = frame?.powers?.reduce((best, p, i) => p > best.power ? { power: p, freq: frame.freqs[i] } : best, { power: -Infinity, freq: 0 })
  const signalStrength = peak ? peak.power : null
  const barWidth = signalStrength !== null ? Math.max(0, Math.min(100, ((signalStrength + 110) / 60) * 100)) : 0
  const strengthColor = signalStrength > -60 ? 'var(--danger)' : signalStrength > -80 ? 'var(--warning)' : 'var(--success)'

  const logHotspot = () => {
    if (!peak?.freq) return
    setHotspots(h => [...h, {
      id: Date.now(),
      freq: peak.freq,
      power: peak.power,
      time: new Date().toLocaleTimeString(),
    }])
  }

  return (
    <div style={{ display: 'grid', gridTemplateColumns: '1fr 280px', gap: 16 }}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {/* Signal strength meter */}
        <div className="card" style={{ borderTop: `2px solid ${strengthColor}` }}>
          <div style={{ fontSize: 11, color: 'var(--text-muted)', letterSpacing: 0.5, marginBottom: 12 }}>SIGNAL STRENGTH METER</div>

          {/* Large dBm display */}
          <div style={{ textAlign: 'center', marginBottom: 16 }}>
            <div className="mono" style={{ fontSize: 48, fontWeight: 700, color: strengthColor, lineHeight: 1 }}>
              {signalStrength !== null ? signalStrength.toFixed(1) : '—'}
            </div>
            <div style={{ fontSize: 13, color: 'var(--text-secondary)', marginTop: 4 }}>dBFS peak</div>
            {peak?.freq > 0 && <div className="mono" style={{ fontSize: 14, color: 'var(--text-muted)', marginTop: 2 }}>{peak.freq?.toFixed(2)} MHz</div>}
          </div>

          {/* Bar meter */}
          <div style={{ marginBottom: 12 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 10, color: 'var(--text-muted)', marginBottom: 4 }}>
              <span>Weak (−110 dBFS)</span>
              <span>Strong (−50 dBFS)</span>
            </div>
            <div style={{ height: 20, background: 'var(--bg-elevated)', borderRadius: 4, overflow: 'hidden', position: 'relative' }}>
              <div style={{ height: '100%', width: `${barWidth}%`, background: `linear-gradient(to right, var(--success), var(--warning) 60%, var(--danger))`, borderRadius: 4, transition: 'width 0.1s' }} />
              {/* Threshold line */}
              <div style={{ position: 'absolute', top: 0, bottom: 0, left: `${Math.max(0, Math.min(100, ((threshold + 110) / 60) * 100))}%`, width: 2, background: 'rgba(255,255,255,0.5)' }} />
            </div>
          </div>

          {/* Threshold + log button */}
          <div style={{ display: 'flex', alignItems: 'center', gap: 12 }}>
            <label style={{ fontSize: 11, color: 'var(--text-muted)' }}>Alert threshold</label>
            <input type="range" min={-110} max={-40} value={threshold} onChange={e => setThreshold(+e.target.value)}
              style={{ flex: 1 }} />
            <span className="mono" style={{ fontSize: 11, color: 'var(--tscm)', minWidth: 50 }}>{threshold} dBFS</span>
            <button onClick={logHotspot} style={{ color: 'var(--tscm)', borderColor: 'var(--tscm)', fontSize: 11 }}>Log Hotspot</button>
          </div>

          {signalStrength > threshold && (
            <div style={{ marginTop: 10, padding: '8px 12px', background: 'rgba(251,148,0,0.1)', border: '1px solid var(--warning)', borderRadius: 4, fontSize: 12, color: 'var(--warning)' }}>
              ⚠ Signal above threshold — possible near-field emission source
            </div>
          )}
        </div>

        {/* Instructions */}
        <div className="card">
          <div style={{ fontSize: 11, color: 'var(--text-muted)', letterSpacing: 0.5, marginBottom: 8 }}>PROCEDURE</div>
          {['Hold antenna close to suspect surfaces (walls, furniture, power outlets)',
            'Walk slowly — watch the meter for increases above background',
            'Log hotspots when meter spikes above threshold',
            'Concentrate on areas near windows, vents, power strips',
            'Check behind mirrors, picture frames, smoke detectors'].map((s, i) => (
            <div key={i} style={{ fontSize: 12, color: 'var(--text-secondary)', padding: '4px 0', borderBottom: '1px solid var(--border)' }}>
              {i + 1}. {s}
            </div>
          ))}
        </div>
      </div>

      {/* Hotspot log */}
      <div className="card">
        <div style={{ fontSize: 11, color: 'var(--text-muted)', letterSpacing: 0.5, marginBottom: 8 }}>
          HOTSPOT LOG ({hotspots.length})
        </div>
        {hotspots.length === 0 && <div style={{ fontSize: 12, color: 'var(--text-muted)' }}>No hotspots logged yet</div>}
        {[...hotspots].reverse().map(h => (
          <div key={h.id} style={{ padding: '6px 0', borderBottom: '1px solid var(--border)' }}>
            <div className="mono" style={{ fontSize: 13, color: 'var(--tscm)' }}>{h.power?.toFixed(1)} dBFS</div>
            <div style={{ fontSize: 11, color: 'var(--text-secondary)' }}>{h.freq?.toFixed(2)} MHz</div>
            <div style={{ fontSize: 10, color: 'var(--text-muted)' }}>{h.time}</div>
          </div>
        ))}
        {hotspots.length > 0 && (
          <button onClick={() => setHotspots([])} style={{ marginTop: 8, fontSize: 11, color: 'var(--danger)' }}>Clear Log</button>
        )}
      </div>
    </div>
  )
}

// ── CELLULAR ──────────────────────────────────────────────────────────────────

function CellularTab() {
  const [anomalies, setAnomalies] = useState([])
  useEffect(() => {
    api.anomalies.list().then(all => {
      // Filter to cellular bands: 700–960 MHz, 1700–2100 MHz, 2500–2700 MHz
      const cellular = all.filter(a =>
        (a.freq_mhz >= 700 && a.freq_mhz <= 960) ||
        (a.freq_mhz >= 1700 && a.freq_mhz <= 2100) ||
        (a.freq_mhz >= 2500 && a.freq_mhz <= 2700)
      )
      setAnomalies(cellular)
    }).catch(() => {})
  }, [])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div className="card" style={{ borderLeft: `4px solid ${anomalies.length > 0 ? 'var(--warning)' : 'var(--success)'}` }}>
        <div style={{ fontSize: 10, color: 'var(--text-muted)', letterSpacing: 1 }}>CELLULAR BAND STATUS</div>
        <div className="mono" style={{ fontSize: 20, fontWeight: 700, color: anomalies.length > 0 ? 'var(--warning)' : 'var(--success)', marginTop: 4 }}>
          {anomalies.length > 0 ? `${anomalies.length} ANOMALIES` : 'NORMAL'}
        </div>
        <div style={{ fontSize: 12, color: 'var(--text-secondary)', marginTop: 4 }}>
          Monitoring 700–960 MHz, 1700–2100 MHz, 2500–2700 MHz cellular bands
        </div>
      </div>

      <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
        <div style={{ padding: '8px 12px', borderBottom: '1px solid var(--border)', fontSize: 11, color: 'var(--text-secondary)' }}>
          CELLULAR BAND ANOMALIES
        </div>
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 12 }}>
          <thead>
            <tr style={{ background: 'var(--bg-elevated)' }}>
              {['TIME', 'FREQ (MHz)', 'KIND', 'DELTA', 'Z-SCORE', 'SEV'].map(h => (
                <th key={h} style={{ padding: '7px 12px', textAlign: 'left', color: 'var(--text-muted)', fontWeight: 500, fontSize: 10, letterSpacing: 0.5 }}>{h}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {anomalies.length === 0 && (
              <tr><td colSpan={6} style={{ padding: 16, textAlign: 'center', color: 'var(--success)', fontSize: 12 }}>
                ✓ No cellular band anomalies — environment normal
              </td></tr>
            )}
            {anomalies.map(a => (
              <tr key={a.id} style={{ borderBottom: '1px solid var(--border)' }}>
                <td className="mono" style={{ padding: '6px 12px', fontSize: 11, color: 'var(--text-muted)' }}>{new Date(a.detected_at * 1000).toLocaleTimeString()}</td>
                <td className="mono" style={{ padding: '6px 12px', color: 'var(--tscm)' }}>{a.freq_mhz?.toFixed(2)}</td>
                <td style={{ padding: '6px 12px', color: 'var(--text-secondary)' }}>{a.kind}</td>
                <td className="mono" style={{ padding: '6px 12px' }}>{a.delta_db?.toFixed(1)} dB</td>
                <td className="mono" style={{ padding: '6px 12px', color: 'var(--text-secondary)' }}>{a.z_score?.toFixed(1)}</td>
                <td style={{ padding: '6px 12px' }}>
                  <span style={{ color: a.severity === 'CRITICAL' ? 'var(--danger)' : 'var(--warning)', fontSize: 11 }}>{a.severity}</span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      <div className="card" style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6 }}>
        <strong style={{ color: 'var(--text-secondary)' }}>IMSI Catcher Indicators:</strong> Anomalous signal strength in cellular bands, unexpected frequency occupancy, or signals appearing in bands not served by known carriers may indicate rogue base station (IMSI catcher / Stingray) activity. Capture a baseline first for accurate comparison.
      </div>
    </div>
  )
}

// ── HARMONICS ─────────────────────────────────────────────────────────────────

function HarmonicsTab() {
  const [groups, setGroups] = useState([])
  const load = () => api.harmonics.list().then(setGroups).catch(() => {})
  useEffect(() => { load(); const t = setInterval(load, 5000); return () => clearInterval(t) }, [])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {groups.length === 0 && (
        <div style={{ textAlign: 'center', padding: 48, color: 'var(--text-muted)', fontSize: 13 }}>
          No harmonic relationships detected yet — harmonic analysis runs every 10s on detected signal peaks
        </div>
      )}

      {groups.map(g => (
        <div key={g.id} className="card" style={{ borderLeft: '3px solid var(--tscm)' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
            <div>
              <div style={{ fontWeight: 600, color: 'var(--tscm)', fontSize: 14, marginBottom: 4 }}>
                f₀ = {g.fundamental_mhz?.toFixed(3)} MHz
              </div>
              {g.source_hypothesis && (
                <div style={{ fontSize: 12, color: 'var(--warning)', marginBottom: 6 }}>
                  ⚠ Likely source: {g.source_hypothesis}
                </div>
              )}
              {/* Harmonic chain visualization */}
              <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap', marginTop: 6 }}>
                <HarmonicPill freq={g.fundamental_mhz} label="f₀" />
                {g.harmonics.map((h, i) => {
                  const n = Math.round(h / g.fundamental_mhz)
                  return <HarmonicPill key={i} freq={h} label={`${n}f`} />
                })}
              </div>
            </div>
            <span className="mono" style={{ fontSize: 11, color: 'var(--text-muted)' }}>
              {new Date(g.detected_at * 1000).toLocaleTimeString()}
            </span>
          </div>
          {!g.source_hypothesis && (
            <div style={{ marginTop: 8, fontSize: 11, color: 'var(--text-muted)' }}>
              Unknown source — could be switching power supply, oscillator, or hidden electronic device
            </div>
          )}
        </div>
      ))}

      <div className="card" style={{ fontSize: 12, color: 'var(--text-muted)', lineHeight: 1.6 }}>
        <strong style={{ color: 'var(--text-secondary)' }}>Harmonic Analysis:</strong> Hidden electronics emit at their clock frequency and integer multiples (harmonics). A device at 12 MHz will produce emissions at 24, 36, 48 MHz, etc. Unexpected harmonic groups may indicate concealed transmitters, recording devices, or TEMPEST-vulnerable equipment.
      </div>
    </div>
  )
}

function HarmonicPill({ freq, label }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', padding: '4px 10px',
      background: 'var(--bg-elevated)', border: '1px solid var(--tscm)', borderRadius: 4 }}>
      <span style={{ fontSize: 10, color: 'var(--tscm)', fontWeight: 600 }}>{label}</span>
      <span className="mono" style={{ fontSize: 11, color: 'var(--text-secondary)' }}>{freq?.toFixed(2)}</span>
    </div>
  )
}
