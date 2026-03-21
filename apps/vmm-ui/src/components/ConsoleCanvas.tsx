import { useEffect, useRef, useState, useCallback } from 'react'

const SCANCODE_MAP: Record<string, number> = {
  Escape: 0x01,
  Digit1: 0x02, Digit2: 0x03, Digit3: 0x04, Digit4: 0x05,
  Digit5: 0x06, Digit6: 0x07, Digit7: 0x08, Digit8: 0x09,
  Digit9: 0x0A, Digit0: 0x0B,
  Minus: 0x0C, Equal: 0x0D, Backspace: 0x0E, Tab: 0x0F,
  KeyQ: 0x10, KeyW: 0x11, KeyE: 0x12, KeyR: 0x13,
  KeyT: 0x14, KeyY: 0x15, KeyU: 0x16, KeyI: 0x17,
  KeyO: 0x18, KeyP: 0x19,
  BracketLeft: 0x1A, BracketRight: 0x1B, Enter: 0x1C,
  ControlLeft: 0x1D,
  KeyA: 0x1E, KeyS: 0x1F, KeyD: 0x20, KeyF: 0x21,
  KeyG: 0x22, KeyH: 0x23, KeyJ: 0x24, KeyK: 0x25, KeyL: 0x26,
  Semicolon: 0x27, Quote: 0x28, Backquote: 0x29,
  ShiftLeft: 0x2A, Backslash: 0x2B,
  KeyZ: 0x2C, KeyX: 0x2D, KeyC: 0x2E, KeyV: 0x2F,
  KeyB: 0x30, KeyN: 0x31, KeyM: 0x32,
  Comma: 0x33, Period: 0x34, Slash: 0x35,
  ShiftRight: 0x36, NumpadMultiply: 0x37,
  AltLeft: 0x38, Space: 0x39, CapsLock: 0x3A,
  F1: 0x3B, F2: 0x3C, F3: 0x3D, F4: 0x3E,
  F5: 0x3F, F6: 0x40, F7: 0x41, F8: 0x42,
  F9: 0x43, F10: 0x44,
  NumLock: 0x45, ScrollLock: 0x46,
  Numpad7: 0x47, Numpad8: 0x48, Numpad9: 0x49, NumpadSubtract: 0x4A,
  Numpad4: 0x4B, Numpad5: 0x4C, Numpad6: 0x4D, NumpadAdd: 0x4E,
  Numpad1: 0x4F, Numpad2: 0x50, Numpad3: 0x51,
  Numpad0: 0x52, NumpadDecimal: 0x53,
  F11: 0x57, F12: 0x58,
  NumpadEnter: 0x1C, ControlRight: 0x1D, NumpadDivide: 0x35, AltRight: 0x38,
  Home: 0x47, ArrowUp: 0x48, PageUp: 0x49,
  ArrowLeft: 0x4B, ArrowRight: 0x4D,
  End: 0x4F, ArrowDown: 0x50, PageDown: 0x51,
  Insert: 0x52, Delete: 0x53,
}

interface Props {
  vmId: string
  /** Whether to capture keyboard events globally (true for fullscreen, false for embedded) */
  captureKeyboard?: boolean
}

