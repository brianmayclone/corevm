# Datenbankstruktur

CoreVM verwendet **SQLite** mit der **rusqlite**-Bibliothek (v0.33). Es gibt zwei unabhängige Datenbanken:

| Komponente | Datei | Beschreibung |
|------------|-------|-------------|
| **vmm-server** | `.vmm/vms/vmm.db` | Einzelner Host — lokale VM-Verwaltung |
| **vmm-cluster** | konfigurierbar (z.B. `/var/lib/vmm/vmm-cluster.db`) | Cluster-Autorität — zentraler Single Source of Truth |

Beide Datenbanken verwenden `PRAGMA journal_mode=WAL` und `PRAGMA foreign_keys=ON`.

---

## vmm-server (12 Tabellen)

Schema-Quelle: `apps/vmm-server/src/db/mod.rs`

### ER-Diagramm

```
users ─────────┬────────> vms ──────────┬────> snapshots
               │           │            ├────> port_forwards
               │           │            └────> disk_images
               │           │
               ├────> audit_log         storage_pools ──┬──> disk_images
               │                                        └──> isos
               └────> group_members <──── groups
                                            │
resource_groups <── resource_group_permissions
```

### users

Benutzerkonten mit rollenbasiertem Zugriff. Beim ersten Start wird ein Admin-User geseedet (`admin`/`admin`, Argon2-Hash).

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `username` | TEXT UNIQUE | Login-Name |
| `password_hash` | TEXT | Argon2-Passworthash |
| `role` | TEXT | `admin`, `operator`, `viewer` (Default: `operator`) |
| `created_at` | TEXT | Erstellzeitpunkt |
| `updated_at` | TEXT | Letzte Änderung |

### vms

VM-Konfigurationen. Die vollständige VM-Config wird als JSON gespeichert.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | TEXT PK | UUID |
| `name` | TEXT | Anzeigename |
| `description` | TEXT | Beschreibung |
| `config_json` | TEXT | Vollständige VM-Konfiguration (JSON) |
| `owner_id` | INTEGER FK→users | Ersteller/Besitzer |
| `resource_group_id` | INTEGER FK→resource_groups | Ressourcengruppe (Default: 1, via Migration) |
| `created_at` | TEXT | Erstellzeitpunkt |
| `updated_at` | TEXT | Letzte Änderung |

### storage_pools

Speicher-Pools für Disk-Images und ISOs. Unterstützt lokale und shared-Storage-Typen.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `name` | TEXT | Pool-Name |
| `path` | TEXT UNIQUE | Pfad im Dateisystem |
| `pool_type` | TEXT | `local`, `nfs`, `cephfs`, `glusterfs` |
| `shared` | INTEGER | 0 = lokal, 1 = von mehreren Hosts erreichbar |
| `mount_source` | TEXT | NFS: `server:/export`, CephFS: `mon1,mon2:/path` |
| `mount_opts` | TEXT | Mount-Optionen (z.B. `vers=4,noatime`) |
| `created_at` | TEXT | Erstellzeitpunkt |

### disk_images

VM-Disk-Dateien, zugeordnet zu einem Storage-Pool und optional einer VM.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `name` | TEXT | Dateiname |
| `path` | TEXT UNIQUE | Vollständiger Pfad |
| `size_bytes` | INTEGER | Grösse in Bytes |
| `format` | TEXT | `raw`, `qcow2`, etc. |
| `pool_id` | INTEGER FK→storage_pools | Zugehöriger Pool |
| `vm_id` | TEXT FK→vms (ON DELETE SET NULL) | Zugeordnete VM |
| `created_at` | TEXT | Erstellzeitpunkt |

### isos

ISO-Installationsmedien.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `name` | TEXT | ISO-Name |
| `path` | TEXT UNIQUE | Vollständiger Pfad |
| `size_bytes` | INTEGER | Grösse in Bytes |
| `pool_id` | INTEGER FK→storage_pools | Zugehöriger Pool |
| `uploaded_at` | TEXT | Upload-Zeitpunkt |

### snapshots

