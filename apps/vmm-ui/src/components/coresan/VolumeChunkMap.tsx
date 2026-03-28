import { useEffect, useState, useMemo, useCallback } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { ArrowLeft, RefreshCw, Grid3x3, Pause, Play } from 'lucide-react'
import type { ChunkMapResponse, ChunkMapEntry } from '../../api/types'
import { useClusterStore } from '../../stores/clusterStore'
import Button from '../Button'
import Card from '../Card'
import { formatBytes } from '../../utils/format'

/** Color for each chunk state */
const stateColor: Record<string, string> = {
  synced: '#22c55e',   // green
  stale: '#eab308',    // yellow
  syncing: '#3b82f6',  // blue
  error: '#ef4444',    // red
  empty: '#1e293b',    // dark slate
}

const stateBorder: Record<string, string> = {
  synced: '#16a34a',
  stale: '#ca8a04',
  syncing: '#2563eb',
  error: '#dc2626',
  empty: '#334155',
}

/** Unique color per node (up to 8 nodes) */
const nodeColors = [
  '#06b6d4', '#8b5cf6', '#f59e0b', '#ec4899',
  '#10b981', '#6366f1', '#f97316', '#14b8a6',
]

export default function VolumeChunkMap() {
  const { volumeId } = useParams<{ volumeId: string }>()
  const navigate = useNavigate()
  const { backendMode, hosts } = useClusterStore()
  const isCluster = backendMode === 'cluster'

  const [data, setData] = useState<ChunkMapResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [live, setLive] = useState(true)
  const [hoveredChunk, setHoveredChunk] = useState<ChunkMapEntry | null>(null)
  const [colorMode, setColorMode] = useState<'state' | 'node' | 'file'>('state')

  const localSanBase = `${window.location.protocol}//${window.location.hostname}:7443`
  const sanApi = (path: string) => isCluster ? `/api/san${path.replace(/^\/api/, '')}` : `${localSanBase}${path}`

  const sanFetch = useCallback((url: string) => {
    if (isCluster) {
      const token = localStorage.getItem('vmm_token')
      const headers: HeadersInit = token ? { Authorization: `Bearer ${token}` } : {}
      return fetch(url, { headers })
    }
    return fetch(url)
  }, [isCluster])

  const fetchData = useCallback(async () => {
    if (!volumeId) return
    try {
      const res = await sanFetch(sanApi(`/api/volumes/${volumeId}/chunk-map`))
      if (!res.ok) {
        setError(`Failed to load chunk map: ${res.status}`)
        return
      }
      const json: ChunkMapResponse = await res.json()
      setData(json)
      setError('')
    } catch (e: any) {
      setError(e.message || 'Failed to fetch')
    } finally {
      setLoading(false)
    }
  }, [volumeId, sanFetch])

  useEffect(() => {
    fetchData()
  }, [fetchData])

  // Live refresh every 3 seconds
  useEffect(() => {
    if (!live) return
    const interval = setInterval(fetchData, 3000)
    return () => clearInterval(interval)
  }, [live, fetchData])

  // Build node color map
  const nodeColorMap = useMemo(() => {
    if (!data) return new Map<string, string>()
    const nodes = [...new Set(data.chunks.map(c => c.node_id))]
    const map = new Map<string, string>()
    nodes.forEach((n, i) => map.set(n, nodeColors[i % nodeColors.length]))
    return map
  }, [data])

  // Build file color map (hash-based)
  const fileColorMap = useMemo(() => {
    if (!data) return new Map<number, string>()
    const files = [...new Set(data.chunks.map(c => c.file_id))]
    const map = new Map<number, string>()
    const palette = [
      '#06b6d4', '#8b5cf6', '#f59e0b', '#ec4899', '#10b981',
      '#6366f1', '#f97316', '#14b8a6', '#a855f7', '#84cc16',
      '#e11d48', '#0891b2', '#7c3aed', '#d97706', '#059669',
    ]
    files.forEach((f, i) => map.set(f, palette[i % palette.length]))
    return map
  }, [data])

  // Group chunks by backend for per-backend grid view
  const chunksByBackend = useMemo(() => {
    if (!data) return new Map<string, ChunkMapEntry[]>()
    const map = new Map<string, ChunkMapEntry[]>()
    for (const c of data.chunks) {
      const key = `${c.node_hostname}:${c.backend_id}`
      if (!map.has(key)) map.set(key, [])
      map.get(key)!.push(c)
    }
    return map
  }, [data])

  // Stats
  const stats = useMemo(() => {
    if (!data) return { synced: 0, stale: 0, error: 0, syncing: 0, total: 0 }
    const s = { synced: 0, stale: 0, error: 0, syncing: 0, total: data.chunks.length }
    for (const c of data.chunks) {
      if (c.state === 'synced') s.synced++
      else if (c.state === 'stale') s.stale++
      else if (c.state === 'error') s.error++
      else if (c.state === 'syncing') s.syncing++
    }
    return s
  }, [data])

  const getChunkColor = (chunk: ChunkMapEntry) => {
    if (colorMode === 'node') return nodeColorMap.get(chunk.node_id) || '#64748b'
    if (colorMode === 'file') return fileColorMap.get(chunk.file_id) || '#64748b'
    return stateColor[chunk.state] || stateColor.empty
  }

  const getChunkBorder = (chunk: ChunkMapEntry) => {
    if (colorMode !== 'state') return 'transparent'
    return stateBorder[chunk.state] || stateBorder.empty
  }

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64 text-vmm-text-muted">
        Loading chunk map...
      </div>
    )
  }

  if (error || !data) {
    return (
      <div className="p-6 space-y-4">
        <Button variant="ghost" onClick={() => navigate(-1)}>
          <ArrowLeft className="w-4 h-4 mr-2" /> Back
        </Button>
        <div className="text-vmm-danger">{error || 'No data available'}</div>
      </div>
    )
  }

  return (
    <div className="p-4 sm:p-6 space-y-4 overflow-auto">
      {/* Header */}
      <div className="flex items-center justify-between flex-wrap gap-3">
        <div className="flex items-center gap-3">
          <Button variant="ghost" onClick={() => navigate(-1)}>
            <ArrowLeft className="w-4 h-4" />
          </Button>
          <div>
            <h2 className="text-lg font-semibold text-vmm-text">
              Allocation Details: {data.volume_name}
            </h2>
            <p className="text-sm text-vmm-text-muted">
              {formatBytes(data.chunk_size_bytes)} chunks &middot; {data.total_chunks} total replicas &middot; {formatBytes(data.used_bytes)} used
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          {/* Color mode selector */}
          <select
            className="text-xs bg-vmm-surface border border-vmm-border rounded px-2 py-1.5 text-vmm-text"
            value={colorMode}
            onChange={e => setColorMode(e.target.value as any)}
          >
            <option value="state">Color: Status</option>
            <option value="node">Color: Node</option>
            <option value="file">Color: File</option>
          </select>
          <Button
            variant={live ? 'primary' : 'outline'}
            size="sm"
            onClick={() => setLive(!live)}
          >
            {live ? <Pause className="w-3.5 h-3.5 mr-1" /> : <Play className="w-3.5 h-3.5 mr-1" />}
            {live ? 'Live' : 'Paused'}
          </Button>
          <Button variant="ghost" size="sm" onClick={fetchData}>
            <RefreshCw className="w-3.5 h-3.5" />
          </Button>
        </div>
      </div>

      {/* Stats bar */}
      <div className="flex items-center gap-4 text-xs flex-wrap">
        <span className="flex items-center gap-1.5">
          <span className="w-3 h-3 rounded-sm" style={{ background: stateColor.synced }} />
          Synced: {stats.synced}
        </span>
        <span className="flex items-center gap-1.5">
          <span className="w-3 h-3 rounded-sm" style={{ background: stateColor.stale }} />
          Stale: {stats.stale}
        </span>
        <span className="flex items-center gap-1.5">
          <span className="w-3 h-3 rounded-sm" style={{ background: stateColor.syncing }} />
          Syncing: {stats.syncing}
        </span>
        <span className="flex items-center gap-1.5">
          <span className="w-3 h-3 rounded-sm" style={{ background: stateColor.error }} />
          Error: {stats.error}
        </span>
        {colorMode === 'node' && (
          <>
            <span className="text-vmm-text-muted ml-2">Nodes:</span>
            {[...nodeColorMap.entries()].map(([nodeId, color]) => {
              const hostname = data.chunks.find(c => c.node_id === nodeId)?.node_hostname || nodeId.slice(0, 8)
              return (
                <span key={nodeId} className="flex items-center gap-1.5">
                  <span className="w-3 h-3 rounded-sm" style={{ background: color }} />
                  {hostname}
                </span>
              )
            })}
          </>
        )}
      </div>

      {/* Chunk grids — one per backend */}
      <div className="space-y-4">
        {data.backends.map(backend => {
          const key = `${backend.node_hostname}:${backend.backend_id}`
          const chunks = chunksByBackend.get(key) || []
          if (chunks.length === 0) return null

          // Sort by file_id then chunk_index for consistent layout
          const sorted = [...chunks].sort((a, b) =>
            a.file_id !== b.file_id ? a.file_id - b.file_id : a.chunk_index - b.chunk_index
          )

          return (
            <Card key={key}>
              <div className="p-3">
                <div className="flex items-center justify-between mb-2">
                  <div className="text-sm font-medium text-vmm-text">
                    <span className="text-vmm-accent">{backend.node_hostname}</span>
                    <span className="text-vmm-text-muted ml-2 text-xs font-normal">
                      {backend.path} &middot; {backend.status}
                    </span>
                  </div>
                  <div className="text-xs text-vmm-text-muted">
                    {formatBytes(backend.total_bytes - backend.free_bytes)} / {formatBytes(backend.total_bytes)}
                    <span className="ml-1 text-vmm-text-dim">
                      ({sorted.length} chunks)
                    </span>
                  </div>
                </div>

                {/* Block grid */}
                <div
                  className="flex flex-wrap gap-[2px]"
                  style={{ minHeight: 24 }}
                >
                  {sorted.map((chunk, i) => (
                    <div
                      key={`${chunk.file_id}-${chunk.chunk_index}-${chunk.backend_id}`}
                      className="rounded-[2px] cursor-pointer transition-transform hover:scale-150 hover:z-10 relative"
                      style={{
                        width: 10,
                        height: 10,
                        background: getChunkColor(chunk),
                        border: `1px solid ${getChunkBorder(chunk)}`,
                        opacity: hoveredChunk?.file_id === chunk.file_id && hoveredChunk?.chunk_index !== chunk.chunk_index
                          ? 0.5
                          : 1,
                      }}
                      onMouseEnter={() => setHoveredChunk(chunk)}
                      onMouseLeave={() => setHoveredChunk(null)}
                      title={`${chunk.rel_path} [${chunk.chunk_index}] — ${chunk.state}`}
                    />
                  ))}
                </div>
              </div>
            </Card>
          )
        })}
      </div>

      {/* Hover detail panel */}
      {hoveredChunk && (
        <div className="fixed bottom-4 right-4 bg-vmm-surface border border-vmm-border rounded-lg shadow-xl p-3 text-xs space-y-1 z-50 min-w-[280px]">
          <div className="font-medium text-vmm-text text-sm mb-1.5">
            Chunk Details
          </div>
          <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5">
            <span className="text-vmm-text-muted">File:</span>
            <span className="text-vmm-text font-mono truncate">{hoveredChunk.rel_path}</span>
            <span className="text-vmm-text-muted">Index:</span>
            <span className="text-vmm-text">{hoveredChunk.chunk_index}</span>
            <span className="text-vmm-text-muted">Size:</span>
            <span className="text-vmm-text">{formatBytes(hoveredChunk.size_bytes)}</span>
            <span className="text-vmm-text-muted">Status:</span>
            <span style={{ color: stateColor[hoveredChunk.state] }} className="font-medium">
              {hoveredChunk.state}
            </span>
            <span className="text-vmm-text-muted">Node:</span>
            <span className="text-vmm-text">{hoveredChunk.node_hostname}</span>
            <span className="text-vmm-text-muted">Backend:</span>
            <span className="text-vmm-text font-mono truncate text-[10px]">{hoveredChunk.backend_path}</span>
            {hoveredChunk.sha256 && (
              <>
                <span className="text-vmm-text-muted">SHA256:</span>
                <span className="text-vmm-text font-mono text-[10px]">{hoveredChunk.sha256.slice(0, 16)}...</span>
              </>
            )}
          </div>
        </div>
      )}
    </div>
  )
}
