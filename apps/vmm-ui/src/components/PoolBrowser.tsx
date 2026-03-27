import { useEffect, useState } from 'react'
import { Folder, File, HardDrive, Boxes, ArrowLeft } from 'lucide-react'
import api from '../api/client'
import type { StoragePool, PoolFile, CoreSanVolume } from '../api/types'
import Dialog from './Dialog'
import Select from './Select'
import Button from './Button'
import { formatBytes } from '../utils/format'
import { useClusterStore } from '../stores/clusterStore'

interface Props {
  open: boolean
  onClose: () => void
  /** File extension filter (e.g. ".iso", ".raw") */
  filterExt?: string
  /** Called when user selects a file */
  onSelect: (path: string) => void
  title?: string
  /** In cluster mode: only show pools accessible by all hosts in this cluster */
  clusterId?: string
}

type Source =
  | { type: 'pool'; pool: StoragePool }
  | { type: 'san'; volume: CoreSanVolume }

export default function PoolBrowser({ open, onClose, filterExt, onSelect, title, clusterId }: Props) {
  const { backendMode } = useClusterStore()
  const isCluster = backendMode === 'cluster'

  const [sources, setSources] = useState<Source[]>([])
  const [selectedIdx, setSelectedIdx] = useState(0)
  const [files, setFiles] = useState<PoolFile[]>([])
  const [sanEntries, setSanEntries] = useState<{ name: string; is_dir: boolean; size_bytes: number }[]>([])
  const [sanPath, setSanPath] = useState('')
  const [loading, setLoading] = useState(false)

  const sanApi = (path: string) => isCluster ? `/api/san${path.replace(/^\/api/, '')}` : `${window.location.protocol}//${window.location.hostname}:7443${path}`
  const sanFetch = (url: string, init?: RequestInit) => {
    if (isCluster) {
      const token = localStorage.getItem('vmm_token')
      const headers = new Headers(init?.headers)
      if (token) headers.set('Authorization', `Bearer ${token}`)
      return fetch(url, { ...init, headers })
    }
    return fetch(url, init)
  }

  // Load sources on open
  useEffect(() => {
    if (!open) return
    const result: Source[] = []

    const fetchPools = async () => {
      try {
        const params = clusterId ? `?cluster_id=${encodeURIComponent(clusterId)}` : ''
        const { data } = await api.get<StoragePool[]>(`/api/storage/pools${params}`)
        for (const p of data) result.push({ type: 'pool', pool: p })
      } catch { /* no pools */ }
    }

    const fetchSan = async () => {
      try {
        const resp = await sanFetch(sanApi('/api/volumes'))
        const vols: CoreSanVolume[] = await resp.json()
        for (const v of vols.filter(v => v.status === 'online')) {
          result.push({ type: 'san', volume: v })
        }
      } catch { /* SAN not available */ }
    }

    Promise.all([fetchPools(), fetchSan()]).then(() => {
      setSources(result)
      if (result.length > 0 && selectedIdx >= result.length) setSelectedIdx(0)
    })
  }, [open, clusterId])

  // Load files when source changes
  useEffect(() => {
    if (!open || sources.length === 0) return
    const src = sources[selectedIdx]
    if (!src) return

    setLoading(true)
    if (src.type === 'pool') {
      const params = filterExt ? `?ext=${encodeURIComponent(filterExt)}` : ''
      api.get<PoolFile[]>(`/api/storage/pools/${src.pool.id}/browse${params}`)
        .then(({ data }) => setFiles(data))
        .catch(() => setFiles([]))
        .finally(() => setLoading(false))
    } else {
      loadSanDir(src.volume.id, sanPath)
    }
  }, [selectedIdx, sources, open, sanPath])

  const loadSanDir = async (volumeId: string, path: string) => {
    setLoading(true)
    try {
      const encoded = path ? `/${encodeURIComponent(path)}` : ''
      const resp = await sanFetch(sanApi(`/api/volumes/${volumeId}/browse${encoded}`))
      if (resp.ok) {
        let entries = await resp.json()
        if (filterExt) {
          entries = entries.filter((e: any) => e.is_dir || e.name.toLowerCase().endsWith(filterExt.toLowerCase()))
        }
        setSanEntries(entries)
      } else setSanEntries([])
    } catch { setSanEntries([]) }
    setLoading(false)
  }

  const selected = sources[selectedIdx]

  const handleSelect = (path: string) => {
    onSelect(path)
    onClose()
  }

  const handleSanFileClick = (entry: { name: string; is_dir: boolean }) => {
    const src = selected as { type: 'san'; volume: CoreSanVolume }
    if (entry.is_dir) {
      setSanPath(sanPath ? `${sanPath}/${entry.name}` : entry.name)
    } else {
      const filePath = sanPath ? `${sanPath}/${entry.name}` : entry.name
      handleSelect(`/vmm/san/${src.volume.name}/${filePath}`)
    }
  }

  const sanNavigateUp = () => {
    const parts = sanPath.split('/').filter(Boolean)
    parts.pop()
    setSanPath(parts.join('/'))
  }

  return (
    <Dialog open={open} onClose={onClose} title={title || 'Browse Storage'} width="max-w-2xl">
      <div className="space-y-4">
        {/* Source selector */}
        <Select
          options={sources.map((s, i) => ({
            value: String(i),
            label: s.type === 'pool'
              ? `${s.pool.name} (${s.pool.pool_type})`
              : `${s.volume.name} (CoreSAN)`,
          }))}
          value={String(selectedIdx)}
          onChange={(e) => { setSelectedIdx(Number(e.target.value)); setSanPath('') }}
        />

        {/* SAN breadcrumb */}
        {selected?.type === 'san' && sanPath && (
          <div className="flex items-center gap-1 text-sm">
            <button onClick={sanNavigateUp} className="text-vmm-text-muted hover:text-vmm-text cursor-pointer mr-1">
              <ArrowLeft size={14} />
            </button>
            <button onClick={() => setSanPath('')} className="text-vmm-accent hover:underline cursor-pointer font-medium">
              {selected.volume.name}
            </button>
            {sanPath.split('/').filter(Boolean).map((part, i, arr) => (
              <span key={i} className="flex items-center gap-1">
                <span className="text-vmm-text-muted">/</span>
                <button onClick={() => setSanPath(arr.slice(0, i + 1).join('/'))}
                  className="text-vmm-accent hover:underline cursor-pointer">{part}</button>
              </span>
            ))}
          </div>
        )}

        {/* File list */}
        <div className="border border-vmm-border rounded-lg max-h-80 overflow-y-auto bg-vmm-bg-alt">
          {loading ? (
            <div className="text-vmm-text-muted text-sm py-8 text-center">Loading...</div>
          ) : selected?.type === 'pool' ? (
            // Local pool files
            files.length === 0 ? (
              <div className="text-vmm-text-muted text-sm py-8 text-center">No matching files found</div>
            ) : files.map((f, i) => (
              <div key={i}
                onClick={() => !f.is_dir && handleSelect(f.path)}
                className={`flex items-center gap-3 px-4 py-2.5 border-b border-vmm-border last:border-b-0
                  ${f.is_dir ? 'text-vmm-text-muted' : 'hover:bg-vmm-surface-hover cursor-pointer'}`}>
                {f.is_dir ? <Folder size={14} className="text-vmm-accent" /> : <File size={14} className="text-vmm-text-muted" />}
                <span className={`text-sm flex-1 ${f.is_dir ? 'text-vmm-accent font-medium' : 'text-vmm-text'}`}>{f.name}</span>
                {!f.is_dir && <span className="text-xs text-vmm-text-muted">{formatBytes(f.size_bytes)}</span>}
              </div>
            ))
          ) : selected?.type === 'san' ? (
            // CoreSAN volume files
            sanEntries.length === 0 ? (
              <div className="text-vmm-text-muted text-sm py-8 text-center">
                {sanPath ? 'Empty directory' : 'No files yet'}
              </div>
            ) : sanEntries.map((e, i) => (
              <div key={i}
                onClick={() => handleSanFileClick(e)}
                className="flex items-center gap-3 px-4 py-2.5 border-b border-vmm-border last:border-b-0 hover:bg-vmm-surface-hover cursor-pointer">
                {e.is_dir
                  ? <Folder size={14} className="text-vmm-accent" />
                  : <File size={14} className="text-vmm-text-muted" />}
                <span className={`text-sm flex-1 ${e.is_dir ? 'text-vmm-accent font-medium' : 'text-vmm-text'}`}>{e.name}</span>
                {!e.is_dir && <span className="text-xs text-vmm-text-muted">{formatBytes(e.size_bytes)}</span>}
              </div>
            ))
          ) : (
            <div className="text-vmm-text-muted text-sm py-8 text-center">No storage available</div>
          )}
        </div>

        <div className="flex justify-end">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
        </div>
      </div>
    </Dialog>
  )
}
