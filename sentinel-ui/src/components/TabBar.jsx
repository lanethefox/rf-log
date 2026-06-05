export default function TabBar({ tabs, active, color, onChange }) {
  return (
    <div style={{
      display: 'flex',
      gap: 2,
      padding: '8px 16px',
      borderBottom: '1px solid var(--border)',
      background: 'var(--bg-elevated)',
      flexShrink: 0,
    }}>
      {tabs.map(t => (
        <button
          key={t}
          onClick={() => onChange(t)}
          style={{
            padding: '4px 12px',
            fontSize: 11,
            letterSpacing: 0.5,
            fontWeight: 500,
            borderRadius: 4,
            border: active === t ? `1px solid ${color}` : '1px solid transparent',
            background: active === t ? `${color}18` : 'transparent',
            color: active === t ? color : 'var(--text-muted)',
            cursor: 'pointer',
            transition: 'all 0.15s',
          }}
        >
          {t}
        </button>
      ))}
    </div>
  )
}
