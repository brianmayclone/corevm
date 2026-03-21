/** Group Management — create groups with role-based permissions. */
import { useEffect, useState } from 'react'
import { Plus, Trash2, Users, Shield } from 'lucide-react'
import api from '../api/client'
import type { Group } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import Button from '../components/Button'
import ContextMenu from '../components/ContextMenu'
import ConfirmDialog from '../components/ConfirmDialog'

const roleColors: Record<string, string> = {
  admin: 'bg-vmm-danger/20 text-vmm-danger border-vmm-danger/30',
  operator: 'bg-vmm-accent/20 text-vmm-accent border-vmm-accent/30',
  viewer: 'bg-vmm-text-muted/20 text-vmm-text-muted border-vmm-text-muted/30',
}

const roleDescriptions: Record<string, string> = {
  admin: 'Full access — manage VMs, storage, users, and system settings',
  operator: 'Manage VMs and storage — no user/system management',
  viewer: 'Read-only access — view VMs and dashboards only',
}

export default function SettingsGroups() {
  const [groups, setGroups] = useState<Group[]>([])
  const [showCreate, setShowCreate] = useState(false)
  const [deleteGroup, setDeleteGroup] = useState<Group | null>(null)
  const [form, setForm] = useState({ name: '', role: 'viewer', description: '' })

  const refresh = () => api.get<Group[]>('/api/settings/groups').then(({ data }) => setGroups(data))
  useEffect(() => { refresh() }, [])

  const handleCreate = async () => {
    if (!form.name) return
    try {
      await api.post('/api/settings/groups', form)
      setShowCreate(false)
      setForm({ name: '', role: 'viewer', description: '' })
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed')
    }
  }

  const handleDelete = async () => {
    if (!deleteGroup) return
    try {
      await api.delete(`/api/settings/groups/${deleteGroup.id}`)
      setDeleteGroup(null)
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed')
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Group Management</h1>
          <p className="text-sm text-vmm-text-muted mt-1">Role-based access groups for organizing users</p>
        </div>
        <Button variant="primary" icon={<Plus size={14} />} onClick={() => setShowCreate(true)}>Create Group</Button>
      </div>

      {/* Role explanation */}
      <div className="grid grid-cols-3 gap-4">
        {(['admin', 'operator', 'viewer'] as const).map((role) => (
          <Card key={role}>
            <div className="flex items-center gap-2 mb-2">
              <Shield size={16} className={role === 'admin' ? 'text-vmm-danger' : role === 'operator' ? 'text-vmm-accent' : 'text-vmm-text-muted'} />
              <span className={`px-2 py-0.5 text-[10px] font-bold tracking-wider rounded border ${roleColors[role]}`}>
                {role.toUpperCase()}
              </span>
            </div>
            <p className="text-xs text-vmm-text-muted">{roleDescriptions[role]}</p>
          </Card>
        ))}
      </div>

      {/* Create form */}
      {showCreate && (
        <Card>
          <SectionLabel className="mb-4">New Group</SectionLabel>
          <div className="grid grid-cols-3 gap-4">
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Group Name</label>
              <input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })}
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none"
                placeholder="e.g. DevOps Team" />
            </div>
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Role</label>
              <select value={form.role} onChange={(e) => setForm({ ...form, role: e.target.value })}
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none">
                <option value="viewer">Viewer</option>
                <option value="operator">Operator</option>
                <option value="admin">Admin</option>
              </select>
            </div>
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Description</label>
              <input value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })}
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none"
                placeholder="Optional description" />
            </div>
          </div>
          <div className="flex justify-end gap-3 mt-4">
            <Button variant="ghost" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button variant="primary" onClick={handleCreate}>Create Group</Button>
          </div>
        </Card>
      )}

      {/* Groups */}
      {groups.length === 0 && !showCreate ? (
        <Card>
          <div className="flex flex-col items-center justify-center py-12 text-center">
            <Users size={28} className="text-vmm-text-muted mb-3" />
            <h3 className="text-lg font-semibold text-vmm-text mb-2">No Groups</h3>
            <p className="text-sm text-vmm-text-muted">Create groups to organize users and assign role-based permissions.</p>
          </div>
        </Card>
      ) : (
        <Card padding={false}>
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b border-vmm-border text-[10px] text-vmm-text-muted uppercase tracking-wider">
                <th className="text-left px-5 py-3">Group</th>
                <th className="text-left px-5 py-3">Role</th>
                <th className="text-left px-5 py-3">Members</th>
                <th className="text-left px-5 py-3">Description</th>
                <th className="text-right px-5 py-3 w-12"></th>
              </tr>
            </thead>
            <tbody>
              {groups.map((g) => (
                <tr key={g.id} className="border-b border-vmm-border last:border-b-0 hover:bg-vmm-surface-hover/50">
                  <td className="px-5 py-3 text-vmm-text font-medium flex items-center gap-2">
                    <Users size={14} className="text-vmm-text-muted" /> {g.name}
                  </td>
                  <td className="px-5 py-3">
                    <span className={`px-2 py-0.5 text-[10px] font-bold tracking-wider rounded border ${roleColors[g.role] || roleColors.viewer}`}>
                      {g.role.toUpperCase()}
                    </span>
                  </td>
                  <td className="px-5 py-3 text-vmm-text-dim">{g.member_count}</td>
                  <td className="px-5 py-3 text-vmm-text-muted text-xs">{g.description || '—'}</td>
                  <td className="px-5 py-3 text-right">
                    <ContextMenu items={[
                      { label: 'Delete Group', icon: <Trash2 size={14} />, danger: true, onClick: () => setDeleteGroup(g) },
                    ]} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </Card>
      )}

      <ConfirmDialog open={!!deleteGroup} title="Delete Group" danger
        message={`Delete group "${deleteGroup?.name}"? Members will not be deleted.`}
        confirmLabel="Delete" onConfirm={handleDelete} onCancel={() => setDeleteGroup(null)} />
    </div>
  )
}
