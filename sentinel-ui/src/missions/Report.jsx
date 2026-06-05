import { useEffect, useState } from 'react'
import { api } from '../api/client'
import TabBar from '../components/TabBar'
import Placeholder from '../components/Placeholder'

const TABS = ['BUILDER', 'TEMPLATES', 'HISTORY', 'SHARE']

const TEMPLATES = [
  { id: 'tscm',     name: 'TSCM Site Survey Report',          desc: 'Full sweep findings, anomalies, and recommendations' },
  { id: 'rf-env',   name: 'RF Environment Assessment',         desc: 'Baseline comparison and spectral overview' },
  { id: 'surv',     name: 'Surveillance Equipment Detection',  desc: 'Suspected covert device findings with evidence' },
  { id: 'eob',      name: 'Emitter Catalog / EOB',            desc: 'All detected emitters with parameters and classification' },
  { id: 'drone',    name: 'Drone Activity & Incursion Report', desc: 'Detected drones, methods, duration, and compliance status' },
  { id: 'incident', name: 'Incident Report',                   desc: 'Focused analysis of a specific anomaly or event' },
]

export default function Report() {
  const [tab, setTab] = useState('BUILDER')
  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <TabBar tabs={TABS} active={tab} color="var(--report)" onChange={setTab} />
      <div style={{ flex: 1, overflow: 'auto', padding: 16 }}>
        {tab === 'BUILDER'   && <BuilderTab />}
        {tab === 'TEMPLATES' && <TemplatesTab />}
        {tab === 'HISTORY'   && <HistoryTab />}
        {tab === 'SHARE'     && <Placeholder title="Share Report" subtitle="Generate shareable links for local network access (Phase 2)" color="var(--report)" />}
      </div>
    </div>
  )
}

function BuilderTab() {
  const [sections, setSections] = useState([
    { id: 'summary',    label: 'Executive Summary',     enabled: true },
    { id: 'emitters',   label: 'Emitter Table',          enabled: true },
    { id: 'baseline',   label: 'Baseline Comparison',    enabled: false },
    { id: 'anomalies',  label: 'Anomaly Log',            enabled: true },
    { id: 'spectrum',   label: 'Spectrum Captures',      enabled: false },
    { id: 'fingerprint',label: 'Fingerprint Analysis',   enabled: false },
    { id: 'drones',     label: 'Drone Activity',         enabled: true },
    { id: 'recommend',  label: 'Recommendations',        enabled: true },
  ])
  const [notes, setNotes] = useState('')
  const [exporting, setExporting] = useState(false)

  const toggle = id => setSections(s => s.map(x => x.id === id ? { ...x, enabled: !x.enabled } : x))

  const handleExport = async fmt => {
    setExporting(true)
    try {
      const res = await api.reports.create({
        template: 'custom',
        title: 'RF Environment Report',
        location: '',
        sections: sections.filter(s => s.enabled).map(s => s.id),
        notes,
        format: fmt,
      })
      if (fmt === 'json') {
        const blob = new Blob([JSON.stringify(res, null, 2)], { type: 'application/json' })
        const url = URL.createObjectURL(blob)
        Object.assign(document.createElement('a'), { href: url, download: `rf-sentinel-report-${Date.now()}.json` }).click()
        URL.revokeObjectURL(url)
      }
    } catch (e) {
      console.error(e)
    } finally {
      setExporting(false)
    }
  }

  return (
    <div style={{ display: 'grid', gridTemplateColumns: '280px 1fr', gap: 16 }}>
      {/* Section picker */}
      <div>
        <div style={{ fontSize: 11, color: 'var(--text-muted)', letterSpacing: 0.5, marginBottom: 8 }}>REPORT SECTIONS</div>
        {sections.map(s => (
          <div
            key={s.id}
            className="card"
            onClick={() => toggle(s.id)}
            style={{
              marginBottom: 6,
              cursor: 'pointer',
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              padding: '8px 12px',
              opacity: s.enabled ? 1 : 0.5,
              borderLeft: `3px solid ${s.enabled ? 'var(--report)' : 'var(--border)'}`,
            }}
          >
            <div style={{
              width: 16, height: 16,
              borderRadius: 3,
              border: `1px solid ${s.enabled ? 'var(--report)' : 'var(--border)'}`,
              background: s.enabled ? 'var(--report)' : 'transparent',
              flexShrink: 0,
              display: 'flex', alignItems: 'center', justifyContent: 'center',
              fontSize: 10, color: '#fff',
            }}>
              {s.enabled ? '✓' : ''}
            </div>
            <span style={{ fontSize: 12 }}>{s.label}</span>
          </div>
        ))}

        {/* Analyst notes */}
        <div style={{ marginTop: 12 }}>
          <div style={{ fontSize: 11, color: 'var(--text-muted)', letterSpacing: 0.5, marginBottom: 6 }}>ANALYST NOTES</div>
          <textarea
            value={notes}
            onChange={e => setNotes(e.target.value)}
            rows={5}
            placeholder="Add context, observations, or recommendations..."
            style={{
              width: '100%', boxSizing: 'border-box',
              background: 'var(--bg-elevated)',
              border: '1px solid var(--border)',
              borderRadius: 4,
              color: 'var(--text-primary)',
              padding: '8px 10px',
              fontSize: 12,
              resize: 'vertical',
              fontFamily: 'inherit',
            }}
          />
        </div>
      </div>

      {/* Preview + export */}
      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        <div className="card" style={{ borderTop: '2px solid var(--report)' }}>
          <div style={{ fontSize: 11, color: 'var(--text-muted)', letterSpacing: 0.5, marginBottom: 12 }}>REPORT PREVIEW</div>
          <div style={{ fontSize: 13, fontWeight: 600, marginBottom: 4 }}>RF Environment Report</div>
          <div style={{ fontSize: 12, color: 'var(--text-secondary)', marginBottom: 12 }}>
            {new Date().toLocaleDateString()} · {sections.filter(s => s.enabled).length} sections selected
          </div>
          {sections.filter(s => s.enabled).map(s => (
            <div key={s.id} style={{ padding: '6px 0', borderBottom: '1px solid var(--border)', fontSize: 12, color: 'var(--text-secondary)' }}>
              § {s.label}
            </div>
          ))}
          {notes && (
            <div style={{ marginTop: 12, padding: 10, background: 'var(--bg-elevated)', borderRadius: 4, fontSize: 12, color: 'var(--text-secondary)', fontStyle: 'italic' }}>
              Note: {notes.slice(0, 120)}{notes.length > 120 ? '…' : ''}
            </div>
          )}
        </div>

        <div className="card" style={{ display: 'flex', gap: 10, alignItems: 'center' }}>
          <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>EXPORT AS</span>
          <button onClick={() => handleExport('json')} disabled={exporting} style={{ color: 'var(--report)' }}>
            {exporting ? 'Generating…' : 'JSON'}
          </button>
          <button disabled style={{ opacity: 0.4 }}>PDF (Phase 2)</button>
          <button disabled style={{ opacity: 0.4 }}>HTML (Phase 2)</button>
        </div>
      </div>
    </div>
  )
}

