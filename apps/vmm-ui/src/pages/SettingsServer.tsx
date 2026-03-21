/** Server Settings — bind address, ports, session, logging. */
import { useEffect, useState } from 'react'
import { Server, Clock, Shield, Activity } from 'lucide-react'
import api from '../api/client'
import type { ServerSettings, SecuritySettings } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import SpecRow from '../components/SpecRow'

function formatUptime(secs: number): string {
  const d = Math.floor(secs / 86400)
  const h = Math.floor((secs % 86400) / 3600)
  const m = Math.floor((secs % 3600) / 60)
  return `${d}d ${h}h ${m}m`
}

export default function SettingsServer() {
  const [server, setServer] = useState<ServerSettings | null>(null)
  const [security, setSecurity] = useState<SecuritySettings | null>(null)

  useEffect(() => {
    api.get<ServerSettings>('/api/settings/server').then(({ data }) => setServer(data))
    api.get<SecuritySettings>('/api/settings/security').then(({ data }) => setSecurity(data))
    const interval = setInterval(() => {
      api.get<ServerSettings>('/api/settings/server').then(({ data }) => setServer(data))
    }, 5000)
    return () => clearInterval(interval)
  }, [])

  if (!server) return <div className="text-vmm-text-muted">Loading...</div>

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">Server Configuration</h1>
        <p className="text-sm text-vmm-text-muted mt-1">Runtime parameters and process information</p>
      </div>

      {/* Server Info */}
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
            <SectionLabel>Limits</SectionLabel>
          </div>
          <SpecRow icon={<Clock size={14} />} label="Session Timeout" value={`${server.session_timeout_hours}h`} />
          <SpecRow icon={<Server size={14} />} label="Max Disk Size" value={`${server.max_disk_size_gb} GB`} />
        </Card>
      </div>

      {/* Security */}
      {security && (
        <Card>
          <div className="flex items-center gap-2 mb-4">
            <Shield size={18} className="text-vmm-danger" />
            <SectionLabel>Security Policy</SectionLabel>
          </div>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-x-8 gap-y-3 text-sm">
            <div className="flex justify-between">
              <span className="text-vmm-text-muted">Max Login Attempts</span>
              <span className="text-vmm-text font-mono">{security.max_login_attempts}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-vmm-text-muted">Lockout Duration</span>
              <span className="text-vmm-text font-mono">{security.lockout_duration_secs}s</span>
            </div>
            <div className="flex justify-between">
              <span className="text-vmm-text-muted">Min Password Length</span>
              <span className="text-vmm-text font-mono">{security.password_min_length}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-vmm-text-muted">Require Uppercase</span>
              <span className={security.require_uppercase ? 'text-vmm-success' : 'text-vmm-text-muted'}>{security.require_uppercase ? 'Yes' : 'No'}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-vmm-text-muted">Require Numbers</span>
              <span className={security.require_numbers ? 'text-vmm-success' : 'text-vmm-text-muted'}>{security.require_numbers ? 'Yes' : 'No'}</span>
            </div>
            <div className="flex justify-between">
              <span className="text-vmm-text-muted">API Keys</span>
              <span className={security.api_keys_enabled ? 'text-vmm-success' : 'text-vmm-text-muted'}>{security.api_keys_enabled ? 'Enabled' : 'Disabled'}</span>
            </div>
          </div>
        </Card>
      )}
    </div>
  )
}
