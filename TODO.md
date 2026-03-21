# VMM-Cluster — TODO

Offene Implementierungen, sortiert nach Priorität.
Stand: 2026-03-21

---

## CRITICAL — Produktions-Blocker

### 1. Managed-Mode auf vmm-server durchsetzen

**Problem:** Wenn ein vmm-server als Agent beim Cluster registriert ist, zeigt er
`mode: "managed"` in `/api/system/info` an, blockiert aber keinen einzigen
API-Aufruf. Ein Benutzer kann direkt auf dem Host VMs anlegen, löschen oder
konfigurieren — der Cluster weiß davon nichts.

**Aufgaben:**
- [ ] Middleware in `vmm-server/src/api/mod.rs` einbauen die bei `managed_config.is_some()` alle `/api/*`-Routen mit `403 managed_by_cluster` beantwortet
- [ ] Ausnahmen definieren: `/api/system/info`, `/api/auth/login` (damit UI den Managed-Hinweis anzeigen kann)
- [ ] Agent-Routen (`/agent/*`) und WebSocket (`/ws/*`) weiterhin erlauben
- [ ] Test: Nach Registrierung darf `POST /api/vms` nicht mehr funktionieren

**Dateien:** `apps/vmm-server/src/api/mod.rs`, `apps/vmm-server/src/auth/middleware.rs`

---

### 2. Migration direkt Host-zu-Host mit einmaligem Token

**Problem:** Aktuell orchestriert der Cluster die Migration komplett selbst:
Stop auf Quelle → Provision auf Ziel → Destroy auf Quelle. Disk-Dateien werden
dabei NICHT transferiert — es funktioniert nur mit Shared Storage. Bei lokalem
Storage bleiben die Disks auf dem Quell-Host und die VM auf dem Ziel hat keine.

**Aufgaben:**
- [ ] Neuen Agent-Endpunkt `POST /agent/migration/send` auf vmm-server erstellen (sendet VM-Disks an Ziel-Host)
- [ ] Neuen Agent-Endpunkt `POST /agent/migration/receive` auf vmm-server erstellen (empfängt VM-Disks von Quell-Host)
- [ ] Einmaligen Migrations-Token im Cluster generieren (UUID, 5 Minuten gültig)
- [ ] Token an beide Hosts senden: Quelle bekommt Ziel-Adresse + Token, Ziel erwartet Verbindung mit Token
- [ ] Disk-Transfer: Quelle streamt Disk-Dateien direkt an Ziel über HTTP/TCP
- [ ] Cluster überwacht Fortschritt via Task-System
- [ ] Fallback auf Shared-Storage-Modus wenn beide Hosts denselben Datastore gemountet haben (keine Disk-Kopie nötig)
- [ ] Progress-Reporting: Agent meldet Transfer-Fortschritt (Bytes gesendet) an Cluster

**Dateien:** `apps/vmm-server/src/agent/handlers.rs` (neue Endpunkte), `apps/vmm-cluster/src/services/migration.rs` (Orchestrierung), `libs/vmm-core/src/cluster.rs` (Token-Typen)

---

### 3. Input-Validierung in allen API-Handlern

**Problem:** Netzwerk-, VM- und Storage-APIs akzeptieren beliebige Werte ohne
Prüfung. Ungültige IPs, Subnets und VLAN-IDs landen in der Datenbank.

**Aufgaben:**
- [ ] Validierungsmodul `apps/vmm-cluster/src/services/validation.rs` erstellen
- [ ] CIDR-Validierung: Subnet muss Format `x.x.x.x/y` haben, y in 0-32
- [ ] IP-Validierung: Gateway, DHCP-Range-Start/End müssen gültige IPv4 sein
- [ ] IP-in-Subnet-Check: Gateway und DHCP-Range müssen im angegebenen Subnet liegen
- [ ] VLAN-Validierung: VLAN-ID muss 1-4094 sein (wenn angegeben)
- [ ] VM-Validierung: `cpu_cores > 0`, `ram_mb >= 64`, `name` nicht leer
- [ ] Datastore-Validierung: `mount_source` für NFS muss Format `host:/path` haben
- [ ] Validierung in allen API-Handlern einbauen: `api/network.rs`, `api/vms.rs`, `api/storage.rs`
- [ ] Fehlermeldungen mit konkretem Grund zurückgeben (z.B. "Gateway 10.0.0.999 is not a valid IPv4 address")

**Dateien:** Neues Modul `services/validation.rs`, alle Dateien in `apps/vmm-cluster/src/api/`

---

### 4. Host-Registrierung: Vorhandene VMs importieren