VM-Snapshots. Werden beim Löschen der VM kaskadiert gelöscht.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `vm_id` | TEXT FK→vms (CASCADE) | Zugehörige VM |
| `name` | TEXT | Snapshot-Name |
| `description` | TEXT | Beschreibung |
| `disk_snapshot_path` | TEXT | Pfad zum Disk-Snapshot |
| `created_at` | TEXT | Erstellzeitpunkt |

### port_forwards

Netzwerk-Portweiterleitung pro VM.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `vm_id` | TEXT FK→vms (CASCADE) | Zugehörige VM |
| `protocol` | TEXT | `tcp` oder `udp` |
| `host_port` | INTEGER | Host-Port |
| `guest_port` | INTEGER | Guest-Port |
| `host_ip` | TEXT | Bind-Adresse (Default: `0.0.0.0`) |

### audit_log

Protokollierung aller Benutzeraktionen.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `user_id` | INTEGER FK→users | Ausführender Benutzer |
| `action` | TEXT | Durchgeführte Aktion |
| `target_type` | TEXT | Zieltyp (z.B. `vm`, `user`) |
| `target_id` | TEXT | Ziel-ID |
| `details` | TEXT | Zusätzliche Details |
| `created_at` | TEXT | Zeitstempel |

### groups

Benutzergruppen mit einer zugewiesenen Rolle.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `name` | TEXT UNIQUE | Gruppenname |
| `role` | TEXT | `admin`, `operator`, `viewer` (Default: `viewer`) |
| `description` | TEXT | Beschreibung |

### group_members

Verknüpfung Benutzer ↔ Gruppen (Composite PK).

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `group_id` | INTEGER FK→groups (CASCADE) | Gruppe |
| `user_id` | INTEGER FK→users (CASCADE) | Benutzer |

### resource_groups

Organisatorische Gruppierung von VMs. "All Machines" (ID=1) ist die Default-Gruppe und kann nicht gelöscht werden.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `name` | TEXT UNIQUE | Gruppenname |
| `description` | TEXT | Beschreibung |
| `is_default` | INTEGER | 1 = Standard-Gruppe |
| `created_at` | TEXT | Erstellzeitpunkt |

### resource_group_permissions

RBAC: Welche Benutzergruppe hat welche Rechte auf welcher Ressourcengruppe. Permissions als CSV: `vm.create`, `vm.edit`, `vm.delete`, `vm.start_stop`, `vm.console`, `network.edit`, `storage.edit`, `snapshots.manage`.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `resource_group_id` | INTEGER FK→resource_groups (CASCADE) | Ressourcengruppe |
| `group_id` | INTEGER FK→groups (CASCADE) | Benutzergruppe |
| `permissions` | TEXT | Komma-separierte Berechtigungen |

**UNIQUE-Constraint** auf `(resource_group_id, group_id)`.

---

## vmm-cluster (31 Tabellen)

Schema-Quelle: `apps/vmm-cluster/src/db/mod.rs`

Der Cluster ist der **Single Source of Truth** — Nodes sind Ausführungsagenten. Die Cluster-Datenbank enthält alle Tabellen des vmm-server (in erweiterter Form) plus zusätzliche Tabellen für Cluster-Topologie, DRS, HA, Benachrichtigungen und Netzwerkdienste.

### ER-Diagramm

```
clusters ─────┬────> hosts ──────────────> datastore_hosts <── datastores
              │        │                                          ├──> disk_images
              │        ├── drs_recommendations                    └──> isos
              │        └── migrations
              │
              ├────> vms ──────> ha_vm_overrides
              │        │
              │        └── drs_recommendations
              │
              ├────> drs_rules
              ├────> drs_exclusions
              ├────> virtual_networks ──> pxe_boot_entries
              └────> network_services ──┬──> dhcp_leases
                                        └──> dns_records

users ────┬──> group_members <── groups ──> resource_group_permissions <── resource_groups
          ├──> tasks
          ├──> migrations
          └──> audit_log

notification_channels <── notification_rules ──> notification_log
```

### Cluster-Topologie

