import { Monitor, ExternalLink, Shield } from 'lucide-react'
import { useClusterStore } from '../stores/clusterStore'
import { useUiStore } from '../stores/uiStore'

export default function ManagedModePage() {
  const { clusterUrl } = useClusterStore()
  const { brandName } = useUiStore()

  return (
    <div className="min-h-screen bg-vmm-bg flex items-center justify-center p-4">
      <div className="max-w-lg w-full text-center space-y-6">
        <div className="w-20 h-20 mx-auto rounded-2xl bg-vmm-accent/20 flex items-center justify-center">
          <Shield size={40} className="text-vmm-accent" />
        </div>

        <div>
          <h1 className="text-2xl font-bold text-vmm-text">Managed by VMM-Cluster</h1>
          <p className="text-vmm-text-muted mt-3 leading-relaxed">
            This {brandName} host is managed by a VMM-Cluster instance.
            All management operations (VMs, storage, networking) are controlled
            through the cluster management interface.
          </p>
        </div>

        <div className="bg-vmm-surface border border-vmm-border rounded-xl p-4 space-y-3">
          <div className="flex items-center gap-2 text-sm text-vmm-text-muted">
            <Monitor size={14} />
            <span>Cluster URL</span>
          </div>
          <code className="block text-sm text-vmm-accent break-all">
            {clusterUrl || 'Unknown'}
          </code>
        </div>

        {clusterUrl && (
          <a
            href={clusterUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-2 px-6 py-3 bg-vmm-accent hover:bg-vmm-accent-hover
              text-white rounded-lg font-medium transition-colors"
          >
            <ExternalLink size={16} />
            Open Cluster Management
          </a>
        )}

        <p className="text-xs text-vmm-text-muted">
          Running VMs on this host are not affected. They will continue to operate normally
          even if the cluster is temporarily unavailable.
        </p>
      </div>
    </div>
  )
}
