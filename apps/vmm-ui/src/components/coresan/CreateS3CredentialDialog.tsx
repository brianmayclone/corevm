import { useState } from 'react'
import { Copy, AlertTriangle } from 'lucide-react'
import api from '../../api/client'
import type { S3CredentialCreateResponse } from '../../api/types'
import Dialog from '../Dialog'
import FormField from '../FormField'
import TextInput from '../TextInput'
import Button from '../Button'

interface Props {
  open: boolean
  onClose: () => void
  onCreated: () => void
  isCluster: boolean
  sanBase: string
}

export default function CreateS3CredentialDialog({ open, onClose, onCreated, isCluster, sanBase }: Props) {
  const [userId, setUserId] = useState('')
  const [displayName, setDisplayName] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const [result, setResult] = useState<S3CredentialCreateResponse | null>(null)
  const [copied, setCopied] = useState('')

  const handleCreate = async () => {
    if (!userId.trim()) { setError('User ID is required'); return }
    setSaving(true)
    setError('')
    try {
      let data: S3CredentialCreateResponse
      if (isCluster) {
        const resp = await api.post<S3CredentialCreateResponse>('/api/san/s3/credentials', { user_id: userId, display_name: displayName })
        data = resp.data
      } else {
        const resp = await fetch(`${sanBase}/api/s3/credentials`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ user_id: userId, display_name: displayName }),
        })
        data = await resp.json()
      }
      setResult(data)
    } catch (e: any) {
      setError(e.response?.data?.error || e.message || 'Failed to create credential')
    } finally {
      setSaving(false)
    }
  }

  const handleCopy = (text: string, label: string) => {
    navigator.clipboard.writeText(text)
    setCopied(label)
    setTimeout(() => setCopied(''), 2000)
  }

  const handleClose = () => {
    if (result) onCreated()
    setUserId('')
    setDisplayName('')
    setError('')
    setResult(null)
    onClose()
  }

  return (
    <Dialog open={open} title="Create S3 Access Key" onClose={handleClose} width="max-w-md">
      {!result ? (
        <div className="space-y-4">
          {error && (
            <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">{error}</div>
          )}
          <FormField label="User ID">
            <TextInput value={userId} onChange={e => setUserId(e.target.value)} placeholder="admin" />
          </FormField>
          <FormField label="Display Name (optional)">
            <TextInput value={displayName} onChange={e => setDisplayName(e.target.value)} placeholder="Backup Service Key" />
          </FormField>
          <div className="flex justify-end gap-2 pt-2">
            <Button variant="ghost" onClick={handleClose}>Cancel</Button>
            <Button onClick={handleCreate} disabled={saving}>
              {saving ? 'Creating...' : 'Create Key'}
            </Button>
          </div>
        </div>
      ) : (
        <div className="space-y-4">
          <div className="bg-yellow-500/10 border border-yellow-500/30 rounded-lg p-3 flex gap-2">
            <AlertTriangle size={16} className="text-yellow-400 shrink-0 mt-0.5" />
            <p className="text-sm text-yellow-300">
              Save the Secret Key now. It will not be shown again.
            </p>
          </div>

          <div>
            <label className="block text-xs text-vmm-muted mb-1">Access Key</label>
            <div className="flex items-center gap-2">
              <code className="flex-1 bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text">{result.access_key}</code>
              <button onClick={() => handleCopy(result.access_key, 'access')}
                className="p-2 rounded hover:bg-vmm-hover text-vmm-muted hover:text-vmm-text">
                <Copy size={14} />
              </button>
              {copied === 'access' && <span className="text-xs text-green-400">Copied</span>}
            </div>
          </div>

          <div>
            <label className="block text-xs text-vmm-muted mb-1">Secret Key</label>
            <div className="flex items-center gap-2">
              <code className="flex-1 bg-vmm-surface-2 rounded px-3 py-2 text-sm font-mono text-vmm-text break-all">{result.secret_key}</code>
              <button onClick={() => handleCopy(result.secret_key, 'secret')}
                className="p-2 rounded hover:bg-vmm-hover text-vmm-muted hover:text-vmm-text">
                <Copy size={14} />
              </button>
              {copied === 'secret' && <span className="text-xs text-green-400">Copied</span>}
            </div>
          </div>

          <div className="flex justify-end pt-2">
            <Button onClick={handleClose}>Done</Button>
          </div>
        </div>
      )}
    </Dialog>
  )
}
