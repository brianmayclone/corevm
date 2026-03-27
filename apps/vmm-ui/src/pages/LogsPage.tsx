import { useEffect, useState, useRef } from 'react'
import { RefreshCw, FileText, AlertCircle, Server } from 'lucide-react'
import api from '../api/client'
import Card from '../components/Card'
import type { AllHostLogsResponse, HostLogsResponse, ServiceLogEntry } from '../api/types'

const SERVICE_LABELS: Record<string, string> = {
  'vmm-server': 'VMM Server',
  'vmm-san': 'CoreSAN',
  'vmm-cluster': 'VMM Cluster',
}

export default function LogsPage() {
  const [data, setData] = useState<AllHostLogsResponse | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [activeHost, setActiveHost] = useState<string | null>(null)
  const [activeService, setActiveService] = useState('vmm-server')
  const [lines, setLines] = useState(200)
  const [autoRefresh, setAutoRefresh] = useState(false)
  const scrollRef = useRef<HTMLPreElement>(null)

  const fetchLogs = async () => {
    setLoading(true)
    setError(null)
    try {
      const { data: resp } = await api.get<AllHostLogsResponse>('/api/logs', {
        params: { lines }
      })
      setData(resp)
      if (!activeHost && resp.hosts.length > 0) {
        setActiveHost(resp.hosts[0].host_id || resp.hosts[0].hostname)
      }
    } catch (e: any) {
      setError(e?.response?.data?.error || e?.message || 'Failed to fetch logs')
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { fetchLogs() }, [lines])

  useEffect(() => {
    if (!autoRefresh) return
    const interval = setInterval(fetchLogs, 5000)
    return () => clearInterval(interval)
  }, [autoRefresh, lines])

  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight
    }
  }, [data, activeHost, activeService])

  const currentHost = data?.hosts.find(h => (h.host_id || h.hostname) === activeHost)
  const activeLog: ServiceLogEntry | undefined = currentHost?.services.find(s => s.service === activeService)

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-bold text-vmm-text">Service Logs</h1>
        <div className="flex items-center gap-2">
          <select
            value={lines}
            onChange={e => setLines(Number(e.target.value))}
            className="bg-vmm-surface border border-vmm-border rounded px-2 py-1.5 text-xs text-vmm-text"
          >
            <option value={50}>50 lines</option>
            <option value={200}>200 lines</option>
            <option value={500}>500 lines</option>
            <option value={1000}>1000 lines</option>
          </select>
          <button
            onClick={() => setAutoRefresh(!autoRefresh)}
            className={`flex items-center gap-1 px-3 py-1.5 text-xs rounded border transition-colors cursor-pointer ${
              autoRefresh
                ? 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30'
                : 'bg-vmm-surface text-vmm-text-muted border-vmm-border hover:text-vmm-text'
            }`}
          >
            <RefreshCw size={12} className={autoRefresh ? 'animate-spin' : ''} />
            Auto-Refresh
          </button>
          <button
            onClick={fetchLogs}
            disabled={loading}
            className="flex items-center gap-1 px-3 py-1.5 text-xs rounded border bg-vmm-surface text-vmm-text-muted border-vmm-border hover:text-vmm-text transition-colors cursor-pointer disabled:opacity-50"
          >
            <RefreshCw size={12} className={loading ? 'animate-spin' : ''} />
            Refresh
          </button>
        </div>
      </div>

      {error && (
        <div className="flex items-center gap-2 px-3 py-2 bg-vmm-danger/10 border border-vmm-danger/30 rounded-lg text-sm text-vmm-danger">
          <AlertCircle size={14} /> {error}
        </div>
      )}

      <div className="flex gap-4">
        {/* Host sidebar */}
        <div className="w-48 flex-shrink-0 space-y-1">
          <div className="text-xs font-semibold text-vmm-text-muted uppercase tracking-wide mb-2">Hosts</div>
          {data?.hosts.map(h => {
            const key = h.host_id || h.hostname
            return (
              <button
                key={key}
                onClick={() => setActiveHost(key)}
                className={`w-full flex items-center gap-2 px-3 py-2 text-xs rounded-lg transition-colors cursor-pointer text-left
                  ${activeHost === key
                    ? 'bg-vmm-accent/20 text-vmm-accent border border-vmm-accent/30'
                    : 'bg-vmm-surface text-vmm-text-muted hover:text-vmm-text border border-vmm-border'
                  }`}
              >
                <Server size={12} />
                {h.hostname}
              </button>
            )
          })}
          {data && data.hosts.length === 0 && (
            <p className="text-xs text-vmm-text-muted">No online hosts.</p>
          )}
        </div>

        {/* Log content */}
        <div className="flex-1 space-y-3">
          {/* Service tabs */}
          <div className="flex gap-1">
            {['vmm-server', 'vmm-san', 'vmm-cluster'].map(svc => (
              <button
                key={svc}
                onClick={() => setActiveService(svc)}
                className={`px-3 py-1.5 text-xs font-medium rounded-md transition-colors cursor-pointer
                  ${activeService === svc
                    ? 'bg-vmm-accent/20 text-vmm-accent border border-vmm-accent/30'
                    : 'bg-vmm-surface text-vmm-text-muted hover:text-vmm-text border border-vmm-border'
                  }`}
              >
                {SERVICE_LABELS[svc] || svc}
              </button>
            ))}
          </div>

          {/* Log file path */}
          {activeLog && (
            <div className="flex items-center gap-2 text-xs text-vmm-text-muted">
              <FileText size={12} />
              <span className={activeLog.available ? 'text-vmm-success' : 'text-vmm-danger'}>
                {activeLog.log_file}
              </span>
              {!activeLog.available && <span>(not available)</span>}
            </div>
          )}

          {/* Log output */}
          <pre
            ref={scrollRef}
            className="bg-[#0d1117] border border-vmm-border rounded-lg p-3 text-[11px] leading-[1.6] font-mono text-vmm-text-dim overflow-auto max-h-[600px] min-h-[400px] whitespace-pre-wrap break-all"
          >
            {activeLog && activeLog.lines.length > 0
              ? activeLog.lines.join('\n')
              : activeLog && !activeLog.available
                ? 'Log file not found on this host. The service may not be running or logging to a different location.'
                : loading
                  ? 'Loading...'
                  : 'Select a host and service to view logs.'
            }
          </pre>
        </div>
      </div>
    </div>
  )
}
