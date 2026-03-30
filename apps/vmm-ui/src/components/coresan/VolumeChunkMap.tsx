import { useEffect, useState, useMemo, useCallback } from 'react'
import { useParams, useNavigate } from 'react-router-dom'
import { ArrowLeft, RefreshCw, Pause, Play } from 'lucide-react'
import type { ChunkMapResponse, ChunkMapEntry } from '../../api/types'
import { useClusterStore } from '../../stores/clusterStore'
import Button from '../Button'
import Card from '../Card'
import { formatBytes } from '../../utils/format'

/** Health of a logical chunk based on how many nodes have it */
type ChunkHealth = 'protected' | 'degraded' | 'lost' | 'empty'

interface ConsolidatedChunk {
  file_id: number
  chunk_index: number
  rel_path: string
  size_bytes: number
  sha256: string
  health: ChunkHealth
  nodes: { node_id: string; hostname: string; state: string }[]
}

const healthColor: Record<ChunkHealth, string> = {
  protected: '#22c55e', // green — on enough nodes
  degraded: '#eab308',  // yellow — exists but under-replicated
  lost: '#ef4444',      // red — zero synced copies
  empty: '#1e293b',     // dark — unallocated space
}

const healthBorder: Record<ChunkHealth, string> = {
  protected: '#16a34a',
  degraded: '#ca8a04',
  lost: '#dc2626',
  empty: '#334155',
}

const healthLabels: Record<ChunkHealth, string> = {
  protected: 'Protected',
  degraded: 'Degraded',
  lost: 'Lost',
  empty: 'Free',
}