#### clusters

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | TEXT PK | UUID |
| `name` | TEXT UNIQUE | Cluster-Name |
| `description` | TEXT | Beschreibung |
| `drs_enabled` | INTEGER | DRS aktiv (Default: 1) |
| `ha_enabled` | INTEGER | HA aktiv (Default: 1) |
| `ha_host_monitoring` | INTEGER | Host-Monitoring (Default: 1) |
| `ha_vm_restart_priority` | TEXT | `low`, `medium`, `high` (Default: `medium`) |
| `ha_admission_control` | INTEGER | Admission Control aktiv (Default: 1) |
| `ha_failover_hosts` | INTEGER | Anzahl Reserve-Hosts (Default: 1) |
| `created_at` / `updated_at` | TEXT | Zeitstempel |

#### hosts

Physische oder virtuelle Hosts (vmm-server Nodes). Hardware-Daten und CPU-Auslastung werden via Heartbeat aktualisiert.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | TEXT PK | UUID |
| `hostname` | TEXT | Hostname |
| `display_name` | TEXT | Benutzerdefinierter Anzeigename |
| `address` | TEXT UNIQUE | Netzwerkadresse |
| `cluster_id` | TEXT FK→clusters | Zugehöriger Cluster |
| `agent_token` | TEXT | Authentifizierungstoken |
| `cpu_model` | TEXT | CPU-Modell |
| `cpu_cores` / `cpu_threads` | INTEGER | CPU-Kerne/Threads |
| `total_ram_mb` / `free_ram_mb` | INTEGER | RAM total/frei (MB) |
| `hw_virtualization` | INTEGER | Hardware-Virtualisierung verfügbar |
| `cpu_usage_pct` | REAL | CPU-Auslastung (%) |
| `status` | TEXT | `connecting`, `connected`, `disconnected`, `error` |
| `maintenance_mode` | INTEGER | Wartungsmodus |
| `connection_state` | TEXT | Verbindungsstatus |
| `last_heartbeat` | TEXT | Letzter Heartbeat |
| `version` | TEXT | Agent-Version |
| `registered_at` | TEXT | Registrierungszeitpunkt |

### Virtuelle Maschinen

#### vms (Cluster)

Erweitert um Cluster-Zuordnung, HA- und DRS-Einstellungen.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | TEXT PK | UUID |
| `name` / `description` | TEXT | Name/Beschreibung |
| `cluster_id` | TEXT FK→clusters | Cluster |
| `host_id` | TEXT FK→hosts | Aktueller Host (kann NULL sein) |
| `config_json` | TEXT | VM-Konfiguration (JSON) |
| `state` | TEXT | `stopped`, `running`, `paused`, etc. |
| `ha_protected` | INTEGER | HA-Schutz aktiv (Default: 1) |
| `ha_restart_priority` | TEXT | Neustart-Priorität |
| `drs_automation` | TEXT | `manual`, `partially_automated`, `fully_automated` |
| `drs_excluded` | INTEGER | Von DRS ausgeschlossen |
| `resource_group_id` | INTEGER FK→resource_groups | Ressourcengruppe |
| `owner_id` | INTEGER FK→users | Besitzer |
| `created_at` / `updated_at` | TEXT | Zeitstempel |

#### ha_vm_overrides

Pro-VM HA-Einstellungen, die die Cluster-Defaults überschreiben.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `vm_id` | TEXT PK FK→vms (CASCADE) | VM |
| `restart_priority` | TEXT | Neustart-Priorität |
| `isolation_response` | TEXT | Reaktion bei Host-Isolation |

### Storage

#### datastores

Cluster-weite Speicher-Pools.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | TEXT PK | UUID |
| `name` | TEXT UNIQUE | Datastore-Name |
| `store_type` | TEXT | `local`, `nfs`, `cephfs`, `glusterfs` |
| `mount_source` / `mount_opts` / `mount_path` | TEXT | Mount-Konfiguration |
| `cluster_id` | TEXT FK→clusters | Cluster |
| `total_bytes` / `free_bytes` | INTEGER | Kapazität |
| `status` | TEXT | `creating`, `active`, `error`, `degraded` |
| `created_at` | TEXT | Erstellzeitpunkt |

#### datastore_hosts

