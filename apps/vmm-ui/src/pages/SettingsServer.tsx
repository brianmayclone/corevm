/** Server/Cluster Settings — server info, SMTP, LDAP/AD. */
import { useEffect, useState } from 'react'
import { Server, Clock, Shield, Activity, Mail, Save, Key, FolderTree, CheckCircle, XCircle, Send, Trash2, Plus, ToggleLeft, ToggleRight } from 'lucide-react'
import api from '../api/client'
import { useClusterStore } from '../stores/clusterStore'
import type { ServerSettings, SecuritySettings } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import SpecRow from '../components/SpecRow'
import TabBar from '../components/TabBar'

function formatUptime(secs: number): string {
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  return `${d}d ${h}h ${m}m`
}

interface SmtpConfig {
  host: string; port: number; username: string; from_address: string; use_tls: boolean; configured: boolean
}

interface LdapConfig {
  id: number; name: string; enabled: boolean; server_url: string; bind_dn: string
  base_dn: string; user_search_dn: string; user_filter: string
  group_search_dn: string; group_filter: string
  attr_username: string; attr_email: string; attr_display: string
  role_mapping: string; use_tls: boolean; skip_tls_verify: boolean
}

export default function SettingsServer() {
  const { backendMode } = useClusterStore()
  const isCluster = backendMode === 'cluster'
  const [server, setServer] = useState<ServerSettings | null>(null)
  const [security, setSecurity] = useState<SecuritySettings | null>(null)
  const [tab, setTab] = useState('general')

  // SMTP state
  const [smtp, setSmtp] = useState<SmtpConfig | null>(null)
  const [smtpForm, setSmtpForm] = useState({ host: '', port: 587, username: '', password: '', from_address: '', use_tls: true })
  const [smtpSaving, setSmtpSaving] = useState(false)
  const [smtpMsg, setSmtpMsg] = useState('')

  // LDAP state
  const [ldapConfigs, setLdapConfigs] = useState<LdapConfig[]>([])
  const [showAddLdap, setShowAddLdap] = useState(false)
  const [ldapForm, setLdapForm] = useState({ name: '', server_url: 'ldap://', base_dn: '' })

  const tabs = isCluster
    ? [{ id: 'general', label: 'General' }, { id: 'smtp', label: 'E-Mail (SMTP)' }, { id: 'ldap', label: 'Directory Services' }]
    : [{ id: 'general', label: 'General' }]

  useEffect(() => {
    api.get<ServerSettings>('/api/settings/server').then(({ data }) => setServer(data))
    api.get<SecuritySettings>('/api/settings/security').then(({ data }) => setSecurity(data))
    if (isCluster) {
      api.get<SmtpConfig>('/api/settings/smtp').then(({ data }) => {
        setSmtp(data)
        setSmtpForm(f => ({ ...f, host: data.host, port: data.port, username: data.username, from_address: data.from_address, use_tls: data.use_tls }))
      }).catch(() => {})
      api.get<LdapConfig[]>('/api/ldap').then(({ data }) => setLdapConfigs(data)).catch(() => {})
    }
  }, [])

  const saveSmtp = async () => {
    setSmtpSaving(true); setSmtpMsg('')
    try {
      await api.put('/api/settings/smtp', smtpForm)
      setSmtpMsg('SMTP settings saved')
      api.get<SmtpConfig>('/api/settings/smtp').then(({ data }) => setSmtp(data))
    } catch (e: any) {
      setSmtpMsg(e.response?.data?.error || 'Failed to save')
    } finally { setSmtpSaving(false) }
  }

  const createLdap = async (e: React.FormEvent) => {
    e.preventDefault()
    await api.post('/api/ldap', ldapForm)
    setShowAddLdap(false); setLdapForm({ name: '', server_url: 'ldap://', base_dn: '' })
    api.get<LdapConfig[]>('/api/ldap').then(({ data }) => setLdapConfigs(data))
  }

  const toggleLdap = async (id: number, enabled: boolean) => {
    await api.put(`/api/ldap/${id}`, { enabled: !enabled })
    api.get<LdapConfig[]>('/api/ldap').then(({ data }) => setLdapConfigs(data))
  }

  const deleteLdap = async (id: number) => {
    if (!confirm('Delete this LDAP configuration?')) return
    await api.delete(`/api/ldap/${id}`)
    api.get<LdapConfig[]>('/api/ldap').then(({ data }) => setLdapConfigs(data))
  }

  const testLdap = async (id: number) => {
    try {
      const { data } = await api.post(`/api/ldap/${id}/test`)
      alert(data.message || 'Test successful')
    } catch (e: any) {
      alert(e.response?.data?.error || 'Test failed')
    }
  }

  if (!server) return <div className="text-vmm-text-muted py-8 text-center">Loading...</div>

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">{isCluster ? 'Cluster Configuration' : 'Server Configuration'}</h1>
        <p className="text-sm text-vmm-text-muted mt-1">
          {isCluster ? 'System settings, mail server, directory services' : 'Runtime parameters and process information'}
        </p>
      </div>

      {tabs.length > 1 && <TabBar tabs={tabs} active={tab} onChange={setTab} />}

      {/* ── General Tab ──────────────────────────────────────────── */}
      {tab === 'general' && (
        <>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-5">
            <Card>
              <div className="flex items-center gap-2 mb-4">
                <Server size={18} className="text-vmm-accent" />
                <SectionLabel>Process Info</SectionLabel>
              </div>
              <SpecRow icon={<Activity size={14} />} label="Version" value={`v${server.version}`} />
              <SpecRow icon={<Clock size={14} />} label="Uptime" value={formatUptime(server.uptime_secs)} />
              <SpecRow icon={<Server size={14} />} label="Bind Address" value={`${server.bind}:${server.port}`} />
              <SpecRow icon={<Activity size={14} />} label="Log Level" value={server.log_level.toUpperCase()} />
            </Card>
            <Card>
              <div className="flex items-center gap-2 mb-4">
                <Shield size={18} className="text-vmm-accent" />
                <SectionLabel>Limits & Security</SectionLabel>
              </div>
              <SpecRow icon={<Clock size={14} />} label="Session Timeout" value={`${server.session_timeout_hours}h`} />
              <SpecRow icon={<Server size={14} />} label="Max Disk Size" value={`${server.max_disk_size_gb} GB`} />
              {isCluster && smtp && (
                <SpecRow icon={<Mail size={14} />} label="SMTP"
                  value={smtp.configured ? `${smtp.host}:${smtp.port}` : 'Not configured'} />
              )}
            </Card>
          </div>

          {security && (
            <Card>
              <div className="flex items-center gap-2 mb-4">
                <Shield size={18} className="text-vmm-danger" />
                <SectionLabel>Security Policy</SectionLabel>
              </div>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-x-8 gap-y-3 text-sm">
                <div className="flex justify-between"><span className="text-vmm-text-muted">Max Login Attempts</span><span className="text-vmm-text font-mono">{security.max_login_attempts}</span></div>
                <div className="flex justify-between"><span className="text-vmm-text-muted">Lockout Duration</span><span className="text-vmm-text font-mono">{security.lockout_duration_secs}s</span></div>
                <div className="flex justify-between"><span className="text-vmm-text-muted">Min Password Length</span><span className="text-vmm-text font-mono">{security.password_min_length}</span></div>
                <div className="flex justify-between"><span className="text-vmm-text-muted">Require Uppercase</span><span className={security.require_uppercase ? 'text-vmm-success' : 'text-vmm-text-muted'}>{security.require_uppercase ? 'Yes' : 'No'}</span></div>
                <div className="flex justify-between"><span className="text-vmm-text-muted">Require Numbers</span><span className={security.require_numbers ? 'text-vmm-success' : 'text-vmm-text-muted'}>{security.require_numbers ? 'Yes' : 'No'}</span></div>
                <div className="flex justify-between"><span className="text-vmm-text-muted">API Keys</span><span className={security.api_keys_enabled ? 'text-vmm-success' : 'text-vmm-text-muted'}>{security.api_keys_enabled ? 'Enabled' : 'Disabled'}</span></div>
              </div>
            </Card>
          )}
        </>
      )}

      {/* ── SMTP Tab (Cluster only) ──────────────────────────────── */}
      {tab === 'smtp' && isCluster && (
        <Card>
          <div className="flex items-center gap-2 mb-5">
            <Mail size={18} className="text-vmm-accent" />
            <SectionLabel>SMTP Mail Server</SectionLabel>
            {smtp?.configured && <span className="ml-2 text-xs px-2 py-0.5 rounded-full bg-vmm-success/10 text-vmm-success">Configured</span>}
            {smtp && !smtp.configured && <span className="ml-2 text-xs px-2 py-0.5 rounded-full bg-vmm-danger/10 text-vmm-danger">Not configured</span>}
          </div>
          <p className="text-xs text-vmm-text-muted mb-4">
            Configure the outgoing mail server for notification emails. Used by notification channels of type "E-Mail".
          </p>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 mb-4">
            <div>
              <label className="block text-xs text-vmm-text-muted mb-1">SMTP Host</label>
              <input type="text" value={smtpForm.host} onChange={e => setSmtpForm({ ...smtpForm, host: e.target.value })}
                placeholder="smtp.example.com" className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
            </div>
            <div>
              <label className="block text-xs text-vmm-text-muted mb-1">Port</label>
              <input type="number" value={smtpForm.port} onChange={e => setSmtpForm({ ...smtpForm, port: parseInt(e.target.value) || 587 })}
                className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
            </div>
            <div>
              <label className="block text-xs text-vmm-text-muted mb-1">Username</label>
              <input type="text" value={smtpForm.username} onChange={e => setSmtpForm({ ...smtpForm, username: e.target.value })}
                placeholder="notifications@example.com" className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
            </div>
            <div>
              <label className="block text-xs text-vmm-text-muted mb-1">Password</label>
              <input type="password" value={smtpForm.password} onChange={e => setSmtpForm({ ...smtpForm, password: e.target.value })}
                placeholder="••••••••" className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
            </div>
            <div>
              <label className="block text-xs text-vmm-text-muted mb-1">From Address</label>
              <input type="email" value={smtpForm.from_address} onChange={e => setSmtpForm({ ...smtpForm, from_address: e.target.value })}
                placeholder="vmm-cluster@example.com" className="w-full px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
            </div>
            <div className="flex items-center gap-3 pt-5">
              <label className="text-sm text-vmm-text">Use TLS</label>
              <button onClick={() => setSmtpForm({ ...smtpForm, use_tls: !smtpForm.use_tls })}>
                {smtpForm.use_tls ? <ToggleRight size={22} className="text-vmm-success" /> : <ToggleLeft size={22} className="text-vmm-text-muted" />}
              </button>
            </div>
          </div>
          <div className="flex items-center gap-3">
            <button onClick={saveSmtp} disabled={smtpSaving}
              className="flex items-center gap-2 px-4 py-2 bg-vmm-accent hover:bg-vmm-accent-hover text-white rounded-lg text-sm font-medium disabled:opacity-50">
              <Save size={14} /> {smtpSaving ? 'Saving...' : 'Save SMTP Settings'}
            </button>
            {smtpMsg && <span className="text-xs text-vmm-success">{smtpMsg}</span>}
          </div>
        </Card>
      )}

      {/* ── LDAP/AD Tab (Cluster only) ───────────────────────────── */}
      {tab === 'ldap' && isCluster && (
        <div className="space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <div className="flex items-center gap-2">
                <FolderTree size={18} className="text-vmm-accent" />
                <SectionLabel>Directory Services (LDAP / Active Directory)</SectionLabel>
              </div>
              <p className="text-xs text-vmm-text-muted mt-1">
                Connect to Active Directory or LDAP for centralized user authentication and group-based role mapping.
              </p>
            </div>
            <button onClick={() => setShowAddLdap(true)}
              className="flex items-center gap-2 px-3 py-1.5 bg-vmm-accent/10 text-vmm-accent hover:bg-vmm-accent/20 rounded-lg text-xs font-medium">
              <Plus size={12} /> Add Directory
            </button>
          </div>

          {showAddLdap && (
            <Card>
              <form onSubmit={createLdap} className="p-4 space-y-3">
                <div className="grid grid-cols-3 gap-3">
                  <input type="text" value={ldapForm.name} onChange={e => setLdapForm({ ...ldapForm, name: e.target.value })}
                    placeholder="Display name (e.g. Corporate AD)" required className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                  <input type="text" value={ldapForm.server_url} onChange={e => setLdapForm({ ...ldapForm, server_url: e.target.value })}
                    placeholder="ldap://dc.example.com:389" required className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                  <input type="text" value={ldapForm.base_dn} onChange={e => setLdapForm({ ...ldapForm, base_dn: e.target.value })}
                    placeholder="DC=example,DC=com" required className="px-3 py-2 bg-vmm-bg border border-vmm-border rounded-lg text-vmm-text text-sm" />
                </div>
                <div className="flex gap-2 justify-end">
                  <button type="button" onClick={() => setShowAddLdap(false)} className="px-3 py-1.5 text-sm text-vmm-text-muted">Cancel</button>
                  <button type="submit" className="px-4 py-1.5 bg-vmm-accent text-white rounded-lg text-sm font-medium">Create</button>
                </div>
              </form>
            </Card>
          )}

          {ldapConfigs.map(ldap => (
            <Card key={ldap.id}>
              <div className="p-4 space-y-3">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-3">
                    <button onClick={() => toggleLdap(ldap.id, ldap.enabled)}>
                      {ldap.enabled ? <ToggleRight size={22} className="text-vmm-success" /> : <ToggleLeft size={22} className="text-vmm-text-muted" />}
                    </button>
                    <div>
                      <span className={`text-sm font-semibold ${ldap.enabled ? 'text-vmm-text' : 'text-vmm-text-muted'}`}>{ldap.name}</span>
                      <div className="text-xs text-vmm-text-muted">{ldap.server_url}</div>
                    </div>
                  </div>
                  <div className="flex gap-2">
                    <button onClick={() => testLdap(ldap.id)}
                      className="flex items-center gap-1 px-3 py-1.5 bg-vmm-surface text-vmm-text-muted hover:text-vmm-text rounded-lg text-xs"><Send size={11} /> Test</button>
                    <button onClick={() => deleteLdap(ldap.id)}
                      className="text-vmm-text-muted hover:text-vmm-danger"><Trash2 size={14} /></button>
                  </div>
                </div>
                <div className="grid grid-cols-2 gap-x-6 gap-y-2 text-xs">
                  <div><span className="text-vmm-text-muted">Base DN:</span> <span className="text-vmm-text font-mono">{ldap.base_dn}</span></div>
                  <div><span className="text-vmm-text-muted">Bind DN:</span> <span className="text-vmm-text font-mono">{ldap.bind_dn || '(not set)'}</span></div>
                  <div><span className="text-vmm-text-muted">User Filter:</span> <span className="text-vmm-text font-mono text-[10px]">{ldap.user_filter}</span></div>
                  <div><span className="text-vmm-text-muted">TLS:</span> <span className="text-vmm-text">{ldap.use_tls ? 'Enabled' : 'Disabled'}</span></div>
                </div>
              </div>
            </Card>
          ))}

          {ldapConfigs.length === 0 && !showAddLdap && (
            <div className="text-center py-8 text-vmm-text-muted">
              <Key size={28} className="mx-auto mb-2 opacity-30" />
              No directory services configured
            </div>
          )}
        </div>
      )}
    </div>
  )
}
