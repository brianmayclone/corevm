import { useEffect } from 'react'
import { useClusterStore } from '../stores/clusterStore'
import { useVmStore } from '../stores/vmStore'
import TreeNode from './TreeNode'
import { Monitor, HardDrive, Network, Server, Workflow, Boxes, Database, Disc } from 'lucide-react'
import type { Host, VmSummary } from '../api/types'

function hostStatus(h: Host): 'green' | 'yellow' | 'red' | 'gray' {
  if (h.status === 'online') return 'green'
  if (h.status === 'maintenance') return 'yellow'
  if (h.status === 'offline' || h.status === 'error') return 'red'
  return 'gray'
}

function vmStatus(v: VmSummary): 'green' | 'yellow' | 'red' | 'gray' {
  if (v.state === 'running') return 'green'
  if (v.state === 'paused' || v.state === 'stopping') return 'yellow'
  return 'gray'
}

interface InventoryTreeProps {
  onNavigate?: () => void
}

export default function InventoryTree({ onNavigate }: InventoryTreeProps) {
  const { hosts, clusters, datastores, backendMode, clusterName, fetchHosts, fetchClusters, fetchDatastores } = useClusterStore()
  const { vms, fetchVms } = useVmStore()
  const isCluster = backendMode === 'cluster'

  useEffect(() => {
    fetchVms()
    if (isCluster) {
      fetchHosts()
      fetchClusters()
      fetchDatastores()
    }
  }, [isCluster])

  // Group VMs by host
  const vmsByHost = new Map<string, VmSummary[]>()
  const unassignedVms: VmSummary[] = []
  for (const vm of vms) {
    const hostId = (vm as any).host_id
    if (hostId) {
      if (!vmsByHost.has(hostId)) vmsByHost.set(hostId, [])
      vmsByHost.get(hostId)!.push(vm)
    } else {
      unassignedVms.push(vm)
    }
  }

  // Group hosts by cluster
  const hostsByCluster = new Map<string, Host[]>()
  const unassignedHosts: Host[] = []
  for (const host of hosts) {
    if (host.cluster_id) {
      if (!hostsByCluster.has(host.cluster_id)) hostsByCluster.set(host.cluster_id, [])
      hostsByCluster.get(host.cluster_id)!.push(host)
    } else {
      unassignedHosts.push(host)
    }
  }

  const sanHosts = hosts.filter(h => h.san_enabled)

  // ── Standalone mode ──
  if (!isCluster) {
    return (
      <div className="py-1 space-y-px">
        <TreeNode icon={Server} label="Local Server" defaultExpanded onNavigate={onNavigate} depth={0}>
          <TreeNode icon={Monitor} label="Virtual Machines" to="/machines/list" defaultExpanded onNavigate={onNavigate} depth={1} count={vms.length}>
            {vms.map(vm => (
              <TreeNode
                key={vm.id}
                icon={Monitor}
                label={vm.name}
                to={`/vms/${vm.id}`}
                statusDot={vmStatus(vm)}
                onNavigate={onNavigate}
                depth={2}
              />
            ))}
          </TreeNode>
          <TreeNode icon={HardDrive} label="Storage" to="/storage/overview" onNavigate={onNavigate} depth={1} />
          <TreeNode icon={Network} label="Networks" to="/networks/overview" onNavigate={onNavigate} depth={1} />
        </TreeNode>
      </div>
    )
  }

  // ── Cluster mode ──
  return (
    <div className="py-1 space-y-px">
      <TreeNode icon={Workflow} label={clusterName || 'CoreVM Cluster'} defaultExpanded onNavigate={onNavigate} depth={0}>

        {/* Clusters → Hosts → VMs */}
        {clusters.map(cluster => {
          const clusterHosts = hostsByCluster.get(cluster.id) || []
          const clusterVmCount = clusterHosts.reduce((sum, h) => sum + (vmsByHost.get(h.id)?.length || 0), 0)
          return (
            <TreeNode
              key={cluster.id}
              icon={Boxes}
              label={cluster.name}
              defaultExpanded
              onNavigate={onNavigate}
              depth={1}
              count={clusterHosts.length}
            >
              {clusterHosts.map(host => {
                const hostVms = vmsByHost.get(host.id) || []
                return (
                  <TreeNode
                    key={host.id}
                    icon={Server}
                    label={host.hostname}
                    to={`/cluster/hosts`}
                    statusDot={hostStatus(host)}
                    defaultExpanded
                    onNavigate={onNavigate}
                    depth={2}
                    count={hostVms.length}
                  >
                    {hostVms.map(vm => (
                      <TreeNode
                        key={vm.id}
                        icon={Monitor}
                        label={vm.name}
                        to={`/vms/${vm.id}`}
                        statusDot={vmStatus(vm)}
                        onNavigate={onNavigate}
                        depth={3}
                      />
                    ))}
                  </TreeNode>
                )
              })}
            </TreeNode>
          )
        })}

        {/* Unassigned hosts */}
        {unassignedHosts.length > 0 && (
          <TreeNode icon={Server} label="Unassigned Hosts" onNavigate={onNavigate} depth={1} count={unassignedHosts.length}>
            {unassignedHosts.map(host => (
              <TreeNode
                key={host.id}
                icon={Server}
                label={host.hostname}
                to={`/cluster/hosts`}
                statusDot={hostStatus(host)}
                onNavigate={onNavigate}
                depth={2}
              >
                {(vmsByHost.get(host.id) || []).map(vm => (
                  <TreeNode
                    key={vm.id}
                    icon={Monitor}
                    label={vm.name}
                    to={`/vms/${vm.id}`}
                    statusDot={vmStatus(vm)}
                    onNavigate={onNavigate}
                    depth={3}
                  />
                ))}
              </TreeNode>
            ))}
          </TreeNode>
        )}

        {/* Unassigned VMs */}
        {unassignedVms.length > 0 && (
          <TreeNode icon={Monitor} label="Unassigned VMs" onNavigate={onNavigate} depth={1} count={unassignedVms.length}>
            {unassignedVms.map(vm => (
              <TreeNode
                key={vm.id}
                icon={Monitor}
                label={vm.name}
                to={`/vms/${vm.id}`}
                statusDot={vmStatus(vm)}
                onNavigate={onNavigate}
                depth={2}
              />
            ))}
          </TreeNode>
        )}

        {/* Datastores */}
        <TreeNode icon={HardDrive} label="Datastores" onNavigate={onNavigate} depth={1} count={datastores.length}>
          {datastores.map(ds => (
            <TreeNode
              key={ds.id}
              icon={Database}
              label={ds.name}
              to="/storage/overview"
              statusDot={ds.status === 'mounted' ? 'green' : ds.status === 'error' ? 'red' : 'yellow'}
              onNavigate={onNavigate}
              depth={2}
            />
          ))}
        </TreeNode>

        {/* CoreSAN */}
        {sanHosts.length > 0 && (
          <TreeNode icon={Boxes} label="CoreSAN" to="/storage/coresan" defaultExpanded onNavigate={onNavigate} depth={1} count={sanHosts.length}>
            {sanHosts.map(host => (
              <TreeNode
                key={host.id}
                icon={Server}
                label={host.hostname}
                to="/storage/coresan"
                statusDot={hostStatus(host)}
                onNavigate={onNavigate}
                depth={2}
              >
                {host.san_volumes > 0 && (
                  <TreeNode
                    icon={Disc}
                    label={`${host.san_volumes} Volume${host.san_volumes !== 1 ? 's' : ''}`}
                    to="/storage/coresan"
                    onNavigate={onNavigate}
                    depth={3}
                  />
                )}
              </TreeNode>
            ))}
          </TreeNode>
        )}

        {/* Networks */}
        <TreeNode icon={Network} label="Networks" to="/networks/overview" onNavigate={onNavigate} depth={1} />
      </TreeNode>
    </div>
  )
}