Mount-Status eines Datastores pro Host (Composite PK).

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `datastore_id` | TEXT FK→datastores (CASCADE) | Datastore |
| `host_id` | TEXT FK→hosts (CASCADE) | Host |
| `mounted` | INTEGER | 1 = gemountet |
| `mount_status` | TEXT | `pending`, `mounted`, `error` |
| `total_bytes` / `free_bytes` | INTEGER | Kapazität auf diesem Host |
| `last_check` | TEXT | Letzte Prüfung |

#### disk_images (Cluster)

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | TEXT PK | UUID |
| `name` | TEXT | Dateiname |
| `datastore_id` | TEXT FK→datastores | Datastore |
| `path` | TEXT | Pfad innerhalb des Datastores |
| `size_bytes` | INTEGER | Grösse |
| `format` | TEXT | `raw`, `qcow2` |
| `vm_id` | TEXT FK→vms (ON DELETE SET NULL) | Zugeordnete VM |
| `created_at` | TEXT | Erstellzeitpunkt |

#### isos (Cluster)

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | TEXT PK | UUID |
| `name` | TEXT | ISO-Name |
| `datastore_id` | TEXT FK→datastores | Datastore |
| `path` | TEXT | Pfad |
| `size_bytes` | INTEGER | Grösse |
| `uploaded_at` | TEXT | Upload-Zeitpunkt |

### Benutzer & Berechtigungen

Die Tabellen `users`, `groups`, `group_members`, `resource_group_permissions` sind identisch mit vmm-server.

#### resource_groups (Cluster)

Erweitert um Cluster-Zuordnung und DRS-Ausschluss.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `name` | TEXT UNIQUE | Gruppenname |
| `description` | TEXT | Beschreibung |
| `is_default` | INTEGER | Standard-Gruppe |
| `cluster_id` | TEXT FK→clusters (ON DELETE SET NULL) | Cluster (NULL = global) |
| `drs_excluded` | INTEGER | VMs in dieser Gruppe von DRS ausgeschlossen |
| `created_at` | TEXT | Erstellzeitpunkt |

### Operationen & Events

#### tasks

Langlebige Operationen (z.B. VM-Migration, Disk-Erstellung).

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | TEXT PK | UUID |
| `task_type` | TEXT | Aufgabentyp |
| `status` | TEXT | `queued`, `running`, `completed`, `failed` |
| `progress_pct` | INTEGER | Fortschritt 0–100 |
| `target_type` / `target_id` | TEXT | Zielobjekt |
| `initiated_by` | INTEGER FK→users | Initiator |
| `details_json` | TEXT | Zusätzliche Details (JSON) |
| `error` | TEXT | Fehlermeldung |
| `created_at` / `started_at` / `completed_at` | TEXT | Zeitstempel |

#### events

System-Events für Monitoring und Alarme.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `severity` | TEXT | `info`, `warning`, `error`, `critical` |
| `category` | TEXT | `ha`, `drs`, `host`, `vm`, `datastore`, `alarm`, `task` |
| `message` | TEXT | Event-Nachricht |
| `target_type` / `target_id` | TEXT | Zielobjekt |
| `host_id` | TEXT FK→hosts | Betroffener Host |
| `created_at` | TEXT | Zeitstempel |

#### audit_log (Cluster)

Identisch mit vmm-server.

#### migrations

VM-Migrationshistorie.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `vm_id` / `vm_name` | TEXT | Migrierte VM |
| `source_host_id` | TEXT FK→hosts | Quell-Host |
| `target_host_id` | TEXT FK→hosts | Ziel-Host |
| `migration_type` | TEXT | `cold`, `live` |
| `reason` | TEXT | `manual`, `drs`, `ha`, `maintenance` |
| `status` | TEXT | `pending`, `running`, `completed`, `failed` |
| `initiated_by` | INTEGER FK→users | Initiator |
| `started_at` / `completed_at` | TEXT | Zeitstempel |
| `error` | TEXT | Fehlermeldung |

### DRS (Distributed Resource Scheduler)

#### drs_rules

