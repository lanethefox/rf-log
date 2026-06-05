import { useState, useEffect, useRef } from 'react'
import { useSpectrumStore } from '../stores/useSpectrumStore'
import SpectrumCanvas from '../canvas/SpectrumCanvas'
import TabBar from '../components/TabBar'
import Placeholder from '../components/Placeholder'
import { api } from '../api/client'

const TABS = ['SPECTRUM', 'BASELINE', 'SWEEP', 'COMPARE']

export default function Survey() {
  const [tab, setTab] = useState('SPECTRUM')
  const { frame } = useSpectrumStore()

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <TabBar tabs={TABS} active={tab} color="var(--survey)" onChange={setTab} />
      <div style={{ flex: 1, overflow: 'auto', padding: 16 }}>
        {tab === 'SPECTRUM' && <SpectrumTab frame={frame} />}
        {tab === 'BASELINE' && <BaselineTab />}
        {tab === 'SWEEP'    && <Placeholder title="Sweep Protocol" subtitle="Configure automated frequency sweeps with dwell, step, and range controls" color="var(--survey)" />}
        {tab === 'COMPARE'  && <CompareTab />}
      </div>
    </div>
  )
}

function SpectrumTab({ frame }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
        <div style={{ padding: '8px 12px', borderBottom: '1px solid var(--border)', fontSize: 11, color: 'var(--text-secondary)' }}>
          LIVE SPECTRUM — {frame?.band || 'No signal'}
        </div>
        <SpectrumCanvas frame={frame} height={220} />
      </div>
      <div className="card">
        <div style={{ fontSize: 11, color: 'var(--text-muted)' }}>WATERFALL — coming in P1.2 refinement</div>
      </div>
    </div>
  )
}

