# vmm-ui — Developer Guide

This guide covers the internal architecture and development workflow for the vmm-ui web frontend.

## Tech Stack

| Technology | Version | Purpose |
|-----------|---------|---------|
| **React** | 19 | UI framework |
| **TypeScript** | Latest | Type safety |
| **Vite** | Latest | Build tool & dev server |
| **Tailwind CSS** | 4.2 | Utility-first styling |
| **Zustand** | Latest | State management |
| **React Router** | v7 | Client-side routing |
| **Axios** | Latest | HTTP client |
| **Lucide React** | Latest | Icon library |

## Development Setup

```bash
cd apps/vmm-ui

# Install dependencies
npm install

# Start dev server (hot reload)
npm run dev

# Build for production
npm run build

# Lint
npm run lint
```

The dev server runs on `http://localhost:5173` and proxies API requests to `http://localhost:8443`.

## Source Structure

```
apps/vmm-ui/src/
├── App.tsx                     Main app, routing, backend detection
├── main.tsx                    Entry point
├── index.css                   Tailwind + custom styles
│
├── pages/                      Page components (40+ pages)
│   ├── Login.tsx               Authentication
│   ├── Dashboard.tsx           System overview
│   ├── MachinesList.tsx        VM listing
│   ├── VmCreate.tsx            VM creation wizard (with SDN network selection)
│   ├── VmDetail.tsx            VM detail + config editor
│   ├── VmConsole.tsx           Live VGA console
│   ├── Storage*.tsx            Storage pages (5)
│   ├── StorageWizard.tsx       Guided cluster filesystem setup (NFS/Gluster/Ceph)
│   ├── Network*.tsx            Network pages (5)
│   ├── SdnNetworks.tsx         SDN network list + creation
│   ├── SdnNetworkDetail.tsx    Network detail (DHCP leases, DNS, PXE tabs)
│   ├── Settings*.tsx           Settings pages (4+)
│   ├── Users*.tsx              User management
│   ├── Resources*.tsx          Resource groups
│   ├── Terminal*.tsx           In-browser terminal
│   ├── Cluster*.tsx            Cluster-specific pages
│   ├── Hosts*.tsx              Host management
│   ├── Datastores*.tsx         Datastore management
│   ├── Tasks*.tsx              Task tracking
│   ├── Events*.tsx             Event log
│   ├── DRS*.tsx                Resource scheduler
│   └── Alarms*.tsx             Alert system
│
├── components/                 Reusable UI components (32)
│   ├── Layout.tsx              Main layout (sidebar + content)
│   ├── Header.tsx              Top navigation bar
│   ├── Sidebar.tsx             Side navigation
│   ├── ConsoleCanvas.tsx       VGA framebuffer renderer
│   ├── Terminal.tsx            Terminal emulator
│   ├── CreateDiskDialog.tsx    Disk creation dialog
│   ├── AddPoolDialog.tsx       Storage pool dialog
│   └── ...
│
├── stores/                     Zustand state stores
│   ├── authStore.ts            JWT token, login/logout, user info
│   ├── uiStore.ts              Sidebar state, theme, preferences
│   └── clusterStore.ts         Backend mode detection (standalone/cluster)
│
├── api/                        API client definitions
│   ├── vms.ts                  VM endpoints
│   ├── storage.ts              Storage endpoints
│   ├── network.ts              Network endpoints
│   ├── users.ts                User endpoints
│   ├── hosts.ts                Host endpoints (cluster)
│   └── ...
│
├── hooks/                      Custom React hooks
├── utils/                      Utility functions
└── assets/                     Images, logos
```

## Key Patterns

### Backend Detection

On startup, `App.tsx` queries `/api/system/info` to determine if the backend is vmm-server (standalone) or vmm-cluster. The result is stored in `clusterStore` and used to conditionally render cluster-specific pages and navigation.

### Authentication

- JWT token stored in `authStore` (Zustand)
- Axios interceptor adds `Authorization: Bearer <token>` to all requests
- On 401/403 response, user is redirected to Login page
- Token persisted across page reloads

### API Layer

Each API module (`api/vms.ts`, `api/storage.ts`, etc.) exports typed functions:

```typescript
// api/vms.ts
export const listVms = () => axios.get<Vm[]>('/api/vms');
export const createVm = (config: VmCreateRequest) => axios.post<Vm>('/api/vms', config);
export const startVm = (id: string) => axios.post(`/api/vms/${id}/start`);
```

### WebSocket Console

`ConsoleCanvas.tsx` handles:
1. Opens WebSocket to `/ws/console/{vm_id}`
2. Receives binary frames (JPEG-encoded framebuffer)
3. Decodes and renders to a `<canvas>` element
4. Captures keyboard events and sends scancodes
5. Captures mouse events and sends position/button data

### Routing

React Router v7 with nested layouts:

```
/                  → Dashboard
/vms               → MachinesList
/vms/create        → VmCreate
/vms/:id           → VmDetail
/vms/:id/console   → VmConsole
/storage           → StorageOverview
/network           → NetworkOverview
/users             → UsersList
/settings          → SettingsServer
/terminal          → Terminal
/hosts             → HostsList        (cluster only)
/clusters          → ClustersList     (cluster only)
/datastores        → DatastoresList   (cluster only)
/tasks             → TasksList        (cluster only)
/events            → EventsList       (cluster only)
/alarms            → AlarmsList       (cluster only)
/drs               → DRSOverview      (cluster only)
/networks          → SdnNetworks      (cluster only)
/networks/:id      → SdnNetworkDetail (cluster only)
/storage/wizard    → StorageWizard    (cluster only)
```

## Adding a New Page

1. Create the page component in `src/pages/`
2. Add a route in `App.tsx`
3. Add navigation link in `Sidebar.tsx`
4. Create API functions in `src/api/` if needed
5. If cluster-only, gate the route/nav behind `clusterStore.isCluster`

## Adding a New Component

1. Create the component in `src/components/`
2. Use Tailwind CSS for styling — no separate CSS files
3. Use Lucide React for icons
4. Accept typed props with TypeScript interfaces

## Styling

- **Tailwind CSS 4.2** — utility classes for all styling
- **Dark theme** — uses Tailwind's dark mode with `class` strategy
- **Custom styles** in `index.css` for global overrides
- **Consistent spacing** — follow existing component patterns
