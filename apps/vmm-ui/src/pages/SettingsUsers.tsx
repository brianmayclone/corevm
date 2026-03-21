/** User Management — create, edit, delete users. */
import { useEffect, useState } from 'react'
import { Plus, Trash2, Shield, User as UserIcon, Edit } from 'lucide-react'
import api from '../api/client'
import type { User } from '../api/types'
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

export default function SettingsUsers() {
  const [users, setUsers] = useState<User[]>([])
  const [showCreate, setShowCreate] = useState(false)
  const [deleteUser, setDeleteUser] = useState<User | null>(null)
  const [form, setForm] = useState({ username: '', password: '', role: 'viewer' })

  const refresh = () => api.get<User[]>('/api/users').then(({ data }) => setUsers(data))
  useEffect(() => { refresh() }, [])

  const handleCreate = async () => {
    if (!form.username || !form.password) return
    try {
      await api.post('/api/users', form)
      setShowCreate(false)
      setForm({ username: '', password: '', role: 'viewer' })
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed to create user')
    }
  }

  const handleDelete = async () => {
    if (!deleteUser) return
    try {
      await api.delete(`/api/users/${deleteUser.id}`)
      setDeleteUser(null)
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed to delete user')
    }
  }

  const handleChangePassword = async (user: User) => {
    const newPw = prompt(`New password for "${user.username}":`)
    if (!newPw) return
    try {
      await api.put(`/api/users/${user.id}/password`, { new_password: newPw })
      alert('Password changed.')
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed')
    }
  }

  const handleChangeRole = async (user: User, newRole: string) => {
    try {
      await api.put(`/api/users/${user.id}`, { role: newRole })
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed')
    }
  }

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">User Management</h1>
          <p className="text-sm text-vmm-text-muted mt-1">Manage user accounts and access levels</p>
        </div>
        <Button variant="primary" icon={<Plus size={14} />} onClick={() => setShowCreate(true)}>Add User</Button>
      </div>

      {/* Create form */}
      {showCreate && (
        <Card>
          <SectionLabel className="mb-4">New User</SectionLabel>
          <div className="grid grid-cols-3 gap-4">
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Username</label>
              <input value={form.username} onChange={(e) => setForm({ ...form, username: e.target.value })}
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none"
                placeholder="johndoe" />
            </div>
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Password</label>
              <input type="password" value={form.password} onChange={(e) => setForm({ ...form, password: e.target.value })}
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none" />
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
          </div>
          <div className="flex justify-end gap-3 mt-4">
            <Button variant="ghost" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button variant="primary" onClick={handleCreate}>Create User</Button>
          </div>
        </Card>
      )}

      {/* Users table */}
      <Card padding={false}>
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-vmm-border text-[10px] text-vmm-text-muted uppercase tracking-wider">
              <th className="text-left px-5 py-3">User</th>
              <th className="text-left px-5 py-3">Role</th>
              <th className="text-left px-5 py-3">Created</th>
              <th className="text-right px-5 py-3 w-12"></th>
            </tr>
          </thead>
          <tbody>
            {users.map((u) => (
              <tr key={u.id} className="border-b border-vmm-border last:border-b-0 hover:bg-vmm-surface-hover/50">
                <td className="px-5 py-3">
                  <div className="flex items-center gap-3">
                    <div className="w-8 h-8 rounded-lg bg-vmm-bg-alt flex items-center justify-center">
                      <UserIcon size={14} className="text-vmm-text-muted" />
                    </div>
                    <span className="text-vmm-text font-medium">{u.username}</span>
                  </div>
                </td>
                <td className="px-5 py-3">
                  <span className={`px-2 py-0.5 text-[10px] font-bold tracking-wider rounded border ${roleColors[u.role] || roleColors.viewer}`}>
                    {u.role.toUpperCase()}
                  </span>
                </td>
                <td className="px-5 py-3 text-vmm-text-muted text-xs">{u.created_at}</td>
                <td className="px-5 py-3 text-right">
                  <ContextMenu items={[
                    { label: 'Change Password', icon: <Shield size={14} />, onClick: () => handleChangePassword(u) },
                    ...(u.role !== 'admin' ? [{ label: 'Promote to Admin', icon: <Shield size={14} />, onClick: () => handleChangeRole(u, 'admin') }] : []),
                    ...(u.role !== 'operator' ? [{ label: 'Set Operator', icon: <Edit size={14} />, onClick: () => handleChangeRole(u, 'operator') }] : []),
                    ...(u.role !== 'viewer' ? [{ label: 'Set Viewer', icon: <Edit size={14} />, onClick: () => handleChangeRole(u, 'viewer') }] : []),
                    { label: 'Delete User', icon: <Trash2 size={14} />, danger: true, onClick: () => setDeleteUser(u) },
                  ]} />
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </Card>

      <ConfirmDialog open={!!deleteUser} title="Delete User" danger
        message={`Permanently delete user "${deleteUser?.username}"?`}
        confirmLabel="Delete" onConfirm={handleDelete} onCancel={() => setDeleteUser(null)} />
    </div>
  )
}
