/** Human-readable labels for audit log actions. */

import type { AuditEntry } from '../api/types'

/** Map of action key → human-readable description. */
const actionLabels: Record<string, string> = {
  // VM lifecycle
  'vm.created':           'Virtual machine created',
  'vm.deleted':           'Virtual machine deleted',
  'vm.started':           'Virtual machine started',
  'vm.stop_requested':    'Graceful shutdown requested',
  'vm.force_stopped':     'Virtual machine force-stopped',
  'vm.exited':            'Virtual machine exited',
  'vm.paused':            'Virtual machine paused',
  'vm.resumed':           'Virtual machine resumed',
  'vm.restarted':         'Virtual machine restarted',
  'vm.config_updated':    'VM configuration updated',

  // Snapshots
  'snapshot.created':     'Snapshot created',
  'snapshot.restored':    'Snapshot restored',
  'snapshot.deleted':     'Snapshot deleted',

  // Storage
  'pool.created':         'Storage pool added',
  'pool.deleted':         'Storage pool removed',
  'pool.updated':         'Storage pool updated',
  'image.created':        'Disk image created',
  'image.deleted':        'Disk image deleted',
  'image.resized':        'Disk image resized',
  'iso.uploaded':         'ISO image uploaded',
  'iso.deleted':          'ISO image deleted',

  // User management
  'user.login':           'User signed in',
  'user.logout':          'User signed out',
  'user.created':         'User account created',
  'user.deleted':         'User account deleted',
  'user.password_changed':'Password changed',
  'user.role_changed':    'User role updated',

  // Groups
  'group.created':        'User group created',
  'group.deleted':        'User group deleted',

  // Resource groups
  'resource_group.created': 'Resource group created',
  'resource_group.deleted': 'Resource group deleted',
  'resource_group.updated': 'Resource group updated',
  'resource_group.permissions_set': 'Permissions updated',

  // Network
  'network.bridge_created': 'Network bridge created',
  'network.bridge_deleted': 'Network bridge deleted',
  'network.vlan_created':   'VLAN created',
  'network.vlan_deleted':   'VLAN deleted',

  // System
  'system.startup':       'System started',
  'system.shutdown':      'System shutting down',
  'system.backup':        'System backup completed',
  'system.restore':       'System restore completed',
  'settings.updated':     'Settings updated',
}

/** Severity level for coloring log entries. */
export type AuditSeverity = 'info' | 'success' | 'warning' | 'danger'

/** Get human-readable label for an audit action. */
export function getActionLabel(action: string): string {
  return actionLabels[action] || action.replace(/[._]/g, ' ').replace(/\b\w/g, c => c.toUpperCase())
}

/** Determine severity of an audit action for color-coding. */
export function getActionSeverity(action: string): AuditSeverity {
  if (action.includes('delete') || action.includes('force_stop') || action.includes('removed')) return 'danger'
  if (action.includes('start') || action.includes('created') || action.includes('login') || action.includes('resume')) return 'success'
  if (action.includes('stop') || action.includes('exit') || action.includes('pause') || action.includes('warning')) return 'warning'
  return 'info'
}

/** Format an audit entry as a single log line for the System Journal. */
export function formatJournalLine(entry: AuditEntry): string {
  const time = entry.created_at.includes('T')
    ? entry.created_at.split('T')[1]?.slice(0, 8) || entry.created_at
    : entry.created_at
  const label = getActionLabel(entry.action)
  const detail = entry.details ? ` — ${entry.details}` : ''
  const target = entry.target_id ? ` (${entry.target_id.slice(0, 8)})` : ''
  return `[${time}] ${label}${detail}${target}`
}

/** Get a suitable icon name for an action category. */
export function getActionIcon(action: string): 'power' | 'trash' | 'plus' | 'edit' | 'user' | 'disk' | 'camera' | 'activity' {
  if (action.includes('start') || action.includes('stop') || action.includes('restart') || action.includes('pause') || action.includes('resume') || action.includes('exit')) return 'power'
  if (action.includes('delete') || action.includes('removed')) return 'trash'
  if (action.includes('create') || action.includes('upload')) return 'plus'
  if (action.includes('update') || action.includes('config') || action.includes('resize') || action.includes('password') || action.includes('role') || action.includes('permission')) return 'edit'
  if (action.includes('user') || action.includes('login') || action.includes('logout')) return 'user'
  if (action.includes('image') || action.includes('pool') || action.includes('iso') || action.includes('disk')) return 'disk'
  if (action.includes('snapshot')) return 'camera'
  return 'activity'
}
