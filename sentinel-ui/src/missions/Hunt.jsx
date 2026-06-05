import { useEffect, useRef, useState } from 'react'
import { api } from '../api/client'
import { useEventStore } from '../stores/useEventStore'
import { useSpectrumStore } from '../stores/useSpectrumStore'
import SpectrumCanvas from '../canvas/SpectrumCanvas'
import TabBar from '../components/TabBar'
import Placeholder from '../components/Placeholder'

const TABS = ['LIVE', 'EMITTERS', 'PULSES', 'FINGERPRINT']

export default function Hunt() {
  const [tab, setTab] = useState('LIVE')
  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <TabBar tabs={TABS} active={tab} color="var(--hunt)" onChange={setTab} />
      <div style={{ flex: 1, overflow: 'auto', padding: 16 }}>
        {tab === 'LIVE'        && <LiveTab />}
        {tab === 'EMITTERS'    && <EmittersTab />}
        {tab === 'PULSES'      && <PulsesTab />}
        {tab === 'FINGERPRINT' && <FingerprintTab />}
      </div>
    </div>
  )
}

// ── LIVE ──────────────────────────────────────────────────────────────────────

function LiveTab() {
  const { frame } = useSpectrumStore()
  const { events } = useEventStore()
  const anomalies = events.filter(e => e.type === 'anomaly').slice(0, 20)
  return (
    <div style={{ display: 'grid', gridTemplateColumns: '1fr 280px', gap: 12, height: '100%' }}>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
          <SpectrumCanvas frame={frame} height={220} />
        </div>
      </div>
      <div className="card" style={{ overflowY: 'auto' }}>
        <div style={{ fontSize: 10, color: 'var(--text-muted)', letterSpacing: 1, marginBottom: 8 }}>ANOMALY FEED</div>
        {anomalies.length === 0 && <div style={{ fontSize: 12, color: 'var(--success)' }}>✓ No anomalies</div>}
        {anomalies.map((evt, i) => (
          <div key={i} style={{ padding: '6px 0', borderBottom: '1px solid var(--border)', fontSize: 12 }}>
            <div style={{ color: 'var(--warning)' }}>⚠ {evt.freq_mhz?.toFixed(2)} MHz</div>
            <div style={{ color: 'var(--text-secondary)', fontSize: 11 }}>{evt.kind} · z={evt.z_score?.toFixed(1)}</div>
          </div>
        ))}
      </div>
    </div>
  )
}

// ── EMITTERS ──────────────────────────────────────────────────────────────────

const STATUS_COLORS = { KNOWN: 'var(--success)', UNKNOWN: 'var(--warning)', NEW: 'var(--danger)', GONE: 'var(--text-muted)' }