**Problem:** Wenn ein vmm-server mit laufenden VMs beim Cluster registriert wird,
importiert der Cluster die vorhandenen VMs nicht. Der Cluster denkt der Host ist
leer. DRS berechnet falsche Auslastung, HA schützt die VMs nicht.

**Aufgaben:**
- [ ] Nach erfolgreicher Registrierung `GET /agent/vms` auf dem neuen Host aufrufen
- [ ] Für jede gefundene VM: Prüfen ob sie schon in der Cluster-DB existiert (z.B. nach UUID)
- [ ] Neue VMs in `vms`-Tabelle einfügen mit `host_id`, `state`, `config_json`
- [ ] Storage-Pools des Hosts importieren: `GET /agent/storage/pools` aufrufen und als Datastores registrieren
- [ ] Event loggen: "Imported X VMs and Y storage pools from host Z"
- [ ] UI-Feedback: Registrierungs-Response soll importierte VMs zählen

**Dateien:** `apps/vmm-cluster/src/api/hosts.rs` (register-Handler), `apps/vmm-cluster/src/services/host.rs`

---

## HIGH — Funktionale Lücken

### 5. DRS-Engine: Exclusions beachten

**Problem:** Die `drs_exclusions`-Tabelle wird angelegt und die API erlaubt
CRUD, aber die DRS-Engine in `engine/drs.rs` prüft sie nie. Ausgeschlossene
VMs werden trotzdem für Migration empfohlen.

**Aufgaben:**
- [ ] In `analyze_and_recommend()`: Exclusions aus DB laden (`DrsService::list_exclusions()`)
- [ ] Beim Filtern von VM-Kandidaten: VMs ausschließen deren `id` in Exclusions ist
- [ ] Ressourcengruppen-Exclusion: VMs deren `resource_group_id` in Exclusions ist ebenfalls ausschließen
- [ ] `drs_excluded`-Flag in `vms`-Tabelle ebenfalls prüfen

**Dateien:** `apps/vmm-cluster/src/engine/drs.rs`

---

### 6. Agent: Datastore-Reporting implementieren

**Problem:** Der Agent-Handler `GET /agent/status` liefert immer ein leeres
Datastore-Array (`Vec::new()`). Der Heartbeat bekommt nie Informationen über
gemountete Storages. Kapazitäten im Cluster sind dadurch immer 0.

**Aufgaben:**
- [ ] In `agent/handlers.rs`: Gemountete Dateisysteme mit `statvfs` auslesen
- [ ] Für jeden gemounteten Datastore (aus `cluster.json` oder einer lokalen Config): Pfad, Kapazität, freien Speicher melden
- [ ] `AgentDatastoreStatus` korrekt befüllen mit `datastore_id`, `mount_path`, `mounted`, `total_bytes`, `free_bytes`
- [ ] Mount-Validierung: Prüfen ob der Pfad tatsächlich ein Mountpoint ist (`mountpoint -q`)

**Dateien:** `apps/vmm-server/src/agent/handlers.rs`

---

### 7. Agent: Per-VM CPU-Tracking und Startzeit

**Problem:** `cpu_usage_pct` ist immer `0.0` und `uptime_secs` immer `0` für
jede VM. DRS kann keine intelligenten Entscheidungen treffen.

**Aufgaben:**
- [ ] VM-Startzeit in `VmInstance` speichern (neues Feld `started_at: Option<Instant>`)
- [ ] Beim VM-Start: `started_at = Some(Instant::now())` setzen
- [ ] `uptime_secs` aus `started_at.elapsed()` berechnen
- [ ] CPU-Tracking: `/proc/stat` oder libcorevm-interne Metriken auslesen (falls verfügbar)
- [ ] Alternativ: CPU-Last-Schätzung basierend auf VM-Exit-Häufigkeit

**Dateien:** `apps/vmm-server/src/state.rs` (VmInstance), `apps/vmm-server/src/agent/handlers.rs`

---

### 8. Notification: Webhook tatsächlich senden

**Problem:** Der Webhook-Dispatch loggt nur `tracing::info!("WEBHOOK...")` aber
sendet nie einen HTTP-POST. Der Kommentar sagt "we can't do async in a sync
context" — das ist aber lösbar.

**Aufgaben:**
- [ ] Notification-Queue als `tokio::sync::mpsc` Channel erstellen
- [ ] `dispatch()` schreibt Webhook-Nachrichten in die Queue statt sie direkt zu senden
- [ ] Background-Task liest aus der Queue und sendet via `reqwest::Client::post(url).json(&payload).send()`
- [ ] Payload-Format: JSON mit `{ severity, category, message, timestamp }`
- [ ] HMAC-Signatur via `secret`-Feld aus Channel-Config (optional)
- [ ] Retry bei Fehler (max 3 Versuche mit Backoff)
- [ ] Status in `notification_log` aktualisieren (sent/failed mit Fehlerdetails)