function TemplatesTab() {
  return (
    <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(280px, 1fr))', gap: 12 }}>
      {TEMPLATES.map(t => (
        <div key={t.id} className="card" style={{ borderTop: '2px solid var(--report)', cursor: 'pointer' }}
          onMouseEnter={e => e.currentTarget.style.background = 'var(--bg-elevated)'}
          onMouseLeave={e => e.currentTarget.style.background = 'var(--bg-surface)'}>
          <div style={{ fontSize: 13, fontWeight: 600, color: 'var(--report)', marginBottom: 4 }}>{t.name}</div>
          <div style={{ fontSize: 12, color: 'var(--text-secondary)' }}>{t.desc}</div>
          <button style={{ marginTop: 12, color: 'var(--report)', fontSize: 11 }}>Use Template →</button>
        </div>
      ))}
    </div>
  )
}

function HistoryTab() {
  const [reports, setReports] = useState([])
  useEffect(() => {
    api.reports.list().then(setReports).catch(() => {})
  }, [])

  return (
    <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
      <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 12 }}>
        <thead>
          <tr style={{ background: 'var(--bg-elevated)' }}>
            {['DATE', 'TITLE', 'TEMPLATE', 'HASH'].map(h => (
              <th key={h} style={{ padding: '8px 12px', textAlign: 'left', color: 'var(--text-muted)', fontWeight: 500, fontSize: 10, letterSpacing: 0.5 }}>{h}</th>
            ))}
          </tr>
        </thead>
        <tbody>
          {reports.length === 0 && (
            <tr><td colSpan={4} style={{ padding: 16, color: 'var(--text-muted)', textAlign: 'center' }}>No reports generated yet</td></tr>
          )}
          {reports.map(r => (
            <tr key={r.id} style={{ borderBottom: '1px solid var(--border)' }}
              onMouseEnter={e => e.currentTarget.style.background = 'var(--bg-elevated)'}
              onMouseLeave={e => e.currentTarget.style.background = ''}>
              <td className="mono" style={{ padding: '7px 12px', fontSize: 11, color: 'var(--text-muted)' }}>{new Date(r.created_at * 1000).toLocaleDateString()}</td>
              <td style={{ padding: '7px 12px' }}>{r.title || '—'}</td>
              <td style={{ padding: '7px 12px', color: 'var(--text-secondary)' }}>{r.template}</td>
              <td className="mono" style={{ padding: '7px 12px', color: 'var(--text-muted)', fontSize: 10 }}>{r.export_hash?.slice(0, 12) || '—'}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
