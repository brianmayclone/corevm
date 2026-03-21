/** Local Storage — manage locally attached storage pools. */
import { useEffect, useState } from 'react'
import { Plus } from 'lucide-react'
import api from '../api/client'
import type { StoragePool } from '../api/types'
import Card from '../components/Card'
import Button from '../components/Button'
import StoragePoolRow from '../components/StoragePoolRow'
import AddPoolDialog from '../components/AddPoolDialog'
import ConfirmDialog from '../components/ConfirmDialog'

export default function StorageLocal() {
  const [pools, setPools] = useState<StoragePool[]>([])
  const [addOpen, setAddOpen] = useState(false)
  const [deletePool, setDeletePool] = useState<StoragePool | null>(null)

  const refresh = () => {
    api.get<StoragePool[]>('/api/storage/pools').then(({ data }) =>
      setPools(data.filter(p => !p.shared))
    )
  }
  useEffect(() => { refresh() }, [])

  const handleDelete = async () => {
    if (!deletePool) return
    try {
      await api.delete(`/api/storage/pools/${deletePool.id}`)
      setDeletePool(null)
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed to delete pool')
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Local Storage</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            Directly attached physical and logical storage pools on this node
          </p>
        </div>
        <Button variant="primary" icon={<Plus size={14} />} onClick={() => setAddOpen(true)}>
          Add Local Pool
        </Button>
      </div>

      {pools.length === 0 ? (
        <Card>
          <div className="text-vmm-text-muted text-sm py-8 text-center">
            No local storage pools configured. Add a pool to get started.
          </div>
        </Card>
      ) : (
        <div className="space-y-3">
          {pools.map((pool) => (
            <StoragePoolRow
              key={pool.id}
              pool={pool}
              onEdit={() => {}}
              onDelete={() => setDeletePool(pool)}
            />
          ))}
        </div>
      )}

      <AddPoolDialog open={addOpen} onClose={() => setAddOpen(false)} onCreated={refresh} />
      <ConfirmDialog
        open={!!deletePool}
        title="Delete Local Pool"
        message={`Delete pool "${deletePool?.name}"? Files on disk will NOT be removed.`}
        confirmLabel="Delete Pool"
        danger
        onConfirm={handleDelete}
        onCancel={() => setDeletePool(null)}
      />
    </div>
  )
}
