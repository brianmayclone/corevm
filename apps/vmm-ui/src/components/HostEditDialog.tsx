import { useState, useEffect } from 'react'
import { Network, Boxes } from 'lucide-react'
import type { Host } from '../api/types'
import api from '../api/client'
import Dialog from './Dialog'
import FormField from './FormField'
import TextInput from './TextInput'
import Select from './Select'
import Button from './Button'

interface Props {
  open: boolean
  onClose: () => void
  host: Host
  onSaved: () => void
}

interface NetworkInterface {
  name: string
  mac: string
  ipv4: string
  state: string
  speed_mbps: number | null
  mtu: number
}

export default function HostEditDialog({ open, onClose, host, onSaved }: Props) {
  const [displayName, setDisplayName] = useState(host.hostname)
  const [sanInterfaces, setSanInterfaces] = useState<NetworkInterface[]>([])
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    if (!open) return
    setDisplayName(host.hostname)

    // Load network interfaces from CoreSAN if active
    if (host.san_enabled && host.san_address) {
      fetch(`${host.san_address}/api/network/interfaces`)
        .then(r => r.json())
        .then(setSanInterfaces)
        .catch(() => setSanInterfaces([]))
    }
  }, [open, host])

  const handleSave = async () => {
    setSaving(true)
    try {
      await api.put(`/api/hosts/${host.id}/rename`, { display_name: displayName })
      onSaved()
      onClose()
    } catch (e: any) {
      alert(e.response?.data?.error || 'Failed to save')
    } finally {
      setSaving(false)
    }
  }

  return (
    <Dialog open={open} title="Edit Host" onClose={onClose} width="max-w-xl">
      <div className="space-y-5">
        <FormField label="Display Name">
          <TextInput value={displayName} onChange={(e) => setDisplayName(e.target.value)}
            placeholder={host.hostname} />
          <p className="text-[10px] text-vmm-text-muted mt-1">
            Custom display name for this host in the cluster UI.
          </p>
        </FormField>

        {/* Management Network */}
        <div className="border-t border-vmm-border pt-4">
          <h3 className="text-xs font-bold text-vmm-text-muted uppercase tracking-wider mb-3 flex items-center gap-2">
            <Network size={14} /> Management Network
          </h3>
          <div className="p-3 rounded-lg bg-vmm-bg/50 border border-vmm-border space-y-1">
            <div className="flex items-center justify-between text-sm">
              <span className="text-vmm-text-muted">Address</span>
              <span className="text-vmm-text font-mono">{host.address}</span>
            </div>
            <div className="flex items-center justify-between text-sm">
              <span className="text-vmm-text-muted">Status</span>
              <span className={host.status === 'online' ? 'text-vmm-success' : 'text-vmm-danger'}>{host.status}</span>
            </div>
            <div className="flex items-center justify-between text-sm">
              <span className="text-vmm-text-muted">Version</span>
              <span className="text-vmm-text">v{host.version}</span>
            </div>
          </div>
        </div>

        {/* CoreSAN Network */}
        {host.san_enabled && (
          <div className="border-t border-vmm-border pt-4">
            <h3 className="text-xs font-bold text-vmm-text-muted uppercase tracking-wider mb-3 flex items-center gap-2">
              <Boxes size={14} /> CoreSAN Storage Network
            </h3>
            <div className="p-3 rounded-lg bg-vmm-bg/50 border border-vmm-border space-y-1 mb-3">
              <div className="flex items-center justify-between text-sm">
                <span className="text-vmm-text-muted">SAN Address</span>
                <span className="text-vmm-text font-mono">{host.san_address || 'auto'}</span>
              </div>
              <div className="flex items-center justify-between text-sm">
                <span className="text-vmm-text-muted">Volumes</span>
                <span className="text-vmm-text">{host.san_volumes}</span>
              </div>
              <div className="flex items-center justify-between text-sm">
                <span className="text-vmm-text-muted">Peers</span>
                <span className="text-vmm-text">{host.san_peers}</span>
              </div>
            </div>

            {sanInterfaces.length > 0 && (
              <>
                <p className="text-xs text-vmm-text-muted mb-2">
                  Available network interfaces for SAN traffic:
                </p>
                <div className="space-y-1.5">
                  {sanInterfaces.map(nic => (
                    <div key={nic.name} className="flex items-center justify-between p-2.5 rounded-lg border border-vmm-border bg-vmm-bg/30 text-xs">
                      <div className="flex items-center gap-2">
                        <Network size={12} className={nic.state === 'up' ? 'text-vmm-success' : 'text-vmm-text-muted'} />
                        <span className="font-mono font-medium text-vmm-text">{nic.name}</span>
                        <span className="text-vmm-text-muted">{nic.mac}</span>
                      </div>
                      <div className="flex items-center gap-3">
                        <span className="text-vmm-text">{nic.ipv4 || 'no IP'}</span>
                        {nic.speed_mbps && <span className="text-vmm-text-muted">{nic.speed_mbps > 1000 ? `${nic.speed_mbps / 1000}G` : `${nic.speed_mbps}M`}</span>}
                        <span className="text-vmm-text-muted">MTU {nic.mtu}</span>
                      </div>
                    </div>
                  ))}
                </div>
              </>
            )}
          </div>
        )}

        <div className="flex justify-end gap-2 pt-2 border-t border-vmm-border">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={handleSave} disabled={saving}>
            {saving ? 'Saving...' : 'Save Changes'}
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