Konfigurierbare Schwellenwerte und Aktionen für den DRS-Engine.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `cluster_id` | TEXT FK→clusters (CASCADE) | Cluster |
| `name` | TEXT | Regel-Name |
| `enabled` | INTEGER | Aktiv |
| `metric` | TEXT | `cpu_usage`, `ram_usage`, `vm_count_imbalance` |
| `threshold` | REAL | Schwellenwert in % (z.B. 80.0 = 80%) |
| `action` | TEXT | `recommend` (manuell) oder `auto_migrate` |
| `cooldown_secs` | INTEGER | Minimum-Sekunden zwischen Empfehlungen (Default: 3600) |
| `priority` | TEXT | `low`, `medium`, `high`, `critical` |
| `created_at` | TEXT | Erstellzeitpunkt |

#### drs_recommendations

Vom DRS-Engine generierte Migrationsempfehlungen.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `cluster_id` | TEXT FK→clusters (CASCADE) | Cluster |
| `vm_id` | TEXT FK→vms (CASCADE) | Betroffene VM |
| `source_host_id` / `target_host_id` | TEXT FK→hosts | Von/Nach Host |
| `reason` | TEXT | Begründung |
| `priority` | TEXT | `low`, `medium`, `high`, `critical` |
| `status` | TEXT | `pending`, `applied`, `dismissed` |
| `created_at` | TEXT | Zeitstempel |

#### drs_exclusions

VMs oder Ressourcengruppen, die von DRS ausgeschlossen sind.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `cluster_id` | TEXT FK→clusters (CASCADE) | Cluster |
| `exclusion_type` | TEXT | `vm` oder `resource_group` |
| `target_id` | TEXT | VM-ID oder Resource-Group-ID |
| `reason` | TEXT | Begründung |
| `created_at` | TEXT | Zeitstempel |

**UNIQUE-Constraint** auf `(cluster_id, exclusion_type, target_id)`.

### Alarme

#### alarms

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `name` | TEXT | Alarm-Name |
| `target_type` / `target_id` | TEXT | Überwachtes Objekt |
| `condition_type` | TEXT | Bedingungstyp |
| `threshold` | REAL | Schwellenwert |
| `severity` | TEXT | `warning`, `error`, `critical` |
| `triggered` | INTEGER | 1 = ausgelöst |
| `acknowledged` | INTEGER | 1 = bestätigt |
| `created_at` / `triggered_at` | TEXT | Zeitstempel |

### Benachrichtigungen

#### notification_channels

Zustellziele für Benachrichtigungen.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `name` | TEXT UNIQUE | Kanal-Name |
| `channel_type` | TEXT | `email`, `webhook`, `log` |
| `enabled` | INTEGER | Aktiv |
| `config_json` | TEXT | Typ-spezifische Konfiguration (JSON) |
| `created_at` | TEXT | Erstellzeitpunkt |

**config_json** je nach Typ:
- `email`: `{ "smtp_host", "smtp_port", "smtp_user", "smtp_pass", "from", "to" }`
- `webhook`: `{ "url", "method", "headers", "secret" }`
- `log`: `{ "level" }`

#### notification_rules

Welche Events welche Kanäle auslösen.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `name` | TEXT | Regel-Name |
| `enabled` | INTEGER | Aktiv |
| `event_category` | TEXT | `ha`, `drs`, `host`, `vm`, `datastore`, `alarm`, `task`, `*` (alle) |
| `min_severity` | TEXT | Minimum-Severity: `info`, `warning`, `error`, `critical` |
| `channel_id` | INTEGER FK→notification_channels (CASCADE) | Zielkanal |
| `cooldown_secs` | INTEGER | Throttle in Sekunden (Default: 300) |
| `cluster_id` | TEXT FK→clusters (CASCADE) | Optional: auf Cluster filtern |
| `created_at` | TEXT | Erstellzeitpunkt |

#### notification_log

Historie gesendeter Benachrichtigungen.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `rule_id` | INTEGER FK→notification_rules | Auslösende Regel |
| `channel_id` | INTEGER FK→notification_channels | Zustellkanal |
| `event_id` | INTEGER | Auslösendes Event |
| `status` | TEXT | `sent`, `failed`, `throttled` |
| `error` | TEXT | Fehlermeldung |
| `sent_at` | TEXT | Zeitstempel |

### Netzwerkdienste

#### virtual_networks

