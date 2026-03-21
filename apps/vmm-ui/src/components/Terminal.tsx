import { useEffect, useRef, useState, useCallback } from 'react'

interface OutputLine {
  kind: 'stdout' | 'stderr' | 'success' | 'warning' | 'info' | 'tableheader' | 'tablerow'
  text: string
}

interface TerminalProps {
  className?: string
}

const LINE_COLORS: Record<string, string> = {
  stdout: 'text-gray-200',
  stderr: 'text-red-400',
  success: 'text-emerald-400',
  warning: 'text-yellow-400',
  info: 'text-gray-500',
  tableheader: 'text-vmm-accent font-semibold',
  tablerow: 'text-gray-300',
}

export default function Terminal({ className = '' }: TerminalProps) {
  const [lines, setLines] = useState<{ kind: string; text: string }[]>([])
  const [input, setInput] = useState('')
  const [history, setHistory] = useState<string[]>([])
  const [historyIdx, setHistoryIdx] = useState(-1)
  const [connected, setConnected] = useState(false)
  const [completions, setCompletions] = useState<string[]>([])

  const wsRef = useRef<WebSocket | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const scrollRef = useRef<HTMLDivElement>(null)
  const mountedRef = useRef(true)

  // Auto-scroll to bottom
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [lines])

  // WebSocket connection
  useEffect(() => {
    mountedRef.current = true
    const token = localStorage.getItem('vmm_token')
    if (!token) return

    const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:'
    // In dev mode Vite proxies /ws/* to the backend, so always use the current host
    const host = window.location.host
    const url = `${proto}//${host}/ws/terminal?token=${token}`

    const ws = new WebSocket(url)
    wsRef.current = ws

    ws.onopen = () => {
      if (mountedRef.current) setConnected(true)
    }

    ws.onmessage = (e) => {
      if (!mountedRef.current) return
      // Skip binary messages (from console handler)
      if (typeof e.data !== 'string') return
      try {
        const msg = JSON.parse(e.data)
        // Ignore Vite HMR or unknown messages
        if (!msg.type || (msg.type !== 'output' && msg.type !== 'completion')) {
          console.log('[terminal] ignoring message type:', msg.type)
          return
        }
        if (msg.type === 'output' && Array.isArray(msg.lines)) {
          // Handle clear command
          const hasClear = msg.lines.some((l: OutputLine) => l.text === '__CLEAR__')
          if (hasClear) {
            setLines([])
            return
          }
          setLines(prev => [...prev, ...msg.lines])
        } else if (msg.type === 'completion' && Array.isArray(msg.completions)) {
          setCompletions(msg.completions)
          // If exactly one match, auto-complete
          if (msg.completions.length === 1) {
            const tokens = input.split(' ')
            tokens[tokens.length - 1] = msg.completions[0]
            setInput(tokens.join(' ') + ' ')
            setCompletions([])
          } else if (msg.completions.length > 1) {
            // Show completions as output
            setLines(prev => [...prev,
              { kind: 'info', text: msg.completions.join('  ') }
            ])
          }
        }
      } catch { /* ignore parse errors */ }
    }

    ws.onerror = () => {
      if (mountedRef.current) setConnected(false)
    }

    ws.onclose = () => {
      if (mountedRef.current) {
        setConnected(false)
        setLines(prev => [...prev, { kind: 'warning', text: 'Connection closed.' }])
      }
    }

    return () => {
      mountedRef.current = false
      ws.close()
    }
  }, [])

  const sendCommand = useCallback((cmd: string) => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return
    wsRef.current.send(JSON.stringify({ type: 'exec', input: cmd }))
    // Add to history
    setHistory(prev => {
      const filtered = prev.filter(h => h !== cmd)
      return [cmd, ...filtered].slice(0, 100)
    })
    setHistoryIdx(-1)
    // Echo the command line
    setLines(prev => [...prev, { kind: 'stdout', text: `vmm> ${cmd}` }])
  }, [])

  const requestCompletion = useCallback(() => {
    if (!wsRef.current || wsRef.current.readyState !== WebSocket.OPEN) return
    wsRef.current.send(JSON.stringify({ type: 'complete', input }))
  }, [input])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault()
      const cmd = input.trim()
      if (cmd) sendCommand(cmd)
      setInput('')
      setCompletions([])
    } else if (e.key === 'Tab') {
      e.preventDefault()
      requestCompletion()
    } else if (e.key === 'ArrowUp') {
      e.preventDefault()
      if (history.length > 0) {
        const newIdx = Math.min(historyIdx + 1, history.length - 1)
        setHistoryIdx(newIdx)
        setInput(history[newIdx])
      }
    } else if (e.key === 'ArrowDown') {
      e.preventDefault()
      if (historyIdx > 0) {
        const newIdx = historyIdx - 1
        setHistoryIdx(newIdx)
        setInput(history[newIdx])
      } else {
        setHistoryIdx(-1)
        setInput('')
      }
    } else if (e.key === 'l' && e.ctrlKey) {
      e.preventDefault()
      setLines([])
    }
  }

  // Focus input on click anywhere
  const handleClick = () => {
    inputRef.current?.focus()
  }

  return (
    <div
      className={`bg-gray-950 rounded-lg border border-gray-800 flex flex-col font-mono text-sm ${className}`}
      onClick={handleClick}
    >
      {/* Title bar */}
      <div className="flex items-center justify-between px-4 py-2 border-b border-gray-800 bg-gray-900/50 rounded-t-lg">
        <div className="flex items-center gap-2">
          <div className={`w-2 h-2 rounded-full ${connected ? 'bg-emerald-400' : 'bg-red-400'}`} />
          <span className="text-xs text-gray-400">
            CoreVM Terminal {connected ? '— Connected' : '— Disconnected'}
          </span>
        </div>
        <button
          onClick={(e) => { e.stopPropagation(); setLines([]) }}
          className="text-xs text-gray-500 hover:text-gray-300 px-2 py-0.5 rounded hover:bg-gray-800"
        >
          Clear
        </button>
      </div>

      {/* Output area */}
      <div ref={scrollRef} className="flex-1 overflow-y-auto p-4 min-h-0">
        {lines.map((line, i) => (
          <div key={i} className={`${LINE_COLORS[line.kind] || 'text-gray-200'} whitespace-pre leading-5`}>
            {line.text || '\u00A0'}
          </div>
        ))}
      </div>

      {/* Input line */}
      <div className="flex items-center px-4 py-2 border-t border-gray-800 bg-gray-900/30">
        <span className="text-vmm-accent mr-2 select-none shrink-0">vmm&gt;</span>
        <input
          ref={inputRef}
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          className="flex-1 bg-transparent text-gray-200 outline-none caret-vmm-accent"
          placeholder={connected ? 'Type a command...' : 'Connecting...'}
          disabled={!connected}
          autoFocus
          autoComplete="off"
          spellCheck={false}
        />
      </div>
    </div>
  )
}