**Dateien:** `apps/vmm-cluster/src/services/notification.rs`, `apps/vmm-cluster/src/main.rs` (Queue-Spawn)

---

### 9. Notification: E-Mail tatsächlich über SMTP senden

**Problem:** Wie Webhook — E-Mail wird nur geloggt, nie versendet. Die
SMTP-Config existiert in `cluster_settings`, wird aber nie benutzt.

**Aufgaben:**
- [ ] `lettre`-Crate als Dependency hinzufügen (Rust SMTP-Client)
- [ ] SMTP-Config aus `ClusterSettingsService::get_smtp_config()` laden
- [ ] E-Mail-Nachricht mit Betreff, Body (Plain-Text + HTML) aufbauen
- [ ] Über Notification-Queue senden (wie Webhook)
- [ ] TLS-Support (STARTTLS) basierend auf `use_tls`-Config
- [ ] Test-E-Mail-Funktion: `POST /api/notifications/channels/{id}/test` soll echte E-Mail senden

**Dateien:** `apps/vmm-cluster/Cargo.toml` (lettre), `apps/vmm-cluster/src/services/notification.rs`

---

### 10. LDAP: Echte Authentifizierung im Login-Flow

**Problem:** LDAP-Configs werden gespeichert, aber der Login-Handler
(`POST /api/auth/login`) prüft nur die lokale SQLite-User-Tabelle. LDAP wird
nie angefragt.

**Aufgaben:**
- [ ] `ldap3`-Crate als Dependency hinzufügen
- [ ] In `services/auth.rs` nach lokalem Login-Fehlschlag: LDAP-Configs laden
- [ ] Für jede aktive LDAP-Config: Bind mit Service-Account, dann User-Search mit Filter
- [ ] Bei Fund: Passwort-Bind mit User-DN + eingegebenem Passwort
- [ ] Gruppen-Lookup: Gruppen des Users abfragen
- [ ] Role-Mapping: AD-Gruppe → CoreVM-Rolle (aus `role_mapping` JSON)
- [ ] Bei erstem LDAP-Login: User automatisch in lokale DB anlegen (Sync)
- [ ] TLS/LDAPS-Support basierend auf Config

**Dateien:** `apps/vmm-cluster/Cargo.toml`, `apps/vmm-cluster/src/services/auth.rs`

---

### 11. HA-Engine: Shared-Storage-Check und Cascading-Failure-Handling

**Problem:** Die HA-Engine nimmt an, dass alle VMs auf Shared Storage liegen.
Bei lokalem Storage sind die Disks auf dem ausgefallenen Host und der HA-Restart
schlägt fehl. Außerdem: Wenn das Ziel während HA ebenfalls ausfällt, bleibt die
VM als "orphaned" liegen.

**Aufgaben:**
- [ ] Vor HA-Restart: Prüfen ob die VM-Disks auf einem Shared Datastore liegen
- [ ] Wenn lokaler Storage: VM als `orphaned` markieren und Event loggen statt fehlerhaften Restart
- [ ] Kapazitätsprüfung laufend aktualisieren: Nach jedem platzierten VM die freie Kapazität des Ziel-Hosts reduzieren
- [ ] Cascading Failure: Wenn Ziel-Host während Provisioning ausfällt, nächsten Host versuchen
- [ ] Max-Retry-Limit pro VM (z.B. 3 Versuche)
- [ ] Admission Control: Reservierte Kapazität für Failover-Hosts nicht überbuchen

**Dateien:** `apps/vmm-cluster/src/engine/ha.rs`

---

### 12. Storage-Monitoring: NFS-Mount-Validierung

**Problem:** Datastores werden nach dem Erstellen als "online" markiert, egal ob
der NFS-Mount auf den Hosts funktioniert. Der Agent meldet nie Datastore-Status
(siehe Aufgabe 6).

**Aufgaben:**
- [ ] Aufgabe 6 zuerst implementieren (Agent meldet Datastores)
- [ ] Im Agent: Bei Mount-Befehl Ergebnis prüfen und zurückmelden
- [ ] Im Agent: Periodisch gemountete Pfade validieren (`mountpoint -q /path`)
- [ ] Im Heartbeat: Datastore-Status aus Agent-Report übernehmen
- [ ] Im Cluster: Datastore-Status auf "error" setzen wenn kein Host ihn gemountet hat
- [ ] Event loggen wenn ein Datastore-Mount fehlschlägt
- [ ] UI: Fehlgeschlagene Mounts rot anzeigen mit Fehlermeldung