function BaselineTab() {
  const [baselines, setBaselines] = useState([])
  const [status, setStatus] = useState({ active: false })
  const [form, setForm] = useState({ name: '', location: '' })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState(null)
  const pollRef = useRef(null)

  const loadBaselines = () => api.baselines.list().then(setBaselines).catch(() => {})

  useEffect(() => {
    loadBaselines()
    // Poll capture status
    pollRef.current = setInterval(() => {
      api.baselines.captureStatus().then(setStatus).catch(() => {})
    }, 1000)
    return () => clearInterval(pollRef.current)
  }, [])

  const handleStart = async () => {
    if (!form.name.trim()) { setError('Name is required'); return }
    setError(null)
    setBusy(true)
    try {
      await api.baselines.captureStart({
        name: form.name.trim(),
        location: form.location.trim() || null,
      })
      setStatus({ active: true, name: form.name, started_at: Date.now() / 1000, sample_count: 0 })
    } catch (e) { setError(e.message) }
    finally { setBusy(false) }
  }

  const handleStop = async () => {
    setBusy(true)
    try {
      await api.baselines.captureStop()
      setStatus({ active: false })
      setForm({ name: '', location: '' })
      await loadBaselines()
    } catch (e) { setError(e.message) }
    finally { setBusy(false) }
  }

  const handleActivate = async (id) => {
    try {
      await api.baselines.activate(id)
      // Mark locally
      setBaselines(bs => bs.map(b => ({ ...b, _active: b.id === id })))
    } catch {}
  }

  return (
    <div style={{ display: 'grid', gridTemplateColumns: '320px 1fr', gap: 16 }}>
      {/* Capture controls */}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        <div className="card" style={{ borderTop: `2px solid ${status.active ? 'var(--danger)' : 'var(--survey)'}` }}>
          <div style={{ fontSize: 11, color: 'var(--text-muted)', letterSpacing: 0.5, marginBottom: 10 }}>
            {status.active ? 'CAPTURING…' : 'CAPTURE BASELINE'}
          </div>

          {status.active ? (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              <div style={{ fontSize: 13, color: 'var(--survey)', fontWeight: 600 }}>{status.name}</div>
              <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12, color: 'var(--text-secondary)' }}>
                <span>Samples</span>
                <span className="mono" style={{ color: 'var(--survey)' }}>{status.sample_count?.toLocaleString()}</span>
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between', fontSize: 12, color: 'var(--text-secondary)' }}>
                <span>Elapsed</span>
                <span className="mono" style={{ color: 'var(--survey)' }}>{status.elapsed_secs}s</span>
              </div>
              {/* Progress animation */}
              <div style={{ height: 4, background: 'var(--bg-elevated)', borderRadius: 2, overflow: 'hidden', marginTop: 4 }}>
                <div style={{
                  height: '100%', width: '100%',
                  background: 'var(--survey)',
                  animation: 'pulse-bar 1.5s ease-in-out infinite',
                }} />
              </div>
              <button onClick={handleStop} disabled={busy}
                style={{ color: 'var(--survey)', borderColor: 'var(--survey)', marginTop: 4 }}>
                {busy ? 'Finalizing…' : 'Stop & Save Baseline'}
              </button>
            </div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              <input
                placeholder="Baseline name *"
                value={form.name}
                onChange={e => setForm(f => ({ ...f, name: e.target.value }))}
                style={inputStyle}
              />
              <input
                placeholder="Location (optional)"
                value={form.location}
                onChange={e => setForm(f => ({ ...f, location: e.target.value }))}
                style={inputStyle}
              />
              {error && <div style={{ fontSize: 11, color: 'var(--danger)' }}>{error}</div>}
              <button onClick={handleStart} disabled={busy}
                style={{ color: 'var(--survey)', borderColor: 'var(--survey)', marginTop: 4 }}>
                {busy ? 'Starting…' : 'Start Capture'}
              </button>
              <div style={{ fontSize: 11, color: 'var(--text-muted)', lineHeight: 1.4 }}>
                Accumulates spectrum statistics over time. Stop capture when environment is stable and clean.
              </div>
            </div>
          )}
        </div>
      </div>

      {/* Baseline library */}
      <div>
        <div style={{ fontSize: 11, color: 'var(--text-muted)', letterSpacing: 0.5, marginBottom: 8 }}>
          SAVED BASELINES ({baselines.length})
        </div>
        {baselines.length === 0 && (
          <div style={{ fontSize: 13, color: 'var(--text-muted)', textAlign: 'center', padding: 32 }}>
            No baselines captured yet
          </div>
        )}
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          {baselines.map(b => (
            <div key={b.id} className="card"
              style={{ borderLeft: `3px solid ${b._active ? 'var(--survey)' : 'var(--border)'}` }}>
              <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
                <div>
                  <div style={{ fontWeight: 600, color: b._active ? 'var(--survey)' : 'var(--text-primary)', fontSize: 13 }}>
                    {b.name}
                    {b._active && <span style={{ marginLeft: 8, fontSize: 10, color: 'var(--survey)' }}>● ACTIVE</span>}
                  </div>
                  {b.location && <div style={{ fontSize: 12, color: 'var(--text-secondary)', marginTop: 2 }}>{b.location}</div>}
                </div>
                <span className="mono" style={{ fontSize: 11, color: 'var(--text-muted)' }}>
                  {new Date(b.captured_at * 1000).toLocaleDateString()}
                </span>
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 6, fontSize: 11, color: 'var(--text-muted)' }}>
                <span className="mono">{b.freq_start_mhz?.toFixed(1)}–{b.freq_end_mhz?.toFixed(1)} MHz · {b.bin_count} bins</span>
                <button onClick={() => handleActivate(b.id)}
                  style={{ fontSize: 10, color: 'var(--survey)', padding: '2px 8px' }}>
                  Use for Detection
                </button>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

function CompareTab() {
  const [baselines, setBaselines] = useState([])
  const [selA, setSelA] = useState('')
  const [selB, setSelB] = useState('')
  const [binsA, setBinsA] = useState(null)
  const [binsB, setBinsB] = useState(null)
  const [loading, setLoading] = useState(false)
  const canvasRef = useRef(null)

  useEffect(() => {
    api.baselines.list().then(setBaselines).catch(() => {})
  }, [])

  const handleCompare = async () => {
    if (!selA || !selB) return
    setLoading(true)
    try {
      const [a, b] = await Promise.all([
        api.baselines.bins(selA),
        api.baselines.bins(selB),
      ])
      setBinsA(a)
      setBinsB(b)
    } catch (e) { console.error(e) }
    finally { setLoading(false) }
  }

  // Draw comparison chart when bins are available
  useEffect(() => {
    if (!binsA || !binsB || !canvasRef.current) return
    const canvas = canvasRef.current
    const ctx = canvas.getContext('2d')
    const W = canvas.width = canvas.offsetWidth
    const H = canvas.height = 180

    ctx.clearRect(0, 0, W, H)
    ctx.fillStyle = '#0d1117'
    ctx.fillRect(0, 0, W, H)

    const len = Math.min(binsA.length, binsB.length, W)
    const step = W / len

    // Power range
    const allPowers = [...binsA.map(b => b.mean), ...binsB.map(b => b.mean)]
    const minP = Math.min(...allPowers) - 5
    const maxP = Math.max(...allPowers) + 5
    const scaleY = p => H - ((p - minP) / (maxP - minP)) * H

    // Draw baseline A (blue)
    ctx.beginPath()
    ctx.strokeStyle = 'rgba(56,139,253,0.8)'
    ctx.lineWidth = 1.5
    for (let i = 0; i < len; i++) {
      const x = i * step
      const y = scaleY(binsA[i].mean)
      i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)
    }
    ctx.stroke()

    // Draw baseline B (amber)
    ctx.beginPath()
    ctx.strokeStyle = 'rgba(251,148,0,0.8)'
    ctx.lineWidth = 1.5
    for (let i = 0; i < len; i++) {
      const x = i * step
      const y = scaleY(binsB[i].mean)
      i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)
    }
    ctx.stroke()
  }, [binsA, binsB])

  // Compute diff table (bins where delta > 3 dB)
  const diffRows = binsA && binsB
    ? binsA.slice(0, Math.min(binsA.length, binsB.length))
        .map((a, i) => ({ freq: a.freq_mhz, delta: binsB[i].mean - a.mean, meanA: a.mean, meanB: binsB[i].mean }))
        .filter(r => Math.abs(r.delta) >= 3.0)
        .sort((a, b) => Math.abs(b.delta) - Math.abs(a.delta))
        .slice(0, 20)
    : []

  const nameA = baselines.find(b => b.id === +selA)?.name || 'Baseline A'
  const nameB = baselines.find(b => b.id === +selB)?.name || 'Baseline B'

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {/* Selector row */}
      <div className="card" style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
        <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>COMPARE</span>
        <select value={selA} onChange={e => setSelA(e.target.value)} style={selectStyle}>
          <option value="">— Select baseline A —</option>
          {baselines.map(b => <option key={b.id} value={b.id}>{b.name} ({new Date(b.captured_at * 1000).toLocaleDateString()})</option>)}
        </select>
        <span style={{ color: 'var(--text-muted)', fontSize: 12 }}>vs</span>
        <select value={selB} onChange={e => setSelB(e.target.value)} style={selectStyle}>
          <option value="">— Select baseline B —</option>
          {baselines.map(b => <option key={b.id} value={b.id}>{b.name} ({new Date(b.captured_at * 1000).toLocaleDateString()})</option>)}
        </select>
        <button onClick={handleCompare} disabled={!selA || !selB || loading}
          style={{ color: 'var(--survey)', borderColor: 'var(--survey)' }}>
          {loading ? 'Loading…' : 'Compare'}
        </button>
      </div>

      {/* Chart */}
      {binsA && binsB && (
        <>
          <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
            <div style={{ padding: '8px 12px', borderBottom: '1px solid var(--border)', fontSize: 11, color: 'var(--text-secondary)', display: 'flex', gap: 16 }}>
              <span>MEAN POWER OVERLAY</span>
              <span style={{ color: 'var(--survey)' }}>— {nameA}</span>
              <span style={{ color: 'var(--hunt)' }}>— {nameB}</span>
            </div>
            <canvas ref={canvasRef} style={{ width: '100%', height: 180, display: 'block' }} />
          </div>

          {/* Diff table */}
          <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
            <div style={{ padding: '8px 12px', borderBottom: '1px solid var(--border)', fontSize: 11, color: 'var(--text-secondary)' }}>
              SIGNIFICANT CHANGES (≥3 dB, top 20)
            </div>
            {diffRows.length === 0 ? (
              <div style={{ padding: 16, textAlign: 'center', color: 'var(--success)', fontSize: 12 }}>
                ✓ No significant differences — environments match
              </div>
            ) : (
              <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 12 }}>
                <thead>
                  <tr style={{ background: 'var(--bg-elevated)' }}>
                    {['FREQ (MHz)', nameA + ' (dBFS)', nameB + ' (dBFS)', 'DELTA'].map(h => (
                      <th key={h} style={{ padding: '6px 12px', textAlign: 'left', color: 'var(--text-muted)', fontWeight: 500, fontSize: 10 }}>{h}</th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {diffRows.map((r, i) => (
                    <tr key={i} style={{ borderBottom: '1px solid var(--border)' }}>
                      <td className="mono" style={{ padding: '6px 12px' }}>{r.freq.toFixed(2)}</td>
                      <td className="mono" style={{ padding: '6px 12px', color: 'var(--text-secondary)' }}>{r.meanA.toFixed(1)}</td>
                      <td className="mono" style={{ padding: '6px 12px', color: 'var(--text-secondary)' }}>{r.meanB.toFixed(1)}</td>
                      <td className="mono" style={{ padding: '6px 12px', color: r.delta > 0 ? 'var(--danger)' : 'var(--text-muted)', fontWeight: 600 }}>
                        {r.delta > 0 ? '+' : ''}{r.delta.toFixed(1)} dB
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </>
      )}

      {!binsA && (
        <div style={{ textAlign: 'center', padding: 48, color: 'var(--text-muted)', fontSize: 13 }}>
          Select two baselines and click Compare to see power differences
        </div>
      )}
    </div>
  )
}

const inputStyle = {
  width: '100%',
  boxSizing: 'border-box',
  background: 'var(--bg-elevated)',
  border: '1px solid var(--border)',
  borderRadius: 4,
  color: 'var(--text-primary)',
  padding: '7px 10px',
  fontSize: 12,
  fontFamily: 'inherit',
}

const selectStyle = {
  background: 'var(--bg-elevated)',
  border: '1px solid var(--border)',
  borderRadius: 4,
  color: 'var(--text-primary)',
  padding: '6px 10px',
  fontSize: 12,
  flex: 1,
  minWidth: 200,
}
