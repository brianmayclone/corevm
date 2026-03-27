import type { CoreSanVolume, Host } from '../../api/types'
import Dialog from '../Dialog'
import FormField from '../FormField'
import Select from '../Select'
import Button from '../Button'

interface Props {
  open: boolean
  onClose: () => void
  onSubmit: () => void
  availableHosts: Host[]
  selectedVolume: CoreSanVolume | undefined
  addHostId: string
  setAddHostId: (v: string) => void
  addHostError: string
}

export default function AddHostDialog({
  open, onClose, onSubmit, availableHosts, selectedVolume,
  addHostId, setAddHostId, addHostError,
}: Props) {
  return (
    <Dialog open={open} title="Add Host to CoreSAN" onClose={onClose}>
      <div className="space-y-4">
        <p className="text-sm text-vmm-text-dim">
          Select a cluster host to add to this volume. Storage will be provisioned automatically.
        </p>
        {addHostError && (
          <div className="bg-vmm-danger/10 border border-vmm-danger/30 text-vmm-danger rounded-lg p-3 text-sm">
            {addHostError}
          </div>
        )}
        <FormField label="Host">
          <Select value={addHostId} onChange={(e) => setAddHostId(e.target.value)}
            options={availableHosts.map(h => ({ value: h.id, label: `${h.hostname} (${h.address})` }))} />
        </FormField>
        {selectedVolume && (
          <p className="text-[10px] text-vmm-text-muted">
            Backend will be created at <code className="text-vmm-accent">/vmm/san-data/{selectedVolume.name}</code> on the selected host.
          </p>
        )}
        <div className="flex justify-end gap-2 pt-2">
          <Button variant="ghost" onClick={onClose}>Cancel</Button>
          <Button variant="primary" onClick={onSubmit} disabled={!addHostId}>
            Add Host
          </Button>
        </div>
      </div>
    </Dialog>
  )
}
