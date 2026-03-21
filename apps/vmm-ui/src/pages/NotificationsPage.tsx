import { useEffect, useState } from 'react'
import { Bell, Plus, Trash2, ToggleLeft, ToggleRight, Mail, Globe, FileText, Send, Clock } from 'lucide-react'
import api from '../api/client'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'

interface Channel {
  id: number; name: string; channel_type: string; enabled: boolean; config_json: string; created_at: string
}
interface Rule {
  id: number; name: string; enabled: boolean; event_category: string; min_severity: string
  channel_id: number; channel_name: string | null; cooldown_secs: number; cluster_id: string | null; created_at: string
}
interface LogEntry {
  id: number; rule_id: number | null; channel_id: number | null; event_id: number | null
  status: string; error: string | null; sent_at: string
}

const channelTypeIcon = (t: string) => {
  switch (t) {
    case 'email': return <Mail size={14} />
    case 'webhook': return <Globe size={14} />
    default: return <FileText size={14} />
  }
}

const severityOptions = [
  { value: 'info', label: 'Info (all)' },
  { value: 'warning', label: 'Warning+' },
  { value: 'error', label: 'Error+' },
  { value: 'critical', label: 'Critical only' },
]

const categoryOptions = [
  { value: '*', label: 'All events' },
  { value: 'ha', label: 'HA (host failures)' },
  { value: 'drs', label: 'DRS (rebalancing)' },
  { value: 'host', label: 'Host events' },
  { value: 'vm', label: 'VM events' },
  { value: 'datastore', label: 'Datastore events' },
  { value: 'alarm', label: 'Alarm triggers' },
  { value: 'task', label: 'Task completions' },
]

