# vmm-ui — User Guide

vmm-ui is the web-based management interface for CoreVM. It connects to either vmm-server (standalone mode) or vmm-cluster (cluster mode) and provides a modern, responsive UI for all management tasks.

## Getting Started

### Accessing the UI

After starting vmm-server (or vmm-cluster), open your browser:

- **Standalone:** `http://localhost:5173` (dev server) or served by vmm-server directly
- **Cluster:** Same URL, pointing to the vmm-cluster address

### Login

Default credentials:
- **Username:** `admin`
- **Password:** `admin`

## Dashboard

The dashboard provides an overview of your infrastructure:

- **System Metrics** — CPU usage, memory utilization, uptime
- **VM Overview** — running/stopped/total VMs at a glance
- **Storage** — disk usage across pools
- **Network** — active interfaces and traffic
- **Recent Activity** — audit trail of recent operations

## Virtual Machines

### Creating a VM

1. Navigate to **Virtual Machines** → **Create**
2. Configure:
   - **Name** — descriptive name for the VM
   - **CPU** — number of vCPUs
   - **RAM** — memory allocation in MB
   - **BIOS** — CoreVM BIOS or SeaBIOS
   - **Disk** — create a new disk image or attach an existing one
   - **Network** — Disconnected, User-mode NAT (SLIRP), or Bridge
   - **Boot Order** — disk first, CD first, or floppy first
3. Click **Create**

### Managing VMs

From the VM list, you can:
- **Start** / **Stop** / **Force Stop** a VM
- **Delete** a VM (stopped only)
- Click a VM name to view details and edit configuration

### VM Detail Page

- **Overview** — current state, CPU/RAM config, uptime
- **Configuration** — edit CPU, RAM, BIOS, disk, network settings
- **Console** — open the live VGA console

### Live Console

The console provides a real-time VGA display of the VM:
- **Keyboard input** — type directly in the console
- **Mouse input** — click and move within the console area
- Rendered via Canvas element with WebSocket framebuffer streaming

## Storage

### Storage Pools

Storage pools organize your disk images and ISOs:
- **Browse** pool contents
- **Create** new pools pointing to directories on the host

### Disk Images

- **Create** new raw disk images (specify size)
- **Resize** existing disk images
- **Delete** unused images

### ISO Management

- **Upload** ISO files for guest OS installation
- **Delete** ISOs no longer needed

## Network

- **Interfaces** — view host network interfaces and their status
- **Statistics** — monitor network I/O (bytes, packets)

## Users & Groups

### User Management (Admin)

- **Create** new user accounts
- **Edit** user details and roles
- **Delete** users
- **Change passwords**

### Roles

| Role | Permissions |
|------|------------|
| **Admin** | Full access to all features |
| **User** | Access to assigned VMs and resources |

### Resource Groups

Organize VMs into groups with granular permissions.

## Settings

- **Server** — view/edit server configuration
- **Timezone** — set the timezone for logs and UI
- **Security** — session timeout, password policies
- **UI Preferences** — sidebar state, theme

## Terminal

An in-browser terminal for executing commands on the server:
- Navigate to **Terminal** in the sidebar
- Runs commands via WebSocket connection to vmm-server

## Cluster Features

When connected to vmm-cluster, additional pages appear:

### Hosts

- View all registered nodes with health status
- Add new hosts to the cluster
- Enter/exit maintenance mode
- View per-host metrics (CPU, RAM, VMs)

### Cluster Dashboard

- Cluster-wide metrics aggregated from all nodes
- Node health overview
- Resource utilization heat map

### SDN Networks

- Create cluster-wide virtual networks with CIDR, VLAN, DHCP, DNS
- Network detail page with tabs for:
  - **DHCP Leases** — active leases and static reservations
  - **DNS Records** — A records and CNAME records
  - **PXE Configuration** — boot file, TFTP root, next-server
- Input validation (CIDR, IP, MAC format)

### Storage Wizard

Guided 4-step wizard for setting up cluster shared storage:
1. Choose filesystem type (NFS, GlusterFS, CephFS) and cluster
2. Check package status on selected hosts
3. Install missing packages (with sudo password support)
4. Configure and mount the filesystem on all hosts

### Datastores

- Manage cluster-wide shared storage
- View datastore capacity across nodes

### Migration

- Migrate VMs between nodes (from VM detail page)
- Direct host-to-host transfer with progress tracking
- View migration history in Tasks

### DRS (Distributed Resource Scheduler)

- View current resource distribution
- See DRS recommendations
- Configure DRS mode (manual/automatic)
- Manage DRS exclusions (per-VM or per-resource-group)

### Notifications

- Configure notification channels (email, webhook, log)
- Create notification rules with severity and category filters
- Test channels with a test notification
- View notification history and delivery status

### Tasks

- Track long-running operations (migrations, provisioning)
- View task status, progress, and history

### Events

- Cluster-wide event log
- Filter by type, time range, severity

### Alarms

- View active alerts
- Configure alarm thresholds

### Cluster Settings

- LDAP / Active Directory configuration
- DRS exclusion management
- SMTP settings for email notifications

## Responsive Design

vmm-ui is fully responsive:
- **Desktop** — full sidebar navigation, wide tables, detailed views
- **Tablet** — collapsible sidebar, adapted layouts
- **Mobile** — compact views, touch-friendly controls, mobile-optimized console
