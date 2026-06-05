# RF-LOG

A **passive, wideband EM reconnaissance platform**: mass-scan the radio spectrum, detect and
characterize emitters, fingerprint and classify them with ML, and build a pattern-of-life
over time. Native macOS app first (Rust + Tauri v2), then web, then a field mobile companion.

## Intent

Most SDR tools are "tune to a frequency and listen." RF-LOG inverts that — the goal is
**survey, collect, and analyze at scale**, not monitor a single channel. The core loop:

```
SURVEY → DETECT → TRIAGE → CHARACTERIZE → CLASSIFY → FINGERPRINT → CORRELATE (pattern-of-life)
```

A heterogeneous pool of SDRs tiles the spectrum; CFAR detection finds signals of interest;
IQ is captured selectively for feature extraction, ML classification, and emitter
fingerprinting; everything accretes into an emitter catalog and a temporal activity map.
Analog/P25 decode exists as a secondary, on-demand drill-down — not the main loop.

**Scope:** strictly **passive receive** — no transmit, no jamming, no active capability.
Operated for research under an FCC Part 5 Experimental Radio License.

## Stack

Rust Cargo workspace (the engine) + Tauri v2 + React, mission-centric UX with a live
spectrum/waterfall.

| Crate | Role |
|-------|------|
| `rf-sensor` | Heterogeneous IQ sensor pool — sweep scheduler, fan-out rings, sim + SoapySDR/RTL backend |
| `rf-dsp` | Survey DSP — windowed PSD, Welch averaging, CA-CFAR detection, occupancy stitching |
| `rf-bus` | Event bus — lossy telemetry + lossless detection path |
| `rf-catalog` | SQLite persistence — missions, sensors, detections |
| `rf-mission` | Mission orchestrator — wires pool → DSP → bus → catalog |
| `rf-types` | Shared contracts |

## Build & run

Requires Rust (edition 2024) and Node. The default build is **simulation-only** and needs
no SDR or system libraries:

```bash
cargo run -p rf-log-app
```

For real hardware (RTL-SDR via SoapySDR):

```bash
brew install soapysdr soapyrtlsdr rtl-sdr
cargo run -p rf-log-app --features soapy   # auto-detects attached SDRs, else falls back to sim
```

## Roadmap

✅ done · 🚧 in progress · ⬜ planned

| Phase | Scope | Status |
|-------|-------|--------|
| **P0** | Foundation & survey: sensor pool, CFAR detection, data layer, Tauri app | 🚧 sim end-to-end working; RTL-SDR hardware validation pending |
| **P1** | Triage & collect: dwell-and-collect lossless IQ, polyphase channelizer, SigMF snapshots, emitter catalog | ⬜ |
| **P2** | Classify & fingerprint: ML inference (ONNX), static + ML-boosted classification, RF-DNA embedding & clustering | ⬜ |
| **P3** | Pattern-of-life: time-bucketed baselines, change detection, activity timelines, anomaly alerts | ⬜ |
| **P4** | Drill-down decode: on-demand analog + P25 on a selected emitter | ⬜ |
| **P5** | Collect-and-train loop: in-app labeling/active learning → model retrain → hot-swap | ⬜ |
| **P6** | Web client (Axum server, shared UI) | ⬜ |
| **P7** | Mobile companion + wideband SDR (HackRF/Airspy for 2.4/5.8 GHz) | ⬜ |
