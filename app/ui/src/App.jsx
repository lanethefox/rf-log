import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "./ipc.js";

const W = 900;
const PRESETS = [
  { name: "VHF 144–174", bands: [{ name: "VHF", low_hz: 144e6, high_hz: 174e6 }] },
  { name: "UHF 400–470", bands: [{ name: "UHF", low_hz: 400e6, high_hz: 470e6 }] },
  { name: "FM 88–108", bands: [{ name: "FM", low_hz: 88e6, high_hz: 108e6 }] },
  {
    name: "VHF + UHF",
    bands: [
      { name: "VHF", low_hz: 144e6, high_hz: 174e6 },
      { name: "UHF", low_hz: 400e6, high_hz: 470e6 },
    ],
  },
];
const SAMPLE_RATES = [2.048e6, 2.4e6, 2.56e6, 3.2e6];

const rangeOf = (bands) => [
  Math.min(...bands.map((b) => b.low_hz)),
  Math.max(...bands.map((b) => b.high_hz)),
];

function colormap(db) {
  const t = Math.max(0, Math.min(1, (db + 120) / 100));
  const r = Math.floor(255 * Math.min(1, Math.max(0, t * 2 - 0.6)));
  const g = Math.floor(255 * Math.min(1, Math.max(0, t * 1.6)));
  const b = Math.floor(255 * Math.min(1, Math.max(0, 1 - t * 1.8)));
  return [r, g, b];
}

const mhz = (hz) => (hz / 1e6).toFixed(3);

