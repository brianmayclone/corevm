import { useState } from 'react'
import type { Host } from '../../api/types'
import Dialog from '../Dialog'
import FormField from '../FormField'
import TextInput from '../TextInput'
import Select from '../Select'
import Button from '../Button'

interface Props {
  open: boolean
  onClose: () => void
  onCreated: () => void
  isCluster: boolean
  sanHosts: Host[]
  sanFetch: (url: string, init?: RequestInit) => Promise<Response>
  sanApi: (path: string) => string
}

export default function CreateFileDiskDialog({
  open, onClose, onCreated, isCluster, sanHosts, sanFetch, sanApi,
}: Props) {
  const [sizeGb, setSizeGb] = useState(10)
  const [fsType, setFsType] = useState('ext4')
  const [name, setName] = useState('')
  const [hostId, setHostId] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')

  const handleCreate = async () => {
    if (isCluster && !hostId) {
      setError('Select a target host')
      return
    }
    if (sizeGb < 1) {
      setError('Minimum size is 1 GB')
      return
    }

    setSaving(true)
    setError('')

    try {
      const body: any = {
        size_bytes: sizeGb * 1024 * 1024 * 1024,
        fs_type: fsType,
        name: name || undefined,
      }
      if (isCluster) body.host_id = hostId

      const resp = await sanFetch(sanApi(isCluster ? '/api/disks/create-file' : '/api/disks/create-file'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      })

      if (!resp.ok) {
        const text = await resp.text()
        setError(text || 'Failed to create virtual disk')
        return
      }

      onCreated()
      resetForm()
      onClose()
    } catch (e: any) {
      setError(e.message || 'Failed to create virtual disk')
    } finally {
      setSaving(false)
    }
  }

  const resetForm = () => {
    setSizeGb(10)
    setFsType('ext4')
    setName('')
    setHostId('')
    setError('')
  }

  const handleClose = () => {
    resetForm()
    onClose()
  }

  return (
    <Dialog open={open} title="Create Virtual Disk" onClose={handleClose} width="max-w-md">
      <div className="space-y-4">
        {error && (
          <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">{error}</div>
        )}

        <div className="bg-vmm-accent/5 border border-vmm-accent/20 rounded-lg p-3 text-sm text-vmm-muted">
          Creates a sparse file-backed disk for development and testing.
          No physical disk required.
        </div>

        {isCluster && (
          <FormField label="Target Host">
            <Select value={hostId} onChange={e => setHostId(e.target.value)}>
              <option value="">Select host...</option>
              {sanHosts.map(h => (
                <option key={h.id} value={h.id}>{h.hostname} ({h.san_address})</option>
              ))}
            </Select>
          </FormField>
        )}

        <FormField label="Size (GB)">
          <TextInput type="number" value={sizeGb} onChange={e => setSizeGb(Number(e.target.value))} min={1} max={1024} />
        </FormField>

        <FormField label="Filesystem">
          <Select value={fsType} onChange={e => setFsType(e.target.value)}>
            <option value="ext4">ext4</option>
            <option value="xfs">XFS</option>
          </Select>
        </FormField>

        <FormField label="Name (optional)">
          <TextInput value={name} onChange={e => setName(e.target.value)} placeholder="test-disk-1" />
        </FormField>

        <div className="flex justify-end gap-2 pt-2">
          <Button variant="ghost" onClick={handleClose}>Cancel</Button>
          <Button onClick={handleCreate} disabled={saving}>
            {saving ? 'Creating...' : 'Create Virtual Disk'}
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
