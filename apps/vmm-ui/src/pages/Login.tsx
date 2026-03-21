import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useAuthStore } from '../stores/authStore'
import Button from '../components/Button'
import { Monitor } from 'lucide-react'

export default function Login() {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const { login } = useAuthStore()
  const navigate = useNavigate()

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault()
    setError('')
    setLoading(true)
    try {
      await login(username, password)
      navigate('/')
    } catch {
      setError('Invalid username or password')
    } finally {
      setLoading(false)
    }
  }

  return (
    <div className="min-h-screen bg-vmm-bg flex items-center justify-center px-4">
      <div className="w-full max-w-sm">
        {/* Logo */}
        <div className="flex items-center justify-center gap-3 mb-8">
          <div className="w-10 h-10 rounded-xl bg-vmm-accent/20 flex items-center justify-center">
            <Monitor size={22} className="text-vmm-accent" />
          </div>
          <div>
            <div className="text-xl font-bold text-vmm-text">VMManager</div>
            <div className="text-[10px] text-vmm-text-muted font-mono tracking-widest">COREVM ENTERPRISE</div>
          </div>
        </div>

        {/* Form */}
        <form onSubmit={handleSubmit} className="bg-vmm-surface border border-vmm-border rounded-xl p-6 space-y-4">
          <div>
            <label className="block text-xs font-medium text-vmm-text-dim mb-1.5">Username</label>
            <input
              type="text" value={username} onChange={(e) => setUsername(e.target.value)}
              className="w-full bg-vmm-bg border border-vmm-border rounded-lg px-3 py-2.5 text-sm text-vmm-text
                placeholder-vmm-text-muted focus:outline-none focus:border-vmm-accent/50"
              placeholder="admin" autoFocus
            />
          </div>
          <div>
            <label className="block text-xs font-medium text-vmm-text-dim mb-1.5">Password</label>
            <input
              type="password" value={password} onChange={(e) => setPassword(e.target.value)}
              className="w-full bg-vmm-bg border border-vmm-border rounded-lg px-3 py-2.5 text-sm text-vmm-text
                placeholder-vmm-text-muted focus:outline-none focus:border-vmm-accent/50"
              placeholder="Password"
            />
          </div>
          {error && <div className="text-xs text-vmm-danger">{error}</div>}
          <Button type="submit" className="w-full" disabled={loading}>
            {loading ? 'Signing in...' : 'Sign In'}
          </Button>
        </form>
      </div>
    </div>
  )
}