export default function ConsoleCanvas({ vmId, captureKeyboard = false }: Props) {
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const wsRef = useRef<WebSocket | null>(null)
  const [connected, setConnected] = useState(false)
  const [resolution, setResolution] = useState({ w: 0, h: 0 })
  const [focused, setFocused] = useState(false)

  // Connect WebSocket — connects directly to backend port to bypass Vite proxy issues
  useEffect(() => {
    const token = localStorage.getItem('vmm_token')
    if (!token || !vmId) return

    // In dev mode Vite proxies /ws/* to the backend, so always use the current host
    const wsHost = window.location.host
    const proto = window.location.protocol === 'https:' ? 'wss' : 'ws'
    const url = `${proto}://${wsHost}/ws/console/${vmId}?token=${token}`

    let reconnectTimer: ReturnType<typeof setTimeout>
    const aliveRef = { current: true }

    const connect = () => {
      if (!aliveRef.current) return
      console.log('[console] connecting to', url)
      const ws = new WebSocket(url)
      ws.binaryType = 'arraybuffer'
      wsRef.current = ws

      ws.onopen = () => {
        console.log('[console] connected')
        setConnected(true)
      }

      ws.onclose = (ev) => {
        console.log('[console] closed', ev.code, ev.reason)
        setConnected(false)
        wsRef.current = null
        if (aliveRef.current) reconnectTimer = setTimeout(connect, 2000)
      }

      ws.onerror = () => {
        // Suppress error on intentional close (React StrictMode double-mount)
        if (aliveRef.current) ws.close()
      }

      ws.onmessage = (ev) => {
        if (!(ev.data instanceof ArrayBuffer)) return
        const buf = new Uint8Array(ev.data)
        if (buf[0] === 0x03) return // keepalive

        if (buf[0] === 0x01 && buf.length > 5) {
          const w = buf[1] | (buf[2] << 8)
          const h = buf[3] | (buf[4] << 8)
          const jpegData = ev.data.slice(5)
          setResolution({ w, h })
          renderFrame(jpegData, w, h)
        }
      }
    }

    connect()

    return () => {
      aliveRef.current = false
      clearTimeout(reconnectTimer)
      if (wsRef.current) {
        wsRef.current.onclose = null // prevent reconnect on intentional close
        wsRef.current.close()
        wsRef.current = null
      }
    }
  }, [vmId])

  // Render JPEG frame to canvas
  const renderFrame = useCallback((jpegBuffer: ArrayBuffer, w: number, h: number) => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return

    // Set canvas internal resolution to match VM framebuffer
    if (canvas.width !== w || canvas.height !== h) {
      canvas.width = w
      canvas.height = h
    }

    const blob = new Blob([jpegBuffer], { type: 'image/jpeg' })
    createImageBitmap(blob).then((bmp) => {
      ctx.drawImage(bmp, 0, 0)
      bmp.close()
    }).catch(() => {
      // Fallback for browsers without createImageBitmap
      const url = URL.createObjectURL(blob)
      const img = new Image()
      img.onload = () => {
        ctx.drawImage(img, 0, 0)
        URL.revokeObjectURL(url)
      }
      img.src = url
    })
  }, [])

  // Keyboard input
  useEffect(() => {
    const shouldCapture = captureKeyboard || focused

    const onKey = (e: KeyboardEvent) => {
      if (!shouldCapture) return
      e.preventDefault()
      e.stopPropagation()
      const code = SCANCODE_MAP[e.code]
      if (code === undefined) return
      wsRef.current?.send(JSON.stringify({
        type: 'key', code, pressed: e.type === 'keydown',
      }))
    }

    window.addEventListener('keydown', onKey, true)
    window.addEventListener('keyup', onKey, true)
    return () => {
      window.removeEventListener('keydown', onKey, true)
      window.removeEventListener('keyup', onKey, true)
    }
  }, [captureKeyboard, focused])

  // Mouse input — send both absolute (USB tablet) and relative (PS/2)
  const lastMouseRef = useRef<{ x: number; y: number } | null>(null)

  const getMousePos = useCallback((e: React.MouseEvent<HTMLCanvasElement>) => {
    const canvas = canvasRef.current
    if (!canvas) return null
    const rect = canvas.getBoundingClientRect()
    const x = Math.round(((e.clientX - rect.left) / rect.width) * canvas.width)
    const y = Math.round(((e.clientY - rect.top) / rect.height) * canvas.height)
    return { x, y }
  }, [])

  const sendMouse = useCallback((e: React.MouseEvent<HTMLCanvasElement>, forceButton?: boolean) => {
    const ws = wsRef.current
    if (!ws) return
    const pos = getMousePos(e)
    if (!pos) return
    const buttons = e.buttons & 0x07

    // Send absolute position (for USB tablet mode)
    ws.send(JSON.stringify({ type: 'mouse_move', x: pos.x, y: pos.y, buttons }))

    // Send relative delta (for PS/2 mouse mode)
    const last = lastMouseRef.current
    if (last) {
      const dx = pos.x - last.x
      const dy = -(pos.y - last.y)  // PS/2: positive Y = up, browser: positive Y = down
      // Send if there's movement OR if it's a button press/release event
      if (dx !== 0 || dy !== 0 || forceButton) {
        ws.send(JSON.stringify({ type: 'mouse_rel', dx, dy, buttons }))
      }
    } else if (forceButton) {
      // First event is a click — send with zero delta
      ws.send(JSON.stringify({ type: 'mouse_rel', dx: 0, dy: 0, buttons }))
    }
    lastMouseRef.current = pos
  }, [getMousePos])

  const sendWheel = useCallback((e: React.WheelEvent) => {
    e.preventDefault()
    const delta = e.deltaY > 0 ? 1 : -1
    wsRef.current?.send(JSON.stringify({ type: 'mouse_wheel', delta }))
  }, [])

  const sendCtrlAltDel = useCallback(() => {
    wsRef.current?.send(JSON.stringify({ type: 'ctrl_alt_del' }))
  }, [])

  // Compute CSS to maintain aspect ratio and fill container
  const aspectStyle = resolution.w > 0 && resolution.h > 0
    ? { aspectRatio: `${resolution.w} / ${resolution.h}` }
    : { aspectRatio: '4 / 3' }

  return (
    <div ref={containerRef} className="relative w-full bg-black rounded-lg overflow-hidden">
      {/* Status overlay */}
      {!connected && (
        <div className="absolute inset-0 flex items-center justify-center z-10 bg-vmm-console-bg/90">
          <div className="text-sm text-vmm-text-muted animate-pulse">Connecting to console...</div>
        </div>
      )}

      {/* Canvas — scales to fill container width while maintaining aspect ratio */}
      <canvas
        ref={canvasRef}
        style={aspectStyle}
        className="w-full h-auto block cursor-none"
        onMouseMove={(e) => sendMouse(e)}
        onMouseDown={(e) => { sendMouse(e, true); setFocused(true) }}
        onMouseUp={(e) => sendMouse(e, true)}
        onWheel={sendWheel}
        onContextMenu={(e) => e.preventDefault()}
        onFocus={() => setFocused(true)}
        onBlur={() => { setFocused(false); lastMouseRef.current = null }}
        tabIndex={0}
      />

      {/* Bottom bar */}
      <div className="absolute bottom-0 left-0 right-0 flex items-center justify-between px-3 py-1.5 bg-black/70 text-[10px] text-vmm-text-muted">
        <div className="flex items-center gap-2">
          <span className={`w-1.5 h-1.5 rounded-full ${connected ? 'bg-vmm-success' : 'bg-vmm-danger'}`} />
          {connected ? `${resolution.w}x${resolution.h}` : 'Disconnected'}
        </div>
        <div className="flex items-center gap-2">
          <button onClick={sendCtrlAltDel}
            className="px-2 py-0.5 bg-vmm-surface/50 hover:bg-vmm-surface rounded text-[10px] text-vmm-text-muted hover:text-vmm-text cursor-pointer">
            Ctrl+Alt+Del
          </button>
          {focused && <span className="text-vmm-accent">Keyboard captured</span>}
        </div>
      </div>
    </div>
  )
}