Cluster-verwaltete Netzwerke mit optionalem DHCP, DNS und PXE.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `cluster_id` | TEXT FK→clusters (CASCADE) | Cluster |
| `name` | TEXT | Netzwerk-Name |
| `vlan_id` | INTEGER | VLAN-ID (NULL = untagged) |
| `subnet` | TEXT | z.B. `10.0.0.0/24` |
| `gateway` | TEXT | z.B. `10.0.0.1` |
| `dhcp_enabled` | INTEGER | DHCP aktiv |
| `dhcp_range_start` / `dhcp_range_end` | TEXT | DHCP-Bereich |
| `dhcp_lease_secs` | INTEGER | Lease-Dauer (Default: 3600) |
| `dns_enabled` | INTEGER | DNS aktiv |
| `dns_domain` | TEXT | z.B. `vm.local` |
| `dns_upstream` | TEXT | z.B. `8.8.8.8,8.8.4.4` |
| `pxe_enabled` | INTEGER | PXE aktiv |
| `pxe_boot_file` / `pxe_tftp_root` / `pxe_next_server` | TEXT | PXE-Konfiguration |
| `auto_register_dns` | INTEGER | Auto-DNS für VM-Namen (Default: 1) |
| `created_at` | TEXT | Erstellzeitpunkt |

**UNIQUE-Constraint** auf `(cluster_id, name)`.

#### network_services (Legacy)

Backward-Kompatibilität — `virtual_networks` ist das primäre Modell.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `cluster_id` | TEXT FK→clusters (CASCADE) | Cluster |
| `service_type` | TEXT | `dhcp`, `dns`, `pxe` |
| `enabled` | INTEGER | Aktiv |
| `config_json` | TEXT | Konfiguration (JSON) |
| `created_at` | TEXT | Erstellzeitpunkt |

**UNIQUE-Constraint** auf `(cluster_id, service_type)`.

#### dhcp_leases

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `network_service_id` | INTEGER FK→network_services (CASCADE) | Netzwerkdienst |
| `mac_address` | TEXT | MAC-Adresse |
| `ip_address` | TEXT | Zugewiesene IP |
| `hostname` | TEXT | Hostname |
| `vm_id` | TEXT FK→vms (ON DELETE SET NULL) | Zugehörige VM |
| `lease_start` / `lease_end` | TEXT | Lease-Zeitraum |

**UNIQUE-Constraint** auf `(network_service_id, mac_address)`.

#### dns_records

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `network_service_id` | INTEGER FK→network_services (CASCADE) | Netzwerkdienst |
| `record_type` | TEXT | `A`, `AAAA`, `CNAME`, `PTR` |
| `name` | TEXT | Record-Name |
| `value` | TEXT | Record-Wert |
| `ttl` | INTEGER | Time to Live (Default: 3600) |
| `auto_registered` | INTEGER | 1 = automatisch aus VM-Name erstellt |

**UNIQUE-Constraint** auf `(network_service_id, record_type, name)`.

#### pxe_boot_entries

PXE-Boot-Menüeinträge pro Netzwerk.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `network_id` | INTEGER FK→virtual_networks (CASCADE) | Netzwerk |
| `name` | TEXT | Menü-Label (z.B. "Ubuntu 24.04 Server") |
| `iso_id` | TEXT FK→isos (ON DELETE SET NULL) | Verknüpftes ISO |
| `iso_path` | TEXT | Direkter Pfad (falls kein ISO-Record) |
| `boot_args` | TEXT | Kernel-Argumente |
| `sort_order` | INTEGER | Sortierung |
| `enabled` | INTEGER | Aktiv |
| `created_at` | TEXT | Erstellzeitpunkt |

### Cluster-Konfiguration

#### cluster_settings

Key-Value Store für globale Konfiguration.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `key` | TEXT PK | Schlüssel |
| `value` | TEXT | Wert |
| `category` | TEXT | `smtp`, `ldap`, `dhcp`, `dns`, `general` |

### LDAP / Active Directory

#### ldap_configs

LDAP/AD-Integrationseinstellungen für zentralisierte Authentifizierung.