**Dateien:** `apps/vmm-server/src/agent/handlers.rs`, `apps/vmm-cluster/src/engine/heartbeat.rs`

---

## MEDIUM — Feature-Gaps

### 13. SDN: DHCP/DNS als echten Dienst implementieren

**Problem:** DHCP- und DNS-Konfigurationen werden in der Datenbank gespeichert,
aber es läuft kein Dienst der tatsächlich IP-Adressen vergibt oder
DNS-Anfragen beantwortet. VMs bekommen aktuell nur die SLIRP-Default-Adresse.

**Aufgaben:**
- [ ] Option A: `dnsmasq`-Integration — Config-Dateien generieren und Daemon starten/stoppen
- [ ] Option B: Rust-nativer DHCP/DNS-Daemon als Teil von vmm-cluster
- [ ] DHCP-Leases in die DB schreiben wenn sie vergeben werden
- [ ] DNS-Records aus der DB bedienen + Upstream-Forwarding
- [ ] Auto-Registrierung: Wenn VM startet und DHCP-Lease bekommt, DNS-A-Record erstellen
- [ ] Pro Netzwerk: eigener DHCP-Scope mit Range, Gateway, DNS-Server
- [ ] Integration mit SLIRP: SDN-Config an die VM's SLIRP-Backend weitergeben (bereits implementiert)

**Dateien:** Neues Modul `apps/vmm-cluster/src/engine/dhcp.rs` oder dnsmasq-Integration

---

### 14. PXE: TFTP-Server und Boot-Menü-Generierung

**Problem:** PXE-Konfiguration wird gespeichert, aber es gibt keinen TFTP-Server
und keine Boot-Menü-Generierung. ISOs können verlinkt werden, aber es passiert
nichts damit.

**Aufgaben:**
- [ ] TFTP-Server-Crate evaluieren (z.B. `tftp` oder `async-tftp`)
- [ ] Boot-Menü aus `pxe_boot_entries`-Tabelle generieren (iPXE oder PXELINUX Format)
- [ ] ISOs als Boot-Quellen verlinken: `memdisk`/`sanboot` für ISO-Boot über Netzwerk
- [ ] DHCP-Option 66 (next-server) und Option 67 (boot-file) korrekt setzen
- [ ] UEFI-Support: iPXE.efi als Default-Boot-File
- [ ] BIOS-Support: pxelinux.0 als Alternative

**Dateien:** Neues Modul, `apps/vmm-cluster/src/engine/tftp.rs`

---

### 15. Console-Bridge: Error-Recovery und Reconnect

**Problem:** Wenn der Host während einer Konsolen-Sitzung ausfällt, wird die
WebSocket-Verbindung stumm geschlossen. Der Client bekommt keine Fehlermeldung.

**Aufgaben:**
- [ ] Bei Verbindungsfehler zum Node: Error-Frame an Client senden mit Fehlermeldung
- [ ] Timeout: Wenn Node nicht innerhalb von 5 Sekunden antwortet, Client informieren
- [ ] Reconnect-Versuch: Bei temporärem Fehler automatisch erneut verbinden
- [ ] Heartbeat: Periodisch Ping/Pong zwischen Cluster und Node prüfen

**Dateien:** `apps/vmm-cluster/src/ws/console_bridge.rs`

---

## LOW — Optimierungen

### 16. State-Reconciler: Cluster-DB vs. Node-Realität abgleichen

**Problem:** Wenn ein Host nach einem Ausfall wiederkommt, wird sein Status auf
"online" gesetzt. Aber die VMs die zwischenzeitlich per HA auf andere Hosts
verschoben wurden, könnten auf dem alten Host noch laufen. Zwei Instanzen
derselben VM auf verschiedenen Hosts = Datenkorruption.

**Aufgaben:**
- [ ] Bei Host-Reconnect (Status wechselt von offline → online): Reconciliation starten
- [ ] VMs auf dem Host abfragen (`GET /agent/vms`)
- [ ] Vergleichen mit Cluster-DB: Welche VMs gehören laut DB zu diesem Host?
- [ ] VMs die laut DB auf einem ANDEREN Host laufen: Auf dem alten Host stoppen (`POST /agent/vms/{id}/force-stop`)
- [ ] VMs die in der DB als "orphaned" markiert sind: Dem wiedergekehrten Host zuweisen
- [ ] Event loggen: "Reconciled X VMs on host Y after reconnect"

**Dateien:** Neues Modul `apps/vmm-cluster/src/engine/reconciler.rs`, `apps/vmm-cluster/src/engine/heartbeat.rs`
