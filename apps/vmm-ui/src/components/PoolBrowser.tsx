import { useEffect, useState } from 'react'
import { Folder, File, HardDrive } from 'lucide-react'
import api from '../api/client'
import type { StoragePool, PoolFile } from '../api/types'
import Dialog from './Dialog'
import Select from './Select'
import Button from './Button'
import { formatBytes } from '../utils/format'

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

export default function PoolBrowser({ open, onClose, filterExt, onSelect, title, clusterId }: Props) {
  const [pools, setPools] = useState<StoragePool[]>([])
  const [selectedPool, setSelectedPool] = useState<number | string | null>(null)
  const [files, setFiles] = useState<PoolFile[]>([])
  const [loading, setLoading] = useState(false)

  useEffect(() => {
    if (open) {
      const params = clusterId ? `?cluster_id=${encodeURIComponent(clusterId)}` : ''
      api.get<StoragePool[]>(`/api/storage/pools${params}`).then(({ data }) => {
        setPools(data)
        if (data.length > 0 && !selectedPool) setSelectedPool(data[0].id)
      })
    }
  }, [open, clusterId])

  useEffect(() => {
    if (selectedPool) {
      setLoading(true)
      const params = filterExt ? `?ext=${encodeURIComponent(filterExt)}` : ''
      api.get<PoolFile[]>(`/api/storage/pools/${selectedPool}/browse${params}`)
        .then(({ data }) => setFiles(data))
        .finally(() => setLoading(false))
    }
  }, [selectedPool, filterExt])

  const handleSelect = (path: string) => {
    onSelect(path)
    onClose()
  }

  return (
    <Dialog open={open} onClose={onClose} title={title || 'Browse Storage Pool'} width="max-w-2xl">
      <div className="space-y-4">
        {/* Pool selector */}
        <Select
          options={pools.map((p) => ({ value: String(p.id), label: `${p.name} (${p.pool_type})` }))}
          value={selectedPool ? String(selectedPool) : ''}
          onChange={(e) => setSelectedPool(parseInt(e.target.value))}
        />

        {/* File list */}
        <div className="border border-vmm-border rounded-lg max-h-80 overflow-y-auto bg-vmm-bg-alt">
          {loading ? (
            <div className="text-vmm-text-muted text-sm py-8 text-center">Loading...</div>
          ) : files.length === 0 ? (
            <div className="text-vmm-text-muted text-sm py-8 text-center">
              {selectedPool ? 'No matching files found' : 'Select a storage pool'}
            </div>
          ) : (
            files.map((f, i) => (
              <div key={i}
                onClick={() => !f.is_dir && handleSelect(f.path)}
                className={`flex items-center gap-3 px-4 py-2.5 border-b border-vmm-border last:border-b-0
                  ${f.is_dir ? 'text-vmm-text-muted' : 'hover:bg-vmm-surface-hover cursor-pointer'}`}
              >
                {f.is_dir ? <Folder size={14} className="text-vmm-accent" /> : <File size={14} className="text-vmm-text-muted" />}
                <span className={`text-sm flex-1 ${f.is_dir ? 'text-vmm-accent font-medium' : 'text-vmm-text'}`}>
                  {f.name}
                </span>
                {!f.is_dir && (
                  <span className="text-xs text-vmm-text-muted">{formatBytes(f.size_bytes)}</span>
                )}
              </div>
            ))
          )}
        </div>

        <div className="flex justify-end">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
        </div>
      </div>
    </Dialog>
  )
}