export default function App() {
  const [missions, setMissions] = useState([]);
  const [selected, setSelected] = useState(null);
  const [activeId, setActiveId] = useState(null);
  const [detections, setDetections] = useState([]);
  const [devices, setDevices] = useState([]);
  const [showSettings, setShowSettings] = useState(false);
  const [starting, setStarting] = useState(false);
  const [name, setName] = useState("Site survey");
  const [presetIdx, setPresetIdx] = useState(0);

  const specRef = useRef(null);
  const fallRef = useRef(null);
  const lineRef = useRef(new Float32Array(W).fill(-120));
  const rangeRef = useRef(rangeOf(PRESETS[0].bands));

  const refreshMissions = useCallback(async () => {
    setMissions(await api.listMissions());
  }, []);

  const onPsd = useCallback((f) => {
    const line = lineRef.current;
    const [lo, hi] = rangeRef.current;
    const n = f.psd_dbfs.length;
    const lowEdge = f.tile_center_hz - (n / 2) * f.bin_hz;
    for (let i = 0; i < n; i++) {
      const freq = lowEdge + (i + 0.5) * f.bin_hz;
      if (freq < lo || freq >= hi) continue;
      const x = Math.floor(((freq - lo) / (hi - lo)) * W);
      if (x >= 0 && x < W && f.psd_dbfs[i] > line[x]) line[x] = f.psd_dbfs[i];
    }
  }, []);

  // initial load + live event subscriptions
  useEffect(() => {
    refreshMissions();
    api.getStatus().then((s) => setActiveId(s.active_mission ?? null));
    api.listDevices().then(setDevices);

    const unsubs = [];
    const sub = (ev, cb) => api.on(ev, cb).then((u) => unsubs.push(u));
    sub("psd", onPsd);
    sub("detection", (d) => setDetections((prev) => [d, ...prev].slice(0, 300)));
    sub("devices", (list) => setDevices(list));
    sub("mission_state", ([id, phase]) => {
      if (phase === "Running") {
        setActiveId(id);
        setStarting(false);
      } else {
        setActiveId(null);
      }
      refreshMissions();
    });
    sub("mission_error", (msg) => {
      setStarting(false);
      alert("Mission error: " + msg);
    });
    return () => unsubs.forEach((u) => u && u());
  }, [refreshMissions, onPsd]);

  // render loop: spectrum line + scrolling waterfall
  useEffect(() => {
    const tick = setInterval(() => {
      const line = lineRef.current;
      const sc = specRef.current;
      if (sc) {
        const ctx = sc.getContext("2d");
        const H = sc.height;
        ctx.fillStyle = "#060a10";
        ctx.fillRect(0, 0, W, H);
        ctx.strokeStyle = "#39d353";
        ctx.lineWidth = 1;
        ctx.beginPath();
        for (let x = 0; x < W; x++) {
          const y = H - Math.max(0, Math.min(1, (line[x] + 120) / 100)) * H;
          x === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y);
        }
        ctx.stroke();
      }
      const fc = fallRef.current;
      if (fc) {
        const ctx = fc.getContext("2d");
        const H = fc.height;
        ctx.drawImage(fc, 0, 0, W, H - 1, 0, 1, W, H - 1);
        const row = ctx.createImageData(W, 1);
        for (let x = 0; x < W; x++) {
          const [r, g, b] = colormap(line[x]);
          row.data[x * 4] = r;
          row.data[x * 4 + 1] = g;
          row.data[x * 4 + 2] = b;
          row.data[x * 4 + 3] = 255;
        }
        ctx.putImageData(row, 0, 0);
      }
      for (let x = 0; x < W; x++) line[x] = Math.max(-120, line[x] - 6);
    }, 120);
    return () => clearInterval(tick);
  }, []);

  const setRangeFromBands = (bands) => {
    if (bands?.length) rangeRef.current = rangeOf(bands);
  };

  async function doCreate() {
    const preset = PRESETS[presetIdx];
    const id = await api.createMission(name, preset.bands);
    setRangeFromBands(preset.bands);
    await refreshMissions();
    setSelected(id);
    setDetections([]);
  }

  async function selectMission(m) {
    setSelected(m.id);
    setRangeFromBands(m.bands);
    setDetections(await api.listDetections(m.id, 300));
  }

  async function doStart() {
    if (selected == null) return;
    setStarting(true);
    try {
      await api.startMission(selected); // returns immediately; devices already open
    } catch (e) {
      setStarting(false);
      alert("Start failed: " + e);
    }
  }

  async function doStop() {
    try {
      await api.stopMission();
      setActiveId(null);
    } catch (e) {
      alert("Stop failed: " + e);
    }
  }

  const updateDevice = (id, cfg) => {
    // optimistic local update; backend confirms via a devices event
    setDevices((ds) => ds.map((d) => (d.id === id ? { ...d, ...renameCfg(cfg) } : d)));
    api.setDeviceConfig(id, cfg);
  };

  const [lo, hi] = rangeRef.current;
  const active = activeId != null;
  const readyCount = devices.filter(
    (d) => d.enabled && (d.state === "Ready" || d.state === "InUse")
  ).length;
  const canStart = selected != null && !active && !starting && readyCount > 0;

  return (
    <div className="app">
      <div className="header">
        <h1>RF-LOG</h1>
        <span className="sub">passive EM reconnaissance</span>
        <div className="spacer" />
        <button className="ghost" onClick={() => api.refreshDevices()} title="Re-scan for SDRs">
          ⟳ scan
        </button>
        <button className="ghost" onClick={() => setShowSettings(true)}>
          ⚙ devices
        </button>
        <span className={`pill ${active ? "on" : starting ? "warn" : "off"}`}>
          {active ? `MISSION ${activeId} RUNNING` : starting ? "STARTING…" : "IDLE"}
        </span>
      </div>

      <div className="body">
        <div className="side">
          <div className="card">
            <h2>New mission</h2>
            <div className="pad">
              <label>Name</label>
              <input value={name} onChange={(e) => setName(e.target.value)} />
              <label>Bands</label>
              <select value={presetIdx} onChange={(e) => setPresetIdx(+e.target.value)}>
                {PRESETS.map((p, i) => (
                  <option key={i} value={i}>
                    {p.name}
                  </option>
                ))}
              </select>
              <div className="row">
                <button className="primary" onClick={doCreate}>
                  Create
                </button>
              </div>
            </div>
          </div>

          <div className="card" style={{ marginTop: 12 }}>
            <h2>Missions</h2>
            <div className="pad">
              {missions.length === 0 && <div className="empty">none yet</div>}
              {missions.map((m) => (
                <div
                  key={m.id}
                  className={`mission ${selected === m.id ? "sel" : ""}`}
                  onClick={() => selectMission(m)}
                >
                  <div className="nm">
                    #{m.id} {m.name}
                  </div>
                  <div className="meta">
                    {m.phase} · {m.bands?.length ?? 0} band(s)
                  </div>
                </div>
              ))}
              <div className="row">
                <button className="primary" disabled={!canStart} onClick={doStart}>
                  {starting ? "Starting…" : "▶ Start"}
                </button>
                <button className="danger" disabled={!active} onClick={doStop}>
                  ■ Stop
                </button>
              </div>
              {readyCount === 0 && (
                <div className="hint">no SDR ready — connect a device (see ⚙ devices)</div>
              )}
            </div>
          </div>
        </div>

        <div className="main">
          <div className="card">
            <h2>Spectrum</h2>
            <div className="pad" style={{ paddingBottom: 6 }}>
              <canvas ref={specRef} width={W} height={140} />
            </div>
          </div>

          <div className="card">
            <h2>Waterfall</h2>
            <div className="pad" style={{ paddingBottom: 6 }}>
              <canvas ref={fallRef} width={W} height={300} />
              <div className="axis">
                <span>{mhz(lo)} MHz</span>
                <span>{mhz((lo + hi) / 2)} MHz</span>
                <span>{mhz(hi)} MHz</span>
              </div>
            </div>
          </div>

          <div className="card det-wrap">
            <h2>Detections ({detections.length})</h2>
            {detections.length === 0 ? (
              <div className="empty">no detections yet — create a mission and press Start</div>
            ) : (
              <table>
                <thead>
                  <tr>
                    <th className="l">center (MHz)</th>
                    <th>BW (kHz)</th>
                    <th>power (dBFS)</th>
                    <th>SNR (dB)</th>
                    <th>sensor</th>
                  </tr>
                </thead>
                <tbody>
                  {detections.map((d, i) => (
                    <tr key={i}>
                      <td className="l">{(d.center_hz / 1e6).toFixed(4)}</td>
                      <td>{(d.bandwidth_hz / 1e3).toFixed(1)}</td>
                      <td>{d.power_dbfs.toFixed(1)}</td>
                      <td>{d.snr_db.toFixed(1)}</td>
                      <td>S{d.sensor}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </div>
        </div>
      </div>

      {/* device status bar */}
      <div className="statusbar">
        <span className="sb-label">SDRs</span>
        {devices.length === 0 && <span className="sb-empty">none detected — connect an SDR</span>}
        {devices.map((d) => (
          <span key={d.id} className={`dev ${d.state}`} title={`${d.serial} · ${d.driver}`}>
            <span className="dot" />
            {d.label}
            <span className="dev-state">{deviceStateText(d.state)}</span>
            {(d.state === "Ready" || d.state === "InUse") && (
              <span className="dev-meta">{(d.sample_rate_hz / 1e6).toFixed(2)}M</span>
            )}
          </span>
        ))}
        <div className="spacer" />
        <span className="sb-label">{readyCount} ready</span>
      </div>

      {showSettings && (
        <DeviceSettings
          devices={devices}
          onClose={() => setShowSettings(false)}
          onChange={updateDevice}
          onRescan={() => api.refreshDevices()}
        />
      )}
    </div>
  );
}

function deviceStateText(s) {
  if (s === "Opening") return "loading…";
  if (s === "Detected") return "detected";
  if (s === "InUse") return "in use";
  if (s === "Disconnected") return "gone";
  return s.toLowerCase();
}

// map camelCase cfg back to snake_case fields for optimistic UI update
function renameCfg(cfg) {
  const out = {};
  if ("enabled" in cfg) out.enabled = cfg.enabled;
  if ("autoGain" in cfg) out.auto_gain = cfg.autoGain;
  if ("gainDb" in cfg) out.gain_db = cfg.gainDb;
  if ("sampleRateHz" in cfg) out.sample_rate_hz = cfg.sampleRateHz;
  return out;
}

function DeviceSettings({ devices, onClose, onChange, onRescan }) {
  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <div className="modal-head">
          <h2>Devices</h2>
          <div className="spacer" />
          <button className="ghost" onClick={onRescan}>
            ⟳ rescan
          </button>
          <button className="ghost" onClick={onClose}>
            ✕
          </button>
        </div>
        <div className="modal-body">
          {devices.length === 0 && <div className="empty">No SDRs detected. Plug one in.</div>}
          {devices.map((d) => (
            <div key={d.id} className="dev-row">
              <label className="dev-enable">
                <input
                  type="checkbox"
                  checked={d.enabled}
                  onChange={(e) => onChange(d.id, { enabled: e.target.checked })}
                />
              </label>
              <div className="dev-id">
                <div className="dev-name">{d.label}</div>
                <div className="dev-sub">
                  {d.serial} · {d.driver} · {(d.freq_min_hz / 1e6).toFixed(0)}–
                  {(d.freq_max_hz / 1e6).toFixed(0)} MHz · <span className={`tag ${d.state}`}>{d.state}</span>
                </div>
              </div>
              <div className="dev-cfg">
                <label>rate</label>
                <select
                  value={nearestRate(d.sample_rate_hz)}
                  onChange={(e) => onChange(d.id, { sampleRateHz: +e.target.value })}
                >
                  {SAMPLE_RATES.map((r) => (
                    <option key={r} value={r}>
                      {(r / 1e6).toFixed(3)} M
                    </option>
                  ))}
                </select>
                <label className="inline">
                  <input
                    type="checkbox"
                    checked={d.auto_gain}
                    onChange={(e) => onChange(d.id, { autoGain: e.target.checked })}
                  />
                  AGC
                </label>
                <input
                  className="gain"
                  type="number"
                  min="0"
                  max="50"
                  step="0.5"
                  disabled={d.auto_gain}
                  value={d.gain_db}
                  onChange={(e) => onChange(d.id, { gainDb: +e.target.value })}
                />
                <span className="unit">dB</span>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function nearestRate(hz) {
  return SAMPLE_RATES.reduce((a, b) => (Math.abs(b - hz) < Math.abs(a - hz) ? b : a), SAMPLE_RATES[1]);
}
