import { useState } from 'react'
import api from '../api/client'
import Dialog from './Dialog'
import FormField from './FormField'
import TextInput from './TextInput'
import Select from './Select'
import Button from './Button'

const poolTypes = [
  { value: 'local', label: 'Local Directory' },
  { value: 'nfs', label: 'NFS v4' },
  { value: 'cephfs', label: 'CephFS' },
  { value: 'glusterfs', label: 'GlusterFS' },
]

interface Props {
  open: boolean
  onClose: () => void
  onCreated: () => void
}

export default function AddPoolDialog({ open, onClose, onCreated }: Props) {
  const [name, setName] = useState('')
  const [path, setPath] = useState('')
  const [poolType, setPoolType] = useState('local')
  const [mountSource, setMountSource] = useState('')
  const [mountOpts, setMountOpts] = useState('')
  const [error, setError] = useState('')
  const [saving, setSaving] = useState(false)

  const isShared = poolType !== 'local'

  const handleSave = async () => {
    if (!name.trim()) { setError('Name is required'); return }
    if (!path.trim()) { setError('Path is required'); return }
    if (isShared && !mountSource.trim()) { setError('Mount source is required for shared storage'); return }
    setSaving(true)
    setError('')
    try {
      await api.post('/api/storage/pools', {
        name, path, pool_type: poolType,
        mount_source: isShared ? mountSource : undefined,
        mount_opts: mountOpts || undefined,
      })
      onCreated()
      onClose()
      setName(''); setPath(''); setPoolType('local'); setMountSource(''); setMountOpts('')
    } catch (e: any) {
      setError(e.response?.data?.error || 'Failed to create pool')
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={open} onClose={onClose} title="Add Storage Pool">
      <div className="space-y-4">
        <FormField label="Pool Name">
          <TextInput value={name} onChange={(e) => setName(e.target.value)} placeholder="Local-NVMe-P01" />
        </FormField>
        <FormField label="Type">
          <Select options={poolTypes} value={poolType} onChange={(e) => setPoolType(e.target.value)} />
        </FormField>
        <FormField label="Local Mount Path">
          <TextInput value={path} onChange={(e) => setPath(e.target.value)}
            placeholder={isShared ? '/mnt/storage/vm-pool' : '/var/lib/vmm/images'} />
        </FormField>
        {isShared && (
          <>
            <FormField label="Mount Source">
              <TextInput value={mountSource} onChange={(e) => setMountSource(e.target.value)}
                placeholder={poolType === 'nfs' ? '10.0.40.15:/mnt/storage/vm-backups' : 'mon1,mon2:/vm-pool'} />
            </FormField>
            <FormField label="Mount Options (optional)">
              <TextInput value={mountOpts} onChange={(e) => setMountOpts(e.target.value)}
                placeholder="vers=4,noatime" />
            </FormField>
          </>
        )}
        {error && <div className="text-xs text-vmm-danger">{error}</div>}
        <div className="flex justify-end gap-3 pt-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleSave} disabled={saving}>
            {saving ? 'Creating...' : 'Create Pool'}
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
