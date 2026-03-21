/** Date & Time Settings — timezone, NTP configuration. */
import { useEffect, useState } from 'react'
import { Clock, RefreshCw, CheckCircle, AlertTriangle } from 'lucide-react'
import api from '../api/client'
import type { TimeSettings } from '../api/types'
import Card from '../components/Card'
import SectionLabel from '../components/SectionLabel'
import Button from '../components/Button'

const commonTimezones = [
  'UTC', 'Europe/Berlin', 'Europe/London', 'Europe/Paris', 'Europe/Moscow',
  'America/New_York', 'America/Chicago', 'America/Los_Angeles',
  'Asia/Tokyo', 'Asia/Shanghai', 'Asia/Kolkata',
  'Australia/Sydney', 'Pacific/Auckland',
]

export default function SettingsTime() {
  const [time, setTime] = useState<TimeSettings | null>(null)
  const [selectedTz, setSelectedTz] = useState('')
  const [currentTime, setCurrentTime] = useState('')

  const refresh = () => {
    api.get<TimeSettings>('/api/settings/time').then(({ data }) => {
      setTime(data)
      setSelectedTz(data.timezone)
      setCurrentTime(data.current_time)
    })
  }
  useEffect(() => {
    refresh()
    const interval = setInterval(() => {
      api.get<TimeSettings>('/api/settings/time').then(({ data }) => setCurrentTime(data.current_time))
    }, 1000)
    return () => clearInterval(interval)
  }, [])

  const handleSaveTz = async () => {
    try {
      await api.put('/api/settings/time/timezone', { timezone: selectedTz })
      refresh()
    } catch (err: any) {
      alert(err?.response?.data?.error || 'Failed')
    }
  }

  if (!time) return <div className="text-vmm-text-muted">Loading...</div>

  return (
    <div className="space-y-6">
      <div>
        <h1 className="text-2xl font-bold text-vmm-text">Date & Time</h1>
        <p className="text-sm text-vmm-text-muted mt-1">System clock and time synchronization settings</p>
      </div>

      {/* Current time display */}
      <Card>
        <div className="flex items-center justify-between">
          <div>
            <SectionLabel className="mb-2">System Clock</SectionLabel>
            <div className="text-4xl font-bold text-vmm-text font-mono">{currentTime}</div>
            <div className="text-sm text-vmm-text-muted mt-1">Timezone: {time.timezone}</div>
          </div>
          <div className="w-20 h-20 rounded-2xl bg-vmm-bg-alt flex items-center justify-center">
            <Clock size={36} className="text-vmm-accent" />
          </div>
        </div>
      </Card>

      <div className="grid grid-cols-1 sm:grid-cols-2 gap-5">
        {/* Timezone */}
        <Card>
          <SectionLabel className="mb-4">Timezone</SectionLabel>
          <select value={selectedTz} onChange={(e) => setSelectedTz(e.target.value)}
            className="w-full bg-vmm-bg-alt border border-vmm-border rounded-lg px-3 py-2 text-sm text-vmm-text focus:border-vmm-accent focus:outline-none mb-3">
            {commonTimezones.map((tz) => <option key={tz} value={tz}>{tz}</option>)}
          </select>
          {selectedTz !== time.timezone && (
            <Button variant="primary" size="sm" onClick={handleSaveTz}>Apply Timezone</Button>
          )}
        </Card>

        {/* NTP Status */}
        <Card>
          <SectionLabel className="mb-4">NTP Synchronization</SectionLabel>
          <div className="flex items-center gap-2 mb-3">
            {time.ntp_enabled ? (
              <><CheckCircle size={16} className="text-vmm-success" /><span className="text-sm text-vmm-success font-medium">NTP Active</span></>
            ) : (
              <><AlertTriangle size={16} className="text-vmm-warning" /><span className="text-sm text-vmm-warning font-medium">NTP Disabled</span></>
            )}
          </div>
          <div className="space-y-2 text-sm">
            <div className="text-[10px] text-vmm-text-muted uppercase tracking-wider">NTP Servers</div>
            {time.ntp_servers.map((srv, i) => (
              <div key={i} className="flex items-center gap-2 text-vmm-text-dim font-mono text-xs bg-vmm-bg-alt px-3 py-2 rounded-lg">
                <RefreshCw size={12} className="text-vmm-accent" /> {srv}
              </div>
            ))}
          </div>
        </Card>
      </div>
    </div>
  )
}