function EmittersTab() {
  const [emitters, setEmitters] = useState([])
  const [selected, setSelected] = useState(null)
  const [editNotes, setEditNotes] = useState('')

  const load = () => api.emitters.list().then(setEmitters).catch(() => {})
  useEffect(() => { load(); const t = setInterval(load, 5000); return () => clearInterval(t) }, [])

  const markKnown = async (id) => {
    await api.emitters.update(id, { status: 'KNOWN' }).catch(() => {})
    load()
  }
  const saveNotes = async (id) => {
    await api.emitters.update(id, { notes: editNotes }).catch(() => {})
    setSelected(null)
    load()
  }

  return (
    <div style={{ display: 'grid', gridTemplateColumns: selected ? '1fr 300px' : '1fr', gap: 12 }}>
      <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 12 }}>
          <thead>
            <tr style={{ background: 'var(--bg-elevated)' }}>
              {['FREQ (MHz)', 'TYPE', 'MATCH', 'CONF', 'STATUS', 'LAST SEEN', ''].map(h => (
                <th key={h} style={{ padding: '8px 12px', textAlign: 'left', color: 'var(--text-muted)', fontWeight: 500, fontSize: 10, letterSpacing: 0.5 }}>{h}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {emitters.length === 0 && (
              <tr><td colSpan={7} style={{ padding: 16, color: 'var(--text-muted)', textAlign: 'center' }}>No emitters cataloged yet — waiting for spectrum data</td></tr>
            )}
            {emitters.map(e => (
              <tr key={e.id}
                style={{ borderBottom: '1px solid var(--border)', background: selected?.id === e.id ? 'var(--bg-elevated)' : '' }}
                onClick={() => { setSelected(e); setEditNotes(e.notes || '') }}
                onMouseEnter={ev => ev.currentTarget.style.background = 'var(--bg-elevated)'}
                onMouseLeave={ev => ev.currentTarget.style.background = selected?.id === e.id ? 'var(--bg-elevated)' : ''}>
                <td className="mono" style={{ padding: '7px 12px' }}>{e.freq_mhz?.toFixed(4)}</td>
                <td style={{ padding: '7px 12px', color: 'var(--text-secondary)' }}>{e.emitter_type || '—'}</td>
                <td style={{ padding: '7px 12px', maxWidth: 160, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{e.id_match || '—'}</td>
                <td className="mono" style={{ padding: '7px 12px', color: 'var(--text-secondary)' }}>{e.confidence > 0 ? `${(e.confidence * 100).toFixed(0)}%` : '—'}</td>
                <td style={{ padding: '7px 12px' }}>
                  <span style={{ color: STATUS_COLORS[e.status] || 'var(--text-secondary)', fontSize: 11 }}>{e.status}</span>
                </td>
                <td className="mono" style={{ padding: '7px 12px', color: 'var(--text-muted)', fontSize: 11 }}>{new Date(e.last_seen * 1000).toLocaleTimeString()}</td>
                <td style={{ padding: '7px 8px' }}>
                  {e.status !== 'KNOWN' && (
                    <button onClick={ev => { ev.stopPropagation(); markKnown(e.id) }}
                      style={{ fontSize: 10, padding: '2px 6px', color: 'var(--success)' }}>✓ Known</button>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Detail panel */}
      {selected && (
        <div className="card" style={{ borderTop: '2px solid var(--hunt)' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 12 }}>
            <span style={{ fontSize: 11, color: 'var(--text-muted)', letterSpacing: 0.5 }}>EMITTER DETAIL</span>
            <button onClick={() => setSelected(null)} style={{ fontSize: 11, padding: '2px 6px' }}>✕</button>
          </div>
          <div className="mono" style={{ fontSize: 18, color: 'var(--hunt)', marginBottom: 8 }}>
            {selected.freq_mhz?.toFixed(4)} MHz
          </div>
          <DetailRow label="Status" value={<span style={{ color: STATUS_COLORS[selected.status] }}>{selected.status}</span>} />
          <DetailRow label="Type" value={selected.emitter_type || '—'} />
          <DetailRow label="ID Match" value={selected.id_match || '—'} />
          <DetailRow label="Confidence" value={selected.confidence > 0 ? `${(selected.confidence * 100).toFixed(0)}%` : '—'} />
          <DetailRow label="First Seen" value={new Date(selected.first_seen * 1000).toLocaleString()} />
          <DetailRow label="Last Seen" value={new Date(selected.last_seen * 1000).toLocaleString()} />
          {selected.fingerprint_json && <FingerprintMini fp={JSON.parse(selected.fingerprint_json)} />}
          <div style={{ marginTop: 12 }}>
            <div style={{ fontSize: 11, color: 'var(--text-muted)', marginBottom: 4 }}>NOTES</div>
            <textarea value={editNotes} onChange={e => setEditNotes(e.target.value)} rows={3}
              style={{ width: '100%', boxSizing: 'border-box', background: 'var(--bg-elevated)', border: '1px solid var(--border)', borderRadius: 4, color: 'var(--text-primary)', padding: '6px 8px', fontSize: 11, resize: 'vertical', fontFamily: 'inherit' }} />
            <button onClick={() => saveNotes(selected.id)} style={{ marginTop: 4, color: 'var(--hunt)', fontSize: 11 }}>Save Notes</button>
          </div>
        </div>
      )}
    </div>
  )
}

function DetailRow({ label, value }) {
  return (
    <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 6, fontSize: 12 }}>
      <span style={{ color: 'var(--text-muted)' }}>{label}</span>
      <span style={{ color: 'var(--text-secondary)' }}>{value}</span>
    </div>
  )
}

function FingerprintMini({ fp }) {
  return (
    <div style={{ marginTop: 8, padding: '8px 10px', background: 'var(--bg-elevated)', borderRadius: 4 }}>
      <div style={{ fontSize: 10, color: 'var(--hunt)', letterSpacing: 0.5, marginBottom: 6 }}>RF FINGERPRINT</div>
      <DetailRow label="CFO" value={`${fp.cfo_hz?.toFixed(1)} Hz`} />
      <DetailRow label="I/Q Imbalance" value={`${fp.iq_imbalance_db?.toFixed(2)} dB`} />
      <DetailRow label="RMS Amplitude" value={fp.rms_amplitude?.toFixed(4)} />
    </div>
  )
}

// ── PULSES ────────────────────────────────────────────────────────────────────

function PulsesTab() {
  const [pdws, setPdws] = useState([])
  const [priStats, setPriStats] = useState([])

  const load = () => {
    api.pdw.list().then(setPdws).catch(() => {})
    api.pdw.priStats().then(setPriStats).catch(() => {})
  }
  useEffect(() => { load(); const t = setInterval(load, 3000); return () => clearInterval(t) }, [])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {/* PRI summary cards */}
      {priStats.length > 0 && (
        <div style={{ display: 'flex', gap: 10, flexWrap: 'wrap' }}>
          {priStats.map((s, i) => (
            <div key={i} className="card" style={{ borderTop: '2px solid var(--hunt)', minWidth: 180 }}>
              <div className="mono" style={{ color: 'var(--hunt)', fontSize: 14, fontWeight: 600 }}>{s.freq_mhz?.toFixed(1)} MHz</div>
              <div style={{ fontSize: 11, color: 'var(--text-secondary)', marginTop: 4 }}>
                PRI: {s.mean_pri_us?.toFixed(0)} µs avg · <span style={{ color: patternColor(s.pattern) }}>{s.pattern}</span>
              </div>
              <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 2 }}>
                {s.count} pulses · {s.min_pri_us?.toFixed(0)}–{s.max_pri_us?.toFixed(0)} µs range
              </div>
            </div>
          ))}
        </div>
      )}

      {/* PDW table */}
      <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
        <div style={{ padding: '8px 12px', borderBottom: '1px solid var(--border)', display: 'flex', justifyContent: 'space-between', fontSize: 11, color: 'var(--text-secondary)' }}>
          <span>PULSE DESCRIPTOR WORDS</span>
          <span className="mono" style={{ color: 'var(--text-muted)' }}>{pdws.length} pulses</span>
        </div>
        <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 12 }}>
          <thead>
            <tr style={{ background: 'var(--bg-elevated)' }}>
              {['TOA (s)', 'FREQ (MHz)', 'PW (µs)', 'PRI (µs)', 'AMP (dBFS)'].map(h => (
                <th key={h} style={{ padding: '7px 12px', textAlign: 'left', color: 'var(--text-muted)', fontWeight: 500, fontSize: 10, letterSpacing: 0.5 }}>{h}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {pdws.length === 0 && (
              <tr><td colSpan={5} style={{ padding: 16, color: 'var(--text-muted)', textAlign: 'center' }}>No pulses detected yet</td></tr>
            )}
            {pdws.slice(0, 100).map(p => (
              <tr key={p.id} style={{ borderBottom: '1px solid var(--border)' }}
                onMouseEnter={e => e.currentTarget.style.background = 'var(--bg-elevated)'}
                onMouseLeave={e => e.currentTarget.style.background = ''}>
                <td className="mono" style={{ padding: '6px 12px', color: 'var(--text-muted)', fontSize: 11 }}>{p.toa?.toFixed(3)}</td>
                <td className="mono" style={{ padding: '6px 12px', color: 'var(--hunt)' }}>{p.freq_mhz?.toFixed(1)}</td>
                <td className="mono" style={{ padding: '6px 12px' }}>{p.pw_us?.toFixed(2)}</td>
                <td className="mono" style={{ padding: '6px 12px', color: 'var(--text-secondary)' }}>{p.pri_us ? p.pri_us.toFixed(0) : '—'}</td>
                <td className="mono" style={{ padding: '6px 12px', color: 'var(--text-secondary)' }}>{p.amplitude_dbfs?.toFixed(1)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function patternColor(p) {
  return p === 'Stable' ? 'var(--success)' : p === 'Stagger' ? 'var(--warning)' : 'var(--danger)'
}

// ── FINGERPRINT ───────────────────────────────────────────────────────────────

function FingerprintTab() {
  const [emitters, setEmitters] = useState([])
  const [selId, setSelId] = useState('')
  const [fp, setFp] = useState(null)

  useEffect(() => {
    api.emitters.list().then(list => {
      const withFp = list.filter(e => e.fingerprint_json)
      setEmitters(withFp)
    }).catch(() => {})
  }, [])

  const handleSelect = (id) => {
    setSelId(id)
    const e = emitters.find(e => e.id === +id)
    if (e?.fingerprint_json) {
      try { setFp(JSON.parse(e.fingerprint_json)) } catch { setFp(null) }
    }
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {/* Emitter selector */}
      <div className="card" style={{ display: 'flex', gap: 12, alignItems: 'center' }}>
        <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>SELECT EMITTER</span>
        <select value={selId} onChange={e => handleSelect(e.target.value)}
          style={{ background: 'var(--bg-elevated)', border: '1px solid var(--border)', borderRadius: 4, color: 'var(--text-primary)', padding: '6px 10px', fontSize: 12, flex: 1 }}>
          <option value="">— Choose an emitter with fingerprint data —</option>
          {emitters.map(e => (
            <option key={e.id} value={e.id}>{e.freq_mhz?.toFixed(4)} MHz — {e.status} {e.id_match ? `(${e.id_match})` : ''}</option>
          ))}
        </select>
      </div>

      {emitters.length === 0 && (
        <div style={{ textAlign: 'center', padding: 48, color: 'var(--text-muted)', fontSize: 13 }}>
          Fingerprints are extracted automatically for cataloged emitters. Check back after signals are detected.
        </div>
      )}

      {fp && (
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
          {/* Feature cards */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            <FpCard label="Carrier Frequency Offset" value={`${fp.cfo_hz?.toFixed(2)} Hz`}
              desc="Deviation from nominal carrier. Unique per transmitter hardware." accent="var(--hunt)" />
            <FpCard label="I/Q Power Imbalance" value={`${fp.iq_imbalance_db?.toFixed(3)} dB`}
              desc="Power asymmetry between I and Q channels. Hardware-specific defect." accent="var(--survey)" />
            <FpCard label="RMS Amplitude" value={fp.rms_amplitude?.toFixed(5)}
              desc="Root mean square signal amplitude. Relates to transmit power and path loss." accent="var(--tscm)" />
          </div>

          {/* Visual fingerprint bars */}
          <div className="card" style={{ borderTop: '2px solid var(--hunt)' }}>
            <div style={{ fontSize: 11, color: 'var(--text-muted)', letterSpacing: 0.5, marginBottom: 12 }}>FEATURE PROFILE</div>
            <FpBar label="CFO (normalized)" value={Math.min(1, Math.abs(fp.cfo_hz || 0) / 500)} color="var(--hunt)" />
            <FpBar label="I/Q Imbalance" value={Math.min(1, Math.abs(fp.iq_imbalance_db || 0) / 3)} color="var(--survey)" />
            <FpBar label="RMS Amplitude" value={Math.min(1, (fp.rms_amplitude || 0))} color="var(--tscm)" />
            <div style={{ marginTop: 16, fontSize: 11, color: 'var(--text-muted)', lineHeight: 1.5 }}>
              These features are extracted from IQ samples of the captured signal.
              Each transmitter has a unique hardware fingerprint due to manufacturing tolerances.
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

function FpCard({ label, value, desc, accent }) {
  return (
    <div className="card" style={{ borderLeft: `3px solid ${accent}` }}>
      <div style={{ fontSize: 10, color: 'var(--text-muted)', letterSpacing: 0.5, marginBottom: 4 }}>{label.toUpperCase()}</div>
      <div className="mono" style={{ fontSize: 20, color: accent, fontWeight: 700, marginBottom: 4 }}>{value}</div>
      <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>{desc}</div>
    </div>
  )
}

function FpBar({ label, value, color }) {
  return (
    <div style={{ marginBottom: 10 }}>
      <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 11, marginBottom: 4 }}>
        <span style={{ color: 'var(--text-secondary)' }}>{label}</span>
        <span className="mono" style={{ color }}>{(value * 100).toFixed(1)}%</span>
      </div>
      <div style={{ height: 6, background: 'var(--bg-base)', borderRadius: 3, overflow: 'hidden' }}>
        <div style={{ height: '100%', width: `${value * 100}%`, background: color, borderRadius: 3, transition: 'width 0.5s' }} />
      </div>
    </div>
  )
}
