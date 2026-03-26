/**
 * CoreSAN file picker — select a file from a CoreSAN volume.
 * Used in VM Create for selecting ISOs and disk images from CoreSAN storage.
 */
import { useEffect, useState } from 'react'
import { Folder, File, ArrowLeft, Boxes } from 'lucide-react'
import type { CoreSanVolume } from '../api/types'
import Dialog from './Dialog'
import Button from './Button'
import Select from './Select'
import { formatBytes } from '../utils/format'

interface BrowseEntry {
  name: string
  is_dir: boolean
  size_bytes: number
  updated_at: string
}

interface Props {
  open: boolean
  onClose: () => void
  title: string
  filterExt?: string
  onSelect: (fusePath: string) => void
}

const SAN_API = 'http://localhost:7443'

export default function CoreSanFilePicker({ open, onClose, title, filterExt, onSelect }: Props) {
  const [volumes, setVolumes] = useState<CoreSanVolume[]>([])
  const [selectedVolume, setSelectedVolume] = useState('')
  const [currentPath, setCurrentPath] = useState('')
  const [entries, setEntries] = useState<BrowseEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [sanAvailable, setSanAvailable] = useState(true)

  useEffect(() => {
    if (!open) return
    fetch(`${SAN_API}/api/volumes`).then(r => r.json())
      .then((vols: CoreSanVolume[]) => {
        setVolumes(vols)
        if (vols.length > 0 && !selectedVolume) setSelectedVolume(vols[0].id)
        setSanAvailable(true)
      })
      .catch(() => setSanAvailable(false))
  }, [open])

  useEffect(() => {
    if (!selectedVolume || !open) return
    loadDir(currentPath)
  }, [selectedVolume, currentPath, open])

  const loadDir = async (path: string) => {
    setLoading(true)
    try {
      const encoded = path ? `/${encodeURIComponent(path)}` : ''
      const resp = await fetch(`${SAN_API}/api/volumes/${selectedVolume}/browse${encoded}`)
      if (resp.ok) setEntries(await resp.json())
      else setEntries([])
    } catch { setEntries([]) }
    setLoading(false)
  }

  const navigateUp = () => {
    const parts = currentPath.split('/').filter(Boolean)
    parts.pop()
    setCurrentPath(parts.join('/'))
  }

  const selectFile = (name: string) => {
    // Build the FUSE mount path that vmm-server can access
    const volName = volumes.find(v => v.id === selectedVolume)?.name || ''
    const filePath = currentPath ? `${currentPath}/${name}` : name
    const fusePath = `/vmm/san/${volName}/${filePath}`
    onSelect(fusePath)
    onClose()
  }

  if (!open) return null

  const pathParts = currentPath.split('/').filter(Boolean)
  const volName = volumes.find(v => v.id === selectedVolume)?.name

  const filteredEntries = filterExt
    ? entries.filter(e => e.is_dir || e.name.toLowerCase().endsWith(filterExt.toLowerCase()))
    : entries

  return (
    <Dialog open={open} onClose={onClose} title={title} width="max-w-3xl">
      <div className="space-y-3">
        {!sanAvailable ? (
          <div className="text-center py-8 text-sm text-vmm-text-muted">
            CoreSAN is not running. Start vmm-san to browse SAN volumes.
          </div>
        ) : (
          <>
            {/* Volume selector */}
            <div className="flex items-center gap-2">
              <Boxes size={16} className="text-vmm-accent shrink-0" />
              <Select value={selectedVolume} onChange={(e) => { setSelectedVolume(e.target.value); setCurrentPath('') }}
                options={volumes.map(v => ({ value: v.id, label: v.name }))} />
            </div>

            {/* Breadcrumb */}
            <div className="flex items-center gap-1 text-sm">
              {currentPath && (
                <button onClick={navigateUp}
                  className="text-vmm-text-muted hover:text-vmm-text cursor-pointer mr-1">
                  <ArrowLeft size={14} />
                </button>
              )}
              <button onClick={() => setCurrentPath('')}
                className="text-vmm-accent hover:underline cursor-pointer font-medium">
                {volName || '/'}
              </button>
              {pathParts.map((part, i) => (
                <span key={i} className="flex items-center gap-1">
                  <span className="text-vmm-text-muted">/</span>
                  <button onClick={() => setCurrentPath(pathParts.slice(0, i + 1).join('/'))}
                    className="text-vmm-accent hover:underline cursor-pointer">{part}</button>
                </span>
              ))}
            </div>

            {/* File list */}
            <div className="border border-vmm-border rounded-lg overflow-hidden max-h-[40vh] overflow-y-auto">
              {loading ? (
                <div className="text-center py-6 text-sm text-vmm-text-muted">Loading...</div>
              ) : filteredEntries.length === 0 ? (
                <div className="text-center py-6 text-sm text-vmm-text-muted">
                  {entries.length === 0 ? 'Empty directory' : `No ${filterExt || ''} files here`}
                </div>
              ) : (
                <div className="divide-y divide-vmm-border/50">
                  {filteredEntries.map(entry => (
                    <button
                      key={entry.name}
                      onClick={() => entry.is_dir ? setCurrentPath(currentPath ? `${currentPath}/${entry.name}` : entry.name) : selectFile(entry.name)}
                      className="w-full flex items-center gap-3 px-3 py-2.5 text-left hover:bg-vmm-surface-hover transition-colors cursor-pointer"
                    >
                      {entry.is_dir
                        ? <Folder size={14} className="text-vmm-accent shrink-0" />
                        : <File size={14} className="text-vmm-text-muted shrink-0" />}
                      <span className={`text-sm flex-1 truncate ${entry.is_dir ? 'text-vmm-accent' : 'text-vmm-text'}`}>
                        {entry.name}
                      </span>
                      {!entry.is_dir && (
                        <span className="text-xs text-vmm-text-muted shrink-0">{formatBytes(entry.size_bytes)}</span>
                      )}
                    </button>
                  ))}
                </div>
              )}
            </div>
          </>
        )}

        <div className="flex justify-end pt-1">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
        </div>
      </div>
    </Dialog>
  )
}