export default function NotificationsPage() {
  const [channels, setChannels] = useState<Channel[]>([])
  const [rules, setRules] = useState<Rule[]>([])
  const [log, setLog] = useState<LogEntry[]>([])
  const [showAddChannel, setShowAddChannel] = useState(false)
  const [showAddRule, setShowAddRule] = useState(false)
  const [newChannel, setNewChannel] = useState({ name: '', channel_type: 'log', config: {} as any })
  const [newRule, setNewRule] = useState({ name: '', event_category: '*', min_severity: 'warning', channel_id: 0, cooldown_secs: 300 })

  const fetchAll = () => {
    api.get('/api/notifications/channels').then(({ data }) => setChannels(data)).catch(() => {})
    api.get('/api/notifications/rules').then(({ data }) => setRules(data)).catch(() => {})
    api.get('/api/notifications/log?limit=20').then(({ data }) => setLog(data)).catch(() => {})
  }

  useEffect(() => { fetchAll() }, [])
  useEffect(() => { if (channels.length > 0 && !newRule.channel_id) setNewRule(r => ({ ...r, channel_id: channels[0].id })) }, [channels])

  const handleCreateChannel = async (e: React.FormEvent) => {
    e.preventDefault()
    const config = newChannel.channel_type === 'webhook'
      ? { url: (newChannel.config as any).url || '', method: 'POST' }
      : newChannel.channel_type === 'email'
        ? { smtp_host: '', smtp_port: 587, from: '', to: (newChannel.config as any).to || '' }
        : { level: 'info' }
    await api.post('/api/notifications/channels', { ...newChannel, config })
    setShowAddChannel(false); setNewChannel({ name: '', channel_type: 'log', config: {} }); fetchAll()
  }

  const handleCreateRule = async (e: React.FormEvent) => {
    e.preventDefault()
    await api.post('/api/notifications/rules', newRule)
    setShowAddRule(false); setNewRule({ name: '', event_category: '*', min_severity: 'warning', channel_id: channels[0]?.id || 0, cooldown_secs: 300 }); fetchAll()
  }

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">Notifications</h1>
        <p className="text-sm text-vmm-text-muted mt-1">Configure notification channels and alert routing rules</p>
      </div>

      {/* ── Channels ───────────────────────────────────────────────── */}
      <div>
        <div className="flex items-center justify-between mb-3">
          <SectionLabel>Channels</SectionLabel>
          <button onClick={() => setShowAddChannel(true)}
            className="flex items-center gap-2 px-3 py-1.5 bg-vmm-accent/10 text-vmm-accent hover:bg-vmm-accent/20 rounded-lg text-xs font-medium">
            <Plus size={12} /> Add Channel
          </button>
        </div>

        {showAddChannel && (
          <Card>
            <form onSubmit={handleCreateChannel} className="p-4 space-y-3">
              <div className="grid grid-cols-2 gap-3">
                <input type="text" value={newChannel.name} onChange={e => setNewChannel({ ...newChannel, name: e.target.value })}
                  placeholder="Channel name" required className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                <select value={newChannel.channel_type} onChange={e => setNewChannel({ ...newChannel, channel_type: e.target.value })}
                  className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  <option value="log">Event Log</option>
                  <option value="webhook">Webhook</option>
                  <option value="email">E-Mail</option>
                </select>
              </div>
              {newChannel.channel_type === 'webhook' && (
                <input type="url" placeholder="Webhook URL (https://...)"
                  value={(newChannel.config as any).url || ''}
                  onChange={e => setNewChannel({ ...newChannel, config: { url: e.target.value } })}
                  className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" required />
              )}
              {newChannel.channel_type === 'email' && (
                <input type="email" placeholder="Recipient email"
                  value={(newChannel.config as any).to || ''}
                  onChange={e => setNewChannel({ ...newChannel, config: { to: e.target.value } })}
                  className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" required />
              )}
              <div className="flex gap-2 justify-end">
                <button type="button" onClick={() => setShowAddChannel(false)} className="px-3 py-1.5 text-sm text-vmm-text-muted">Cancel</button>
                <button type="submit" className="px-4 py-1.5 bg-vmm-accent text-white rounded-lg text-sm font-medium">Create</button>
              </div>
            </form>
          </Card>
        )}

        <div className="space-y-2">
          {channels.map(ch => (
            <Card key={ch.id}>
              <div className="p-3 flex items-center gap-3">
                <button onClick={async () => { await api.put(`/api/notifications/channels/${ch.id}`, { enabled: !ch.enabled }); fetchAll() }}>
                  {ch.enabled ? <ToggleRight size={20} className="text-vmm-success" /> : <ToggleLeft size={20} className="text-vmm-text-muted" />}
                </button>
                <div className="flex items-center gap-2 text-vmm-text-muted">{channelTypeIcon(ch.channel_type)}</div>
                <div className="flex-1">
                  <span className={`text-sm font-medium ${ch.enabled ? 'text-vmm-text' : 'text-vmm-text-muted'}`}>{ch.name}</span>
                  <span className="text-xs text-vmm-text-muted ml-2 uppercase">{ch.channel_type}</span>
                </div>
                <button onClick={async () => { await api.post(`/api/notifications/channels/${ch.id}/test`); alert('Test notification sent') }}
                  className="text-vmm-text-muted hover:text-vmm-accent"><Send size={13} /></button>
                <button onClick={async () => { if (confirm('Delete channel?')) { await api.delete(`/api/notifications/channels/${ch.id}`); fetchAll() } }}
                  className="text-vmm-text-muted hover:text-vmm-danger"><Trash2 size={13} /></button>
              </div>
            </Card>
          ))}
          {channels.length === 0 && !showAddChannel && (
            <div className="text-xs text-vmm-text-muted text-center py-4">No notification channels configured</div>
          )}
        </div>
      </div>

      {/* ── Rules ──────────────────────────────────────────────────── */}
      <div>
        <div className="flex items-center justify-between mb-3">
          <SectionLabel>Routing Rules</SectionLabel>
          <button onClick={() => setShowAddRule(true)} disabled={channels.length === 0}
            className="flex items-center gap-2 px-3 py-1.5 bg-vmm-accent/10 text-vmm-accent hover:bg-vmm-accent/20 rounded-lg text-xs font-medium disabled:opacity-40">
            <Plus size={12} /> Add Rule
          </button>
        </div>

        {showAddRule && (
          <Card>
            <form onSubmit={handleCreateRule} className="p-4 space-y-3">
              <div className="grid grid-cols-2 sm:grid-cols-3 gap-3">
                <input type="text" value={newRule.name} onChange={e => setNewRule({ ...newRule, name: e.target.value })}
                  placeholder="Rule name" required className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                <select value={newRule.event_category} onChange={e => setNewRule({ ...newRule, event_category: e.target.value })}
                  className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  {categoryOptions.map(o => <option key={o.value} value={o.value}>{o.label}</option>)}
                </select>
                <select value={newRule.min_severity} onChange={e => setNewRule({ ...newRule, min_severity: e.target.value })}
                  className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  {severityOptions.map(o => <option key={o.value} value={o.value}>{o.label}</option>)}
                </select>
                <select value={newRule.channel_id} onChange={e => setNewRule({ ...newRule, channel_id: parseInt(e.target.value) })}
                  className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm">
                  {channels.map(ch => <option key={ch.id} value={ch.id}>{ch.name} ({ch.channel_type})</option>)}
                </select>
                <div className="flex items-center gap-2">
                  <span className="text-xs text-vmm-text-muted">Cooldown:</span>
                  <input type="number" min={0} value={newRule.cooldown_secs}
                    onChange={e => setNewRule({ ...newRule, cooldown_secs: parseInt(e.target.value) || 0 })}
                    className="w-20 px-2 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                  <span className="text-xs text-vmm-text-muted">sec</span>
                </div>
              </div>
              <div className="flex gap-2 justify-end">
                <button type="button" onClick={() => setShowAddRule(false)} className="px-3 py-1.5 text-sm text-vmm-text-muted">Cancel</button>
                <button type="submit" className="px-4 py-1.5 bg-vmm-accent text-white rounded-lg text-sm font-medium">Create</button>
              </div>
            </form>
          </Card>
        )}

        <div className="space-y-2">
          {rules.map(rule => (
            <Card key={rule.id}>
              <div className="p-3 flex items-center gap-3">
                <button onClick={async () => { await api.put(`/api/notifications/rules/${rule.id}`, { enabled: !rule.enabled }); fetchAll() }}>
                  {rule.enabled ? <ToggleRight size={20} className="text-vmm-success" /> : <ToggleLeft size={20} className="text-vmm-text-muted" />}
                </button>
                <div className="flex-1">
                  <span className={`text-sm font-medium ${rule.enabled ? 'text-vmm-text' : 'text-vmm-text-muted'}`}>{rule.name}</span>
                  <div className="text-xs text-vmm-text-muted mt-0.5">
                    {rule.event_category === '*' ? 'All events' : rule.event_category} &bull; {rule.min_severity}+ &rarr; {rule.channel_name || `Channel #${rule.channel_id}`}
                    &bull; Cooldown: {rule.cooldown_secs}s
                  </div>
                </div>
                <button onClick={async () => { if (confirm('Delete rule?')) { await api.delete(`/api/notifications/rules/${rule.id}`); fetchAll() } }}
                  className="text-vmm-text-muted hover:text-vmm-danger"><Trash2 size={13} /></button>
              </div>
            </Card>
          ))}
          {rules.length === 0 && !showAddRule && (
            <div className="text-xs text-vmm-text-muted text-center py-4">
              {channels.length === 0 ? 'Create a channel first, then add routing rules' : 'No routing rules configured'}
            </div>
          )}
        </div>
      </div>

      {/* ── Recent Log ─────────────────────────────────────────────── */}
      {log.length > 0 && (
        <div>
          <SectionLabel>Recent Notifications</SectionLabel>
          <div className="space-y-1 mt-2">
            {log.map(entry => (
              <Card key={entry.id}>
                <div className="px-3 py-2 flex items-center gap-3 text-xs">
                  <span className={`w-2 h-2 rounded-full flex-shrink-0 ${entry.status === 'sent' ? 'bg-vmm-success' : entry.status === 'throttled' ? 'bg-yellow-400' : 'bg-vmm-danger'}`} />
                  <span className="text-vmm-text-muted capitalize">{entry.status}</span>
                  {entry.error && <span className="text-vmm-danger truncate">{entry.error}</span>}
                  <span className="ml-auto text-vmm-text-muted flex items-center gap-1">
                    <Clock size={10} /> {new Date(entry.sent_at).toLocaleString()}
                  </span>
                </div>
              </Card>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
