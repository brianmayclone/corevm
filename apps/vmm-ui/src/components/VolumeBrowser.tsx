import { useEffect, useState } from 'react'
import { Folder, File, ArrowLeft, Plus, Trash2, Upload, FolderPlus } from 'lucide-react'
import Dialog from './Dialog'
import Button from './Button'
import TextInput from './TextInput'
import FormField from './FormField'
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
  volumeId: string
  volumeName: string
  sanApi: (path: string) => string
}

export default function VolumeBrowser({ open, onClose, volumeId, volumeName, sanApi }: Props) {
  const [currentPath, setCurrentPath] = useState('')
  const [entries, setEntries] = useState<BrowseEntry[]>([])
  const [loading, setLoading] = useState(false)
  const [mkdirOpen, setMkdirOpen] = useState(false)
  const [newDirName, setNewDirName] = useState('')
  const [deleteEntry, setDeleteEntry] = useState<BrowseEntry | null>(null)
  const [uploading, setUploading] = useState(false)

  const loadDir = async (path: string) => {
    setLoading(true)
    try {
      const encodedPath = path ? `/${encodeURIComponent(path)}` : ''
      const resp = await fetch(sanApi(`/api/volumes/${volumeId}/browse${encodedPath}`))
      if (resp.ok) {
        setEntries(await resp.json())
      } else {
        setEntries([])
      }
    } catch {
      setEntries([])
    }
    setLoading(false)
  }

  useEffect(() => {
    if (open) loadDir(currentPath)
  }, [open, currentPath])

  const navigateUp = () => {
    const parts = currentPath.split('/').filter(Boolean)
    parts.pop()
    setCurrentPath(parts.join('/'))
  }

  const navigateInto = (name: string) => {
    setCurrentPath(currentPath ? `${currentPath}/${name}` : name)
  }

  const handleMkdir = async () => {
    if (!newDirName.trim()) return
    const fullPath = currentPath ? `${currentPath}/${newDirName}` : newDirName
    await fetch(sanApi(`/api/volumes/${volumeId}/mkdir`), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ path: fullPath }),
    })
    setMkdirOpen(false)
    setNewDirName('')
    loadDir(currentPath)
  }

  const handleDelete = async () => {
    if (!deleteEntry) return
    const fullPath = currentPath ? `${currentPath}/${deleteEntry.name}` : deleteEntry.name
    await fetch(sanApi(`/api/volumes/${volumeId}/files/${encodeURIComponent(fullPath)}`), {
      method: 'DELETE',
    })
    setDeleteEntry(null)
    loadDir(currentPath)
  }

  const handleUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const files = e.target.files
    if (!files || files.length === 0) return
    setUploading(true)

    for (const file of Array.from(files)) {
      const fullPath = currentPath ? `${currentPath}/${file.name}` : file.name
      const data = await file.arrayBuffer()
      await fetch(sanApi(`/api/volumes/${volumeId}/files/${encodeURIComponent(fullPath)}`), {
        method: 'PUT',
        body: new Uint8Array(data),
      })
    }

    setUploading(false)
    loadDir(currentPath)
    e.target.value = '' // reset input
  }

  if (!open) return null

  const pathParts = currentPath.split('/').filter(Boolean)

  return (
    <Dialog open={open} onClose={onClose} title={`Browse: ${volumeName}`} width="max-w-4xl">
      <div className="space-y-3">
        {/* Breadcrumb + Actions */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-1 text-sm">
            <button onClick={() => setCurrentPath('')}
              className="text-vmm-accent hover:underline cursor-pointer font-medium">/</button>
            {pathParts.map((part, i) => (
              <span key={i} className="flex items-center gap-1">
                <span className="text-vmm-text-muted">/</span>
                <button onClick={() => setCurrentPath(pathParts.slice(0, i + 1).join('/'))}
                  className="text-vmm-accent hover:underline cursor-pointer">{part}</button>
              </span>
            ))}
          </div>
          <div className="flex items-center gap-2">
            {currentPath && (
              <Button variant="ghost" size="sm" onClick={navigateUp}>
                <ArrowLeft size={13} /> Up
              </Button>
            )}
            <Button variant="ghost" size="sm" onClick={() => { setMkdirOpen(true); setNewDirName('') }}>
              <FolderPlus size={13} /> New Folder
            </Button>
            <label className="inline-flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-lg
              bg-vmm-accent hover:bg-vmm-accent-hover text-white transition-colors cursor-pointer">
              <Upload size={13} /> {uploading ? 'Uploading...' : 'Upload'}
              <input type="file" multiple className="hidden" onChange={handleUpload} disabled={uploading} />
            </label>
          </div>
        </div>

        {/* File List */}
        <div className="border border-vmm-border rounded-lg overflow-hidden max-h-[50vh] overflow-y-auto">
          {loading ? (
            <div className="text-center py-8 text-sm text-vmm-text-muted">Loading...</div>
          ) : entries.length === 0 ? (
            <div className="text-center py-8 text-sm text-vmm-text-muted">
              {currentPath ? 'Empty directory' : 'No files in this volume'}
            </div>
          ) : (
            <table className="w-full text-sm">
              <thead>
                <tr className="border-b border-vmm-border bg-vmm-bg/50">
                  <th className="text-left py-2 px-3 text-xs text-vmm-text-muted font-medium">Name</th>
                  <th className="text-right py-2 px-3 text-xs text-vmm-text-muted font-medium w-24">Size</th>
                  <th className="text-right py-2 px-3 text-xs text-vmm-text-muted font-medium w-40">Modified</th>
                  <th className="w-10"></th>
                </tr>
              </thead>
              <tbody>
                {entries.map(entry => (
                  <tr key={entry.name} className="border-b border-vmm-border/50 hover:bg-vmm-surface-hover transition-colors">
                    <td className="py-2 px-3">
                      {entry.is_dir ? (
                        <button onClick={() => navigateInto(entry.name)}
                          className="flex items-center gap-2 text-vmm-accent hover:underline cursor-pointer">
                          <Folder size={14} /> {entry.name}
                        </button>
                      ) : (
                        <span className="flex items-center gap-2 text-vmm-text">
                          <File size={14} className="text-vmm-text-muted" /> {entry.name}
                        </span>
                      )}
                    </td>
                    <td className="text-right py-2 px-3 text-vmm-text-muted text-xs">
                      {entry.is_dir ? '—' : formatBytes(entry.size_bytes)}
                    </td>
                    <td className="text-right py-2 px-3 text-vmm-text-muted text-xs">
                      {entry.updated_at ? new Date(entry.updated_at).toLocaleString() : '—'}
                    </td>
                    <td className="py-2 px-1">
                      {!entry.is_dir && (
                        <button onClick={() => setDeleteEntry(entry)}
                          className="p-1 rounded hover:bg-vmm-danger/10 text-vmm-text-muted hover:text-vmm-danger transition-colors cursor-pointer">
                          <Trash2 size={12} />
                        </button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>

        {/* Mkdir inline dialog */}
        {mkdirOpen && (
          <div className="flex items-center gap-2 p-3 border border-vmm-accent/30 rounded-lg bg-vmm-accent/5">
            <FolderPlus size={16} className="text-vmm-accent shrink-0" />
            <TextInput value={newDirName} onChange={(e) => setNewDirName(e.target.value)}
              placeholder="Folder name" className="flex-1 !py-1.5" />
            <Button variant="primary" size="sm" onClick={handleMkdir} disabled={!newDirName.trim()}>Create</Button>
            <Button variant="ghost" size="sm" onClick={() => setMkdirOpen(false)}>Cancel</Button>
          </div>
        )}

        {/* Delete confirm */}
        {deleteEntry && (
          <div className="flex items-center justify-between p-3 border border-vmm-danger/30 rounded-lg bg-vmm-danger/5">
            <span className="text-sm text-vmm-danger">Delete "{deleteEntry.name}"?</span>
            <div className="flex gap-2">
              <Button variant="danger" size="sm" onClick={handleDelete}>Delete</Button>
              <Button variant="ghost" size="sm" onClick={() => setDeleteEntry(null)}>Cancel</Button>
            </div>
          </div>
        )}
      </div>
    </Dialog>
  )
}
