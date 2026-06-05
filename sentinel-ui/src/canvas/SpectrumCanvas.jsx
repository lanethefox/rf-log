import { useEffect, useRef } from 'react'

const NOISE_COLOR  = 'rgba(212,167,0,0.6)'
const TRACE_COLOR  = '#388bfd'
const FILL_TOP     = 'rgba(56,139,253,0.25)'
const FILL_BOT     = 'rgba(56,139,253,0.02)'

export default function SpectrumCanvas({ frame, height = 180, style }) {
  const canvasRef = useRef(null)

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    const W = canvas.width
    const H = canvas.height

    ctx.clearRect(0, 0, W, H)
    ctx.fillStyle = '#0d1117'
    ctx.fillRect(0, 0, W, H)

    if (!frame || !frame.freqs?.length) {
      ctx.fillStyle = '#484f58'
      ctx.font = '13px JetBrains Mono, monospace'
      ctx.textAlign = 'center'
      ctx.fillText('No spectrum data', W / 2, H / 2)
      return
    }

    const { freqs, powers, noise_floor } = frame
    const n = freqs.length
    const minF = freqs[0]
    const maxF = freqs[n - 1]
    const minP = -120
    const maxP = -40

    const toX = f => ((f - minF) / (maxF - minF)) * W
    const toY = p => H - ((p - minP) / (maxP - minP)) * H

    // Noise floor line
    const nfY = toY(noise_floor)
    ctx.strokeStyle = NOISE_COLOR
    ctx.lineWidth = 1
    ctx.setLineDash([4, 4])
    ctx.beginPath()
    ctx.moveTo(0, nfY)
    ctx.lineTo(W, nfY)
    ctx.stroke()
    ctx.setLineDash([])

    // Gradient fill
    const grad = ctx.createLinearGradient(0, 0, 0, H)
    grad.addColorStop(0, FILL_TOP)
    grad.addColorStop(1, FILL_BOT)

    ctx.beginPath()
    ctx.moveTo(toX(freqs[0]), H)
    for (let i = 0; i < n; i++) {
      ctx.lineTo(toX(freqs[i]), toY(powers[i]))
    }
    ctx.lineTo(toX(freqs[n - 1]), H)
    ctx.closePath()
    ctx.fillStyle = grad
    ctx.fill()

    // Trace line
    ctx.beginPath()
    ctx.strokeStyle = TRACE_COLOR
    ctx.lineWidth = 1.5
    for (let i = 0; i < n; i++) {
      const x = toX(freqs[i])
      const y = toY(powers[i])
      i === 0 ? ctx.moveTo(x, y) : ctx.lineTo(x, y)
    }
    ctx.stroke()

    // Frequency axis labels
    ctx.fillStyle = '#8b949e'
    ctx.font = '10px JetBrains Mono, monospace'
    ctx.textAlign = 'center'
    const labelCount = 6
    for (let i = 0; i <= labelCount; i++) {
      const f = minF + (maxF - minF) * (i / labelCount)
      const x = toX(f)
      ctx.fillText(`${f.toFixed(1)}`, x, H - 4)
    }
  }, [frame])

  return (
    <canvas
      ref={canvasRef}
      width={800}
      height={height}
      style={{ width: '100%', height, display: 'block', ...style }}
    />
  )
}
