/** Resource Groups — manage VM groupings with granular per-group permissions. */
import { useEffect, useState } from 'react'
import { Plus, Trash2, Shield, FolderOpen, Users, Settings, ChevronDown, ChevronRight, Check } from 'lucide-react'
import api from '../api/client'
import type { ResourceGroup, Group, PermissionsList } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import Button from '../components/Button'
import ContextMenu from '../components/ContextMenu'
import ConfirmDialog from '../components/ConfirmDialog'

const permLabels: Record<string, string> = {
  'vm.create': 'Create VMs',
  'vm.edit': 'Edit VM Settings',
  'vm.delete': 'Delete VMs',
  'vm.start_stop': 'Start / Stop VMs',
  'vm.console': 'Access Console',
  'network.edit': 'Modify Network',
  'storage.edit': 'Modify Storage',
  'snapshots.manage': 'Manage Snapshots',
}

export default function ResourceGroups() {
  const [groups, setGroups] = useState<ResourceGroup[]>([])
  const [userGroups, setUserGroups] = useState<Group[]>([])
  const [permsList, setPermsList] = useState<PermissionsList | null>(null)
  const [showCreate, setShowCreate] = useState(false)
  const [deleteGroup, setDeleteGroup] = useState<ResourceGroup | null>(null)
  const [editPerms, setEditPerms] = useState<{ rg: ResourceGroup; groupId: number; perms: string[] } | null>(null)
  const [addPermRg, setAddPermRg] = useState<ResourceGroup | null>(null)
  const [addPermGroupId, setAddPermGroupId] = useState<number>(0)
  const [expandedId, setExpandedId] = useState<number | null>(null)
  const [form, setForm] = useState({ name: '', description: '' })

  const refresh = () => {
    api.get<ResourceGroup[]>('/api/resource-groups').then(({ data }) => setGroups(data))
    api.get<Group[]>('/api/settings/groups').then(({ data }) => setUserGroups(data))
    api.get<PermissionsList>('/api/resource-groups/permissions-list').then(({ data }) => setPermsList(data))
  }
  useEffect(() => { refresh() }, [])

  const handleCreate = async () => {
    if (!form.name.trim()) return
    try {
      await api.post('/api/resource-groups', form)
      setShowCreate(false)
      setForm({ name: '', description: '' })
      refresh()
    } catch (err: any) { alert(err?.response?.data?.error || 'Failed') }
  }

  const handleDelete = async () => {
    if (!deleteGroup) return
    try {
      await api.delete(`/api/resource-groups/${deleteGroup.id}`)
      setDeleteGroup(null)
      refresh()
    } catch (err: any) { alert(err?.response?.data?.error || 'Failed') }
  }

  const handleSetPerms = async (rgId: number, groupId: number, perms: string[]) => {
    try {
      await api.post(`/api/resource-groups/${rgId}/permissions`, { group_id: groupId, permissions: perms })
      refresh()
    } catch (err: any) { alert(err?.response?.data?.error || 'Failed') }
  }

  const handleRemovePerms = async (rgId: number, groupId: number) => {
    try {
      await api.delete(`/api/resource-groups/${rgId}/permissions`, { data: { group_id: groupId } })
      refresh()
    } catch (err: any) { alert(err?.response?.data?.error || 'Failed') }
  }

  const handleAddPermGroup = async () => {
    if (!addPermRg || !addPermGroupId) return
    await handleSetPerms(addPermRg.id, addPermGroupId, ['vm.console'])
    setAddPermRg(null)
    setAddPermGroupId(0)
  }

  const togglePerm = (perm: string) => {
    if (!editPerms) return
    const newPerms = editPerms.perms.includes(perm)
      ? editPerms.perms.filter(p => p !== perm)
      : [...editPerms.perms, perm]
    setEditPerms({ ...editPerms, perms: newPerms })
  }

  const saveEditPerms = async () => {
    if (!editPerms) return
    await handleSetPerms(editPerms.rg.id, editPerms.groupId, editPerms.perms)
    setEditPerms(null)
  }

  return (
    <div className="space-y-6">
      <div className="flex items-start justify-between">
        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Resource Groups</h1>
          <p className="text-sm text-vmm-text-muted mt-1">
            Organize VMs into groups and assign granular permissions per user-group
          </p>
        </div>
        <Button variant="primary" icon={<Plus size={14} />} onClick={() => setShowCreate(true)}>Create Group</Button>
      </div>

      {/* Create form */}
      {showCreate && (
        <Card>
          <SectionLabel className="mb-4">New Resource Group</SectionLabel>
          <div className="grid grid-cols-2 gap-4">
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Name</label>
              <input value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })}
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none"
                placeholder="e.g. Development Cluster" />
            </div>
            <div>
              <label className="text-[10px] text-vmm-text-muted uppercase tracking-wider block mb-1">Description</label>
              <input value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })}
                className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none"
                placeholder="Optional" />
            </div>
          </div>
          <div className="flex justify-end gap-3 mt-4">
            <Button variant="ghost" onClick={() => setShowCreate(false)}>Cancel</Button>
            <Button variant="primary" onClick={handleCreate}>Create</Button>
          </div>
        </Card>
      )}

      {/* Permission editor dialog */}
      {editPerms && (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm" onClick={() => setEditPerms(null)}>
          <div className="bg-vmm-surface border border-vmm-border rounded-xl p-6 w-full max-w-lg" onClick={(e) => e.stopPropagation()}>
            <h3 className="text-lg font-bold text-vmm-text mb-1">Edit Permissions</h3>
            <p className="text-sm text-vmm-text-muted mb-4">
              {editPerms.rg.name} → {userGroups.find(g => g.id === editPerms.groupId)?.name}
            </p>
            {permsList && Object.entries(permsList.categories).map(([cat, perms]) => (
              <div key={cat} className="mb-4">
                <div className="text-[10px] text-vmm-text-muted uppercase tracking-wider mb-2">{cat}</div>
                <div className="space-y-1">
                  {perms.map((p) => (
                    <label key={p} className="flex items-center gap-3 px-3 py-2 rounded-lg hover:bg-vmm-surface-hover cursor-pointer">
                      <input type="checkbox" checked={editPerms.perms.includes(p)}
                        onChange={() => togglePerm(p)}
                        className="w-4 h-4 rounded accent-vmm-accent" />
                      <span className="text-sm text-vmm-text">{permLabels[p] || p}</span>
                    </label>
                  ))}
                </div>
              </div>
            ))}
            <div className="flex justify-end gap-3 mt-4">
              <Button variant="ghost" onClick={() => setEditPerms(null)}>Cancel</Button>
              <Button variant="primary" onClick={saveEditPerms}>Save Permissions</Button>
            </div>
          </div>
        </div>
      )}

      {/* Resource groups list */}
      <div className="space-y-3">
        {groups.map((rg) => {
          const isExpanded = expandedId === rg.id
          return (
            <Card key={rg.id} padding={false}>
              {/* Header */}
              <div
                className="flex items-center justify-between px-5 py-4 cursor-pointer hover:bg-vmm-surface-hover/30 transition-colors"
                onClick={() => setExpandedId(isExpanded ? null : rg.id)}
              >
                <div className="flex items-center gap-3">
                  {isExpanded ? <ChevronDown size={16} className="text-vmm-text-muted" /> : <ChevronRight size={16} className="text-vmm-text-muted" />}
                  <FolderOpen size={18} className="text-vmm-accent" />
                  <div>
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-semibold text-vmm-text">{rg.name}</span>
                      {rg.is_default && (
                        <span className="px-1.5 py-0.5 text-[9px] font-bold tracking-wider rounded bg-vmm-accent/20 text-vmm-accent border border-vmm-accent/30">DEFAULT</span>
                      )}
                    </div>
                    {rg.description && <div className="text-xs text-vmm-text-muted mt-0.5">{rg.description}</div>}
                  </div>
                </div>
                <div className="flex items-center gap-4" onClick={(e) => e.stopPropagation()}>
                  <div className="text-right">
                    <div className="text-sm font-medium text-vmm-text">{rg.vm_count} VMs</div>
                    <div className="text-[10px] text-vmm-text-muted">{rg.permissions.length} group rule(s)</div>
                  </div>
                  {!rg.is_default && (
                    <ContextMenu items={[
                      { label: 'Delete Group', icon: <Trash2 size={14} />, danger: true, onClick: () => setDeleteGroup(rg) },
                    ]} />
                  )}
                </div>
              </div>

              {/* Expanded: permissions */}
              {isExpanded && (
                <div className="border-t border-vmm-border px-5 py-4">
                  <div className="flex items-center justify-between mb-3">
                    <SectionLabel>Access Control</SectionLabel>
                    <Button variant="outline" size="sm" icon={<Plus size={12} />}
                      onClick={() => { setAddPermRg(rg); setAddPermGroupId(0) }}>
                      Add User Group
                    </Button>
                  </div>

                  {/* Add group selector */}
                  {addPermRg?.id === rg.id && (
                    <div className="flex items-center gap-3 mb-3 bg-vmm-bg-alt rounded-lg p-3">
                      <select value={addPermGroupId} onChange={(e) => setAddPermGroupId(parseInt(e.target.value))}
                        className="flex-1 bg-vmm-surface border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none">
                        <option value={0}>Select user group...</option>
                        {userGroups.filter(ug => !rg.permissions.some(p => p.group_id === ug.id)).map(ug => (
                          <option key={ug.id} value={ug.id}>{ug.name} ({ug.role})</option>
                        ))}
                      </select>
                      <Button variant="primary" size="sm" onClick={handleAddPermGroup} disabled={!addPermGroupId}>Add</Button>
                      <Button variant="ghost" size="sm" onClick={() => setAddPermRg(null)}>Cancel</Button>
                    </div>
                  )}

                  {rg.permissions.length === 0 ? (
                    <div className="text-sm text-vmm-text-muted py-4 text-center">
                      No user groups assigned. Add a group to grant permissions.
                    </div>
                  ) : (
                    <div className="space-y-2">
                      {rg.permissions.map((perm) => (
                        <div key={perm.id} className="flex items-center justify-between bg-vmm-bg-alt rounded-lg px-4 py-3">
                          <div className="flex items-center gap-3">
                            <Users size={16} className="text-vmm-accent" />
                            <div>
                              <div className="text-sm font-medium text-vmm-text">{perm.group_name}</div>
                              <div className="flex flex-wrap gap-1.5 mt-1">
                                {perm.permissions.map((p) => (
                                  <span key={p} className="px-1.5 py-0.5 text-[9px] font-bold tracking-wider rounded bg-vmm-success/15 text-vmm-success border border-vmm-success/20">
                                    {permLabels[p] || p}
                                  </span>
                                ))}
                              </div>
                            </div>
                          </div>
                          <div className="flex items-center gap-1">
                            <button onClick={() => setEditPerms({ rg, groupId: perm.group_id, perms: [...perm.permissions] })}
                              className="p-1.5 text-vmm-text-muted hover:text-vmm-text transition-colors cursor-pointer rounded hover:bg-vmm-surface-hover">
                              <Settings size={14} />
                            </button>
                            <button onClick={() => handleRemovePerms(rg.id, perm.group_id)}
                              className="p-1.5 text-vmm-text-muted hover:text-vmm-danger transition-colors cursor-pointer rounded hover:bg-vmm-danger/10">
                              <Trash2 size={14} />
                            </button>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}
            </Card>
          )
        })}
      </div>

      <ConfirmDialog open={!!deleteGroup} title="Delete Resource Group" danger
        message={`Delete "${deleteGroup?.name}"? All VMs will be moved to "All Machines".`}
        confirmLabel="Delete" onConfirm={handleDelete} onCancel={() => setDeleteGroup(null)} />
    </div>
  )
}