| Spalte | Typ | Beschreibung |
|--------|-----|-------------|
| `id` | INTEGER PK | Auto-Increment |
| `name` | TEXT UNIQUE | Konfig-Name |
| `enabled` | INTEGER | Aktiv |
| `server_url` | TEXT | z.B. `ldap://dc.example.com:389` |
| `bind_dn` | TEXT | Bind-DN |
| `bind_password` | TEXT | Bind-Passwort |
| `base_dn` | TEXT | Basis-DN |
| `user_search_dn` | TEXT | Benutzer-Suchbasis |
| `user_filter` | TEXT | Benutzer-Filter (Default: `(&(objectClass=user)(sAMAccountName={username}))`) |
| `group_search_dn` | TEXT | Gruppen-Suchbasis |
| `group_filter` | TEXT | Gruppen-Filter |
| `attr_username` / `attr_email` / `attr_display` | TEXT | Attribut-Mappings |
| `role_mapping` | TEXT | JSON: `{ "AD-Gruppe": "rolle" }` |
| `use_tls` / `skip_tls_verify` | INTEGER | TLS-Einstellungen |
| `created_at` | TEXT | Erstellzeitpunkt |

---

---

## JSON-Strukturen

### config_json (VmConfig)

Die Spalte `config_json` in der Tabelle `vms` enthält die vollständige VM-Konfiguration als serialisiertes JSON. Definiert in `libs/vmm-core/src/config.rs` als `VmConfig`.

```json
{
  "uuid": "a1b2c3d4e5f6...",
  "name": "My VM",
  "guest_os": "win10",
  "guest_arch": "x64",
  "ram_mb": 4096,
  "cpu_cores": 4,
  "disk_images": ["/path/to/disk1.raw", "/path/to/disk2.raw"],
  "iso_image": "/path/to/installer.iso",
  "boot_order": "diskfirst",
  "bios_type": "seabios",
  "gpu_model": "stdvga",
  "vram_mb": 16,
  "nic_model": "e1000",
  "net_enabled": true,
  "net_mode": "usermode",
  "net_host_nic": "",
  "mac_mode": "dynamic",
  "mac_address": "",
  "audio_enabled": true,
  "usb_tablet": false,
  "ram_alloc": "ondemand",
  "diagnostics": false,
  "disk_cache_mb": 0,
  "disk_cache_mode": "none",
  "sdn_config": null
}
```

#### Felder

| Feld | Typ | Default | Beschreibung |
|------|-----|---------|-------------|
| `uuid` | string | UUID v4 (ohne Bindestriche) | Eindeutige VM-ID |
| `name` | string | `"New VM"` | Anzeigename |
| `guest_os` | enum | `"other"` | Gastbetriebssystem (siehe unten) |
| `guest_arch` | enum | `"x64"` | `x86` oder `x64` |
| `ram_mb` | u32 | `256` | RAM in MB |
| `cpu_cores` | u32 | `1` | Anzahl CPU-Kerne |
| `disk_images` | string[] | `[]` | Pfade zu Disk-Images (erste = primäre Disk) |
| `iso_image` | string | `""` | Pfad zum ISO-Image |
| `boot_order` | enum | `"cdfirst"` | `diskfirst`, `cdfirst`, `floppyfirst` |
| `bios_type` | enum | `"seabios"` | `corevm`, `seabios`, `uefi` |
| `gpu_model` | enum | `"stdvga"` | `stdvga`, `virtiogpu` |
| `vram_mb` | u32 | `16` | Video-RAM in MB |
| `nic_model` | enum | `"e1000"` | `e1000`, `virtionet` |
| `net_enabled` | bool | `false` | Netzwerk aktiviert |
| `net_mode` | enum | `"usermode"` | `disconnected`, `usermode`, `bridge` |
| `net_host_nic` | string | `""` | Host-NIC für Bridge-Modus |
| `mac_mode` | enum | `"dynamic"` | `dynamic`, `static` |
| `mac_address` | string | `""` | Statische MAC-Adresse (wenn `mac_mode` = `static`) |
| `audio_enabled` | bool | `true` | AC'97 Audio aktiviert |
| `usb_tablet` | bool | `false` | USB-Tablet-Modus (absolute Mausposition) |
| `ram_alloc` | enum | `"ondemand"` | `preallocate`, `ondemand` |
| `diagnostics` | bool | `false` | Diagnose-Modus |
| `disk_cache_mb` | u32 | `0` | Disk-Cache-Grösse in MB |
| `disk_cache_mode` | enum | `"none"` | `writeback`, `writethrough`, `none` |
| `sdn_config` | object\|null | `null` | SDN-Netzwerkkonfiguration (nur bei Cluster-Betrieb) |