export default function VolumeChunkMap() {
  const { volumeId } = useParams<{ volumeId: string }>()
  const navigate = useNavigate()
  const { backendMode } = useClusterStore()
  const isCluster = backendMode === 'cluster'

  const [data, setData] = useState<ChunkMapResponse | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [live, setLive] = useState(true)
  const [hoveredChunk, setHoveredChunk] = useState<ConsolidatedChunk | null>(null)

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

  useEffect(() => { fetchData() }, [fetchData])

  useEffect(() => {
    if (!live) return
    const interval = setInterval(fetchData, 3000)
    return () => clearInterval(interval)
  }, [live, fetchData])

  // Get FTT from the volume (from any volume status that has it)
  const ftt = useMemo(() => {
    // Try to extract from chunk data — count distinct nodes for any chunk
    if (!data || data.chunks.length === 0) return 1
    // Heuristic: look at the max node count across chunks
    return 1 // Default to FTT=1
  }, [data])

  // Count unique nodes in the cluster
  const nodeCount = useMemo(() => {
    if (!data) return 0
    return new Set(data.chunks.filter(c => c.node_id).map(c => c.node_id)).size
  }, [data])

  // Consolidate: group all replicas by (file_id, chunk_index) into logical chunks
  const consolidated = useMemo(() => {
    if (!data) return [] as ConsolidatedChunk[]

    const map = new Map<string, ConsolidatedChunk>()

    for (const c of data.chunks) {
      const key = `${c.file_id}:${c.chunk_index}`
      if (!map.has(key)) {
        map.set(key, {
          file_id: c.file_id,
          chunk_index: c.chunk_index,
          rel_path: c.rel_path,
          size_bytes: c.size_bytes,
          sha256: c.sha256,
          health: 'lost',
          nodes: [],
        })
      }
      const entry = map.get(key)!
      // Deduplicate nodes (same node may appear multiple times for mirror backends)
      if (!entry.nodes.find(n => n.node_id === c.node_id)) {
        entry.nodes.push({
          node_id: c.node_id,
          hostname: c.node_hostname,
          state: c.state,
        })
      }
    }

    // Compute health based on synced node count vs FTT
    const required = ftt + 1
    for (const chunk of map.values()) {
      const syncedNodes = chunk.nodes.filter(n => n.state === 'synced').length
      if (syncedNodes >= required) {
        chunk.health = 'protected'
      } else if (syncedNodes > 0) {
        chunk.health = 'degraded'
      } else {
        chunk.health = 'lost'
      }
    }

    // Sort by file_id then chunk_index
    return [...map.values()].sort((a, b) =>
      a.file_id !== b.file_id ? a.file_id - b.file_id : a.chunk_index - b.chunk_index
    )
  }, [data, ftt])

  // Calculate total volume capacity in chunks and add "empty" slots
  const totalSlots = useMemo(() => {
    if (!data || !data.total_capacity_bytes || !data.chunk_size_bytes) return consolidated.length
    // Rough estimate: total capacity / chunk size
    return Math.max(consolidated.length, Math.floor(data.total_capacity_bytes / data.chunk_size_bytes / Math.max(nodeCount, 1)))
  }, [data, consolidated, nodeCount])

  const emptySlots = Math.max(0, totalSlots - consolidated.length)

  // Stats
  const stats = useMemo(() => {
    const s = { protected: 0, degraded: 0, lost: 0, empty: emptySlots }
    for (const c of consolidated) {
      s[c.health]++
    }
    return s
  }, [consolidated, emptySlots])

  if (loading) {
    return <div className="flex items-center justify-center h-64 text-vmm-text-muted">Loading chunk map...</div>
  }

  if (error || !data) {
    return (
      <div className="p-6 space-y-4">
        <Button variant="ghost" onClick={() => navigate(-1)}><ArrowLeft className="w-4 h-4 mr-2" /> Back</Button>
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
              {formatBytes(data.chunk_size_bytes)} chunks &middot; {consolidated.length} used &middot; {formatBytes(data.used_bytes)} stored &middot; {nodeCount} node{nodeCount !== 1 ? 's' : ''}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Button variant={live ? 'primary' : 'outline'} size="sm" onClick={() => setLive(!live)}>
            {live ? <Pause className="w-3.5 h-3.5 mr-1" /> : <Play className="w-3.5 h-3.5 mr-1" />}
            {live ? 'Live' : 'Paused'}
          </Button>
          <Button variant="ghost" size="sm" onClick={fetchData}>
            <RefreshCw className="w-3.5 h-3.5" />
          </Button>
        </div>
      </div>

      {/* Legend */}
      <div className="flex items-center gap-5 text-xs flex-wrap">
        {(['protected', 'degraded', 'lost', 'empty'] as ChunkHealth[]).map(h => (
          <span key={h} className="flex items-center gap-1.5">
            <span className="w-3 h-3 rounded-sm" style={{ background: healthColor[h], border: `1px solid ${healthBorder[h]}` }} />
            {healthLabels[h]}: {stats[h]}
          </span>
        ))}
      </div>

      {/* Defrag-style block grid */}
      <Card>
        <div className="p-4">
          <div className="flex flex-wrap gap-[1px]" style={{ minHeight: 40 }}>
            {/* Used chunks — block size scales down for large volumes */}
            {consolidated.map((chunk) => {
              const totalBlocks = consolidated.length + emptySlots
              const blockSize = totalBlocks > 5000 ? 4 : totalBlocks > 2000 ? 6 : totalBlocks > 500 ? 8 : 12
              return (
              <div
                key={`${chunk.file_id}-${chunk.chunk_index}`}
                className="cursor-pointer transition-all duration-100"
                style={{
                  width: blockSize,
                  height: blockSize,
                  borderRadius: blockSize > 6 ? 2 : 1,
                  background: healthColor[chunk.health],
                  border: `1px solid ${healthBorder[chunk.health]}`,
                  opacity: hoveredChunk && hoveredChunk.file_id === chunk.file_id && hoveredChunk.chunk_index !== chunk.chunk_index
                    ? 0.4 : 1,
                  transform: hoveredChunk?.file_id === chunk.file_id && hoveredChunk?.chunk_index === chunk.chunk_index
                    ? 'scale(1.6)' : 'scale(1)',
                  zIndex: hoveredChunk?.file_id === chunk.file_id && hoveredChunk?.chunk_index === chunk.chunk_index
                    ? 10 : 1,
                }}
                onMouseEnter={() => setHoveredChunk(chunk)}
                onMouseLeave={() => setHoveredChunk(null)}
              />
            )})}
            {/* Empty/free slots */}
            {Array.from({ length: Math.min(emptySlots, 5000) }, (_, i) => {
              const totalBlocks = consolidated.length + emptySlots
              const blockSize = totalBlocks > 5000 ? 4 : totalBlocks > 2000 ? 6 : totalBlocks > 500 ? 8 : 12
              return (
              <div
                key={`empty-${i}`}
                style={{
                  width: blockSize,
                  height: blockSize,
                  borderRadius: blockSize > 6 ? 2 : 1,
                  background: healthColor.empty,
                  border: `1px solid ${healthBorder.empty}`,
                }}
              />
            )})}
          </div>
        </div>
      </Card>

      {/* Hover detail panel — fixed bottom-right like Windows defrag */}
      {hoveredChunk && (
        <div className="fixed bottom-4 right-4 bg-vmm-surface border border-vmm-border rounded-lg shadow-xl p-4 text-xs z-50 min-w-[300px] max-w-[400px]">
          <div className="font-medium text-vmm-text text-sm mb-2 flex items-center gap-2">
            <span className="w-2.5 h-2.5 rounded-sm inline-block"
              style={{ background: healthColor[hoveredChunk.health] }} />
            Chunk Details
            <span className="text-vmm-text-muted font-normal ml-auto">
              {healthLabels[hoveredChunk.health]}
            </span>
          </div>

          <div className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1 mb-3">
            <span className="text-vmm-text-muted">File:</span>
            <span className="text-vmm-text font-mono truncate">{hoveredChunk.rel_path}</span>
            <span className="text-vmm-text-muted">Chunk:</span>
            <span className="text-vmm-text">{hoveredChunk.chunk_index} of {
              consolidated.filter(c => c.file_id === hoveredChunk.file_id).length
            }</span>
            <span className="text-vmm-text-muted">Size:</span>
            <span className="text-vmm-text">{formatBytes(hoveredChunk.size_bytes)}</span>
            {hoveredChunk.sha256 && (
              <>
                <span className="text-vmm-text-muted">SHA256:</span>
                <span className="text-vmm-text font-mono text-[10px]">{hoveredChunk.sha256.slice(0, 16)}...</span>
              </>
            )}
          </div>

          {/* Node availability table */}
          <div className="text-[11px] font-medium text-vmm-text-muted mb-1">
            Available on {hoveredChunk.nodes.length} node{hoveredChunk.nodes.length !== 1 ? 's' : ''}:
          </div>
          <div className="space-y-1">
            {hoveredChunk.nodes.map(n => (
              <div key={n.node_id} className="flex items-center gap-2 py-0.5">
                <span className="w-2 h-2 rounded-full"
                  style={{
                    background: n.state === 'synced' ? '#22c55e' :
                      n.state === 'stale' ? '#eab308' : '#ef4444'
                  }} />
                <span className="text-vmm-text">{n.hostname}</span>
                <span className="text-vmm-text-muted ml-auto">{n.state}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
