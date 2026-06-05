export default function Placeholder({ title, subtitle, color }) {
  return (
    <div style={{
      display: 'flex',
      flexDirection: 'column',
      alignItems: 'center',
      justifyContent: 'center',
      height: '100%',
      minHeight: 240,
      gap: 8,
      padding: 32,
      textAlign: 'center',
    }}>
      <div style={{
        width: 48, height: 48,
        borderRadius: '50%',
        border: `2px solid ${color || 'var(--border)'}`,
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        fontSize: 20,
        marginBottom: 8,
        color: color || 'var(--text-muted)',
      }}>
        ◉
      </div>
      <div style={{ fontSize: 15, fontWeight: 600, color: color || 'var(--text-secondary)' }}>{title}</div>
      <div style={{ fontSize: 12, color: 'var(--text-muted)', maxWidth: 400 }}>{subtitle}</div>
      <div style={{ marginTop: 8, fontSize: 11, color: 'var(--text-muted)', opacity: 0.6 }}>Coming in next phase</div>
    </div>
  )
}