#### guest_os Werte

| Wert | Anzeigename |
|------|-------------|
| `other` | Other |
| `win7` | Windows 7 |
| `win8` | Windows 8/8.1 |
| `win10` | Windows 10 |
| `win11` | Windows 11 |
| `winserver2016` | Windows Server 2016 |
| `winserver2019` | Windows Server 2019 |
| `winserver2022` | Windows Server 2022 |
| `ubuntu` | Ubuntu |
| `debian` | Debian |
| `fedora` | Fedora |
| `opensuse` | openSUSE |
| `redhat` | Red Hat / CentOS |
| `arch` | Arch Linux |
| `linux` | Linux (Other) |
| `freebsd` | FreeBSD |
| `dos` | DOS / FreeDOS |

#### sdn_config (SdnNetConfig)

Optionale SDN-Netzwerkkonfiguration, die vom vmm-cluster an Nodes gepusht wird. Überschreibt die SLIRP-Standardwerte.

```json
{
  "net_prefix": [10, 0, 50],
  "gateway_ip": [10, 0, 50, 1],
  "dns_ip": [10, 0, 50, 1],
  "guest_ip": [10, 0, 50, 100],
  "netmask": [255, 255, 255, 0],
  "upstream_dns": ["8.8.8.8", "8.8.4.4"],
  "dns_domain": "vm.local",
  "pxe_boot_file": "pxelinux.0",
  "pxe_next_server": [10, 0, 50, 1]
}
```

| Feld | Typ | Beschreibung |
|------|-----|-------------|
| `net_prefix` | [u8; 3] | Subnetz-Präfix (erste 3 Oktette) |
| `gateway_ip` | [u8; 4] | Gateway-IP |
| `dns_ip` | [u8; 4] | DNS-Server-IP |
| `guest_ip` | [u8; 4] | Dem Gast zugewiesene IP |
| `netmask` | [u8; 4] | Netzmaske |
| `upstream_dns` | string[] | Upstream-DNS-Server (leer = Host-Resolver) |
| `dns_domain` | string | DNS-Domain-Suffix für Auto-Registrierung |
| `pxe_boot_file` | string | PXE-Boot-Datei (leer = PXE deaktiviert) |
| `pxe_next_server` | [u8; 4] | PXE TFTP-Server-IP |

### notification_channels.config_json

Typ-spezifische Konfiguration für Benachrichtigungskanäle.

**Email:**
```json
{
  "smtp_host": "smtp.example.com",
  "smtp_port": 587,
  "smtp_user": "alerts@example.com",
  "smtp_pass": "secret",
  "from": "alerts@example.com",
  "to": "admin@example.com"
}
```

**Webhook:**
```json
{
  "url": "https://hooks.example.com/notify",
  "method": "POST",
  "headers": { "Authorization": "Bearer token" },
  "secret": "hmac-secret"
}
```

**Log:**
```json
{
  "level": "warning"
}
```

---

## Migrationen

Beide Datenbanken verwenden einen pragmatischen Ansatz ohne externes Migrations-Framework:

- Schema wird inline in `db/mod.rs` mit `CREATE TABLE IF NOT EXISTS` definiert
- Spalten-Migrationen werden programmatisch durchgeführt (z.B. `migrate_vms_resource_group()` in vmm-server)
- Seed-Daten: Default-Admin-User (`admin`/`admin`) und Default-Ressourcengruppe "All Machines"

## Rollen

| Rolle | Beschreibung |
|-------|-------------|
| `admin` | Vollzugriff auf alle Funktionen |
| `operator` | VM-Verwaltung und tägliche Operationen |
| `viewer` | Nur-Lese-Zugriff |
