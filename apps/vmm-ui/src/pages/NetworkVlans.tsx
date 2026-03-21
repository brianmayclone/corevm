/** VLAN Configuration — manage VLAN tags and trunk ports. */
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import Button from '../components/Button'
import { Plus, Layers } from 'lucide-react'

export default function NetworkVlans() {
  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">VLAN Configuration</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            Manage 802.1Q VLAN tags, trunk ports, and traffic segmentation
          </p>
        </div>
        <Button variant="primary" icon={<Plus size={14} />}>Create VLAN</Button>
      </div>

      <Card>
        <div className="flex flex-col items-center justify-center py-16 text-center">
          <div className="w-16 h-16 rounded-2xl bg-vmm-bg-alt flex items-center justify-center mb-4">
            <Layers size={28} className="text-vmm-text-muted" />
          </div>
          <h3 className="text-lg font-semibold text-vmm-text mb-2">No VLANs Configured</h3>
          <p className="text-sm text-vmm-text-muted max-w-md">
            VLANs allow you to segment network traffic for security and performance.
            Create a VLAN to assign virtual machines to isolated network segments.
          </p>
        </div>
      </Card>
    </div>
  )
}
