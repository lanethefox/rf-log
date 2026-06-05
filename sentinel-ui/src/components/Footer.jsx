import { useSpectrumStore } from '../stores/useSpectrumStore'

export default function Footer() {
  const { frame, connected } = useSpectrumStore()

  return (
    <footer style={{
      height: 28,
      background: 'var(--bg-surface)',
      borderTop: '1px solid var(--border)',
      display: 'flex',
      alignItems: 'center',
      padding: '0 16px',
      gap: 16,
      flexShrink: 0,
      fontSize: 11,
      color: 'var(--text-secondary)',
    }}>
      {/* SDR status */}
      <span>
        <span style={{
          display: 'inline-block',
          width: 6, height: 6,
          borderRadius: '50%',
          background: connected ? 'var(--success)' : 'var(--text-muted)',
          marginRight: 5,
          verticalAlign: 'middle',
        }} />
        {connected ? 'SDR ONLINE' : 'SIM MODE'}
      </span>

      {frame && (
        <>
          <span className="mono">{frame.band}</span>
          <span className="mono">
            {frame.freqs?.[0]?.toFixed(1)}–{frame.freqs?.[frame.freqs.length - 1]?.toFixed(1)} MHz
          </span>
          <span className="mono">NF {frame.noise_floor?.toFixed(1)} dBFS</span>
        </>
      )}

      <div style={{ flex: 1 }} />
      <span>RF-SENTINEL v0.1 — local only, no telemetry</span>
    </footer>
  )
}
