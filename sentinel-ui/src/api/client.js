const BASE = ''  // same origin via vite proxy in dev

export async function apiFetch(path, opts = {}) {
  const res = await fetch(BASE + path, {
    headers: { 'Content-Type': 'application/json', ...opts.headers },
    ...opts,
    body: opts.body ? JSON.stringify(opts.body) : undefined,
  })
  if (!res.ok) throw new Error(`API ${path} → ${res.status}`)
  return res.json()
}

export const api = {
  baselines: {
    list:           () => apiFetch('/api/baselines'),
    get:            id => apiFetch(`/api/baselines/${id}`),
    bins:           id => apiFetch(`/api/baselines/${id}/bins`),
    activate:       id => apiFetch(`/api/baselines/${id}/activate`, { method: 'POST' }),
    captureStart:   b  => apiFetch('/api/baselines/capture/start', { method: 'POST', body: b }),
    captureStop:    () => apiFetch('/api/baselines/capture/stop', { method: 'POST' }),
    captureStatus:  () => apiFetch('/api/baselines/capture/status'),
  },
  anomalies: {
    list:        () => apiFetch('/api/anomalies'),
    acknowledge: id => apiFetch(`/api/anomalies/${id}/acknowledge`, { method: 'POST' }),
  },
  surveys:    { list: () => apiFetch('/api/surveys'), create: b => apiFetch('/api/surveys', { method: 'POST', body: b }) },
  reports:    { list: () => apiFetch('/api/reports'), create: b => apiFetch('/api/reports', { method: 'POST', body: b }) },
  drones: {
    detections: () => apiFetch('/api/drones/detections'),
    tracks:     () => apiFetch('/api/drones/tracks'),
    remoteId:   () => apiFetch('/api/drones/remote-id'),
    signatures: () => apiFetch('/api/drones/signatures'),
    whitelist:  () => apiFetch('/api/drones/whitelist'),
    addWhitelist: b => apiFetch('/api/drones/whitelist', { method: 'POST', body: b }),
  },
  pdw: {
    list:     () => apiFetch('/api/pdw'),
    priStats: () => apiFetch('/api/pdw/pri'),
  },
  harmonics: { list: () => apiFetch('/api/harmonics') },
  emitters: {
    list:   () => apiFetch('/api/emitters'),
    get:    id => apiFetch(`/api/emitters/${id}`),
    create: b  => apiFetch('/api/emitters', { method: 'POST', body: b }),
    update: (id, b) => apiFetch(`/api/emitters/${id}`, { method: 'PUT', body: b }),
    delete: id => apiFetch(`/api/emitters/${id}`, { method: 'DELETE' }),
  },
  config: { get: () => apiFetch('/api/config'), update: b => apiFetch('/api/config', { method: 'POST', body: b }) },
}

// WebSocket helpers — return close functions for cleanup
export function connectSpectrumWs(onFrame) {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const ws = new WebSocket(`${proto}://${location.host}/ws/spectrum`)
  ws.onmessage = e => { try { onFrame(JSON.parse(e.data)) } catch {} }
  ws.onerror = () => {}
  return () => ws.close()
}

export function connectEventsWs(onEvent) {
  const proto = location.protocol === 'https:' ? 'wss' : 'ws'
  const ws = new WebSocket(`${proto}://${location.host}/ws/events`)
  ws.onmessage = e => { try { onEvent(JSON.parse(e.data)) } catch {} }
  ws.onerror = () => {}
  return () => ws.close()
}
