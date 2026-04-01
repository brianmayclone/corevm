import { useState } from 'react'
import type { CoreSanVolume } from '../../api/types'
import api from '../../api/client'
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
  sanBase: string
  volumes: CoreSanVolume[]
}

export default function CreateIscsiAclDialog({ open, onClose, onCreated, isCluster, sanBase, volumes }: Props) {
  const [volumeId, setVolumeId] = useState('')
  const [iqn, setIqn] = useState('')
  const [comment, setComment] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)

  const handleSubmit = async () => {
    if (!volumeId || !iqn) return
    if (!iqn.startsWith('iqn.')) {
      setError('Initiator IQN must start with "iqn."')
      return
    }
    setLoading(true)
    setError('')
    try {
      const body = { volume_id: volumeId, initiator_iqn: iqn, comment }
      if (isCluster) {
        await api.post('/api/san/iscsi/acls', body)
      } else {
        const resp = await fetch(`${sanBase}/api/iscsi/acls`, {
          method: 'POST', headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(body),
        })
        if (!resp.ok) throw new Error(await resp.text())
      }
      setVolumeId('')
      setIqn('')
      setComment('')
      onCreated()
    } catch (e: any) {
      setError(e.message || 'Failed to create ACL')
    } finally {
      setLoading(false)
    }
  }

  return (
    <Dialog open={open} title="Add iSCSI Initiator" onClose={onClose} width="max-w-md">
      <div className="space-y-4">
        {error && (
          <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">{error}</div>
        )}
        <FormField label="Volume">
          <Select value={volumeId} onChange={e => setVolumeId(e.target.value)}
            options={[
              { value: '', label: 'Select a volume...' },
              ...volumes.map(v => ({ value: v.id, label: v.name })),
            ]} />
        </FormField>
        <FormField label="Initiator IQN">
          <TextInput value={iqn} onChange={e => setIqn(e.target.value)}
            placeholder="iqn.2024-01.com.example:initiator01" />
          <p className="text-[10px] text-vmm-muted mt-1">
            Find your initiator IQN: <code>cat /etc/iscsi/initiatorname.iscsi</code>
          </p>
        </FormField>
        <FormField label="Comment (optional)">
          <TextInput value={comment} onChange={e => setComment(e.target.value)}
            placeholder="e.g. Production DB server" />
        </FormField>
        <div className="flex justify-end gap-2 pt-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleSubmit} disabled={!volumeId || !iqn || loading}>
            {loading ? 'Adding...' : 'Add Initiator'}
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
