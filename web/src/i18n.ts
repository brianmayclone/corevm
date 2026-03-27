export type Lang = 'en' | 'de';

const translations = {
  en: {
    // Nav
    nav_appliance: 'Appliance',
    nav_features: 'Features',
    nav_screenshots: 'Screenshots',
    nav_cluster: 'Cluster & SDN',
    nav_architecture: 'Architecture',
    nav_get_started: 'Download ISO',

    // Hero
    hero_badge: 'The Open Hypervisor',
    hero_title_1: 'Your Infrastructure.',
    hero_title_2: 'Your Rules.',
    hero_subtitle: 'CoreVM is a turnkey bare-metal hypervisor — boot the ISO, run the installer, manage your VMs. KVM acceleration, enterprise clustering, and a modern web UI. No license fees. No vendor lock-in.',
    hero_cta_primary: 'Download ISO',
    hero_cta_secondary: 'View on GitHub',
    hero_compare: 'Open-source bare-metal hypervisor',

    // Stats
    stat_devices: 'Emulated Devices',
    stat_loc: 'Lines of Code',
    stat_api: 'REST API Endpoints',
    stat_boot: 'Boot to VMs',

    // Appliance
    appliance_badge: 'Turnkey Appliance',
    appliance_title: 'One ISO. Complete hypervisor.',
    appliance_subtitle: 'Download the ISO, boot it on any x86 server, and follow the guided installer. In minutes you have a production-ready hypervisor — standalone or as part of a cluster.',
    appliance_step1_title: 'Boot the ISO',
    appliance_step1_desc: 'Write to USB or mount via IPMI. UEFI and Legacy BIOS supported. Boots into the guided installer automatically.',
    appliance_step2_title: 'Install in minutes',
    appliance_step2_desc: 'Choose standalone or cluster mode. Configure network, storage, and credentials. Automatic disk partitioning with isolated VM storage.',
    appliance_step3_title: 'Manage via Web UI',
    appliance_step3_desc: 'Access the modern dashboard from any browser. Create VMs, manage storage, monitor performance — all from one interface.',
    appliance_includes: 'What\'s included',
    appliance_kernel: 'Optimized Linux kernel',
    appliance_installer: 'Guided TUI installer',
    appliance_dcui: 'Direct Console (DCUI)',
    appliance_firewall: 'nftables firewall',
    appliance_tls: 'Auto TLS certificates',
    appliance_updates: 'Offline update system',
    appliance_standalone: 'Standalone',
    appliance_standalone_desc: 'Single-node hypervisor with full web UI, REST API, and local VM management. Perfect for labs, edge deployments, or dedicated workloads.',
    appliance_cluster_mode: 'Cluster Controller',
    appliance_cluster_desc: 'Central authority managing multiple nodes with DRS, high availability, live migration, and software-defined networking. Enterprise-grade orchestration.',

    // Features
    features_badge: 'Platform',
    features_title: 'Enterprise virtualization, reimagined',
    features_subtitle: 'Everything you expect from a professional hypervisor — built from the ground up, with no legacy baggage.',

    feat_hw_title: 'KVM Hardware Acceleration',
    feat_hw_desc: 'Direct KVM integration for near-native VM performance. No emulation overhead for CPU-intensive workloads.',
    feat_web_title: 'Modern Web Management',
    feat_web_desc: 'React dashboard with real-time metrics, live VGA console via WebSocket, storage management, and full VM lifecycle control.',
    feat_api_title: '40+ REST API Endpoints',
    feat_api_desc: 'Complete automation via REST API. JWT authentication, role-based access control, audit logging, and WebSocket streaming.',
    feat_devices_title: '25+ Emulated Devices',
    feat_devices_desc: 'AHCI/SATA, Intel E1000, VMware SVGA II, AC\'97 audio, APIC, HPET, PS/2, UART, PCI bus, Q35 chipset, and more.',
    feat_dcui_title: 'Direct Console (DCUI)',
    feat_dcui_desc: 'Dedicated server console with network config, service management, diagnostics, log viewer, and factory reset — right on the server.',
    feat_security_title: 'Secure by Default',
    feat_security_desc: 'TLS everywhere, nftables firewall, no root SSH, memory-safe core. Hardened from day one.',

    // Screenshots
    screenshots_badge: 'Interface',
    screenshots_title: 'Designed for operators',
    screenshots_subtitle: 'A clean, responsive web interface that gives you full control over your virtual infrastructure — from desktop or mobile.',
    screenshot_dashboard: 'Dashboard',
    screenshot_vms: 'Virtual Machines',
    screenshot_settings: 'VM Settings',
    screenshot_storage: 'Storage',
    screenshot_network: 'Network',
    screenshot_mobile: 'Mobile',

    // Cluster & SDN
    cluster_badge: 'Cluster & SDN',
    cluster_title: 'Scale from one node to many',
    cluster_subtitle: 'Transform standalone hosts into a unified, resilient virtualization platform with enterprise-grade orchestration and software-defined networking.',

    cluster_drs: 'DRS — Distributed Resource Scheduler',
    cluster_drs_desc: 'Automatic workload balancing across your cluster. VMs are placed and migrated based on real-time CPU, memory, and I/O metrics to prevent hotspots.',
    cluster_ha: 'High Availability',
    cluster_ha_desc: 'When a host fails, affected VMs automatically restart on healthy nodes within seconds. Configurable policies per VM.',
    cluster_migration: 'Live Migration',
    cluster_migration_desc: 'Move running VMs between hosts with zero downtime. Direct host-to-host transfer with one-time security tokens.',
    cluster_sdn: 'Software-Defined Networking',
    cluster_sdn_desc: 'Create virtual networks that span your entire cluster. Integrated DHCP server, DNS, and PXE boot services. Full network isolation between tenants.',
    cluster_storage: 'Shared Storage',
    cluster_storage_desc: 'Storage wizard for NFS, GlusterFS, and CephFS. Shared datastores accessible from all cluster nodes for live migration.',
    cluster_ldap: 'LDAP / Active Directory',
    cluster_ldap_desc: 'Centralized authentication with your existing directory. Role-based access control maps to LDAP groups.',
    cluster_maintenance: 'Maintenance Mode',
    cluster_maintenance_desc: 'Drain a host before maintenance. VMs are automatically migrated to other nodes. No downtime for your workloads.',
    cluster_notifications: 'Alerting & Notifications',
    cluster_notifications_desc: 'Email, webhook, and log-based alerts for cluster events, failovers, resource thresholds, and VM state changes.',

    // Architecture
    arch_badge: 'Under the Hood',
    arch_title: 'From bare metal up.',
    arch_subtitle: 'CoreVM implements a complete x86 PC — from custom BIOS firmware through PCI bus to high-level device emulation.',
    arch_cpu_title: 'Full x86 ISA',
    arch_cpu_desc: '16-bit real mode, 32-bit protected mode, 64-bit long mode. Complete instruction set with hardware-accelerated execution via KVM.',
    arch_memory_title: 'Advanced Memory Subsystem',
    arch_memory_desc: 'Flat, MMIO, segment, and paging modes. Efficient guest-physical to host-virtual address translation.',
    arch_bios_title: 'Custom NASM BIOS',
    arch_bios_desc: '64 KB custom BIOS in NASM assembly with full INT services. SeaBIOS compatible for maximum guest OS support.',
    arch_ffi_title: 'C FFI — Embeddable',
    arch_ffi_desc: '58 C-compatible exports. Embed the CoreVM engine in any application via libcorevm dynamic linking.',

    // VMManager
    vmm_badge: 'For Everyone',
    vmm_title: 'Desktop Virtualization for Everyone!',
    vmm_subtitle: 'The CoreVM VMManager brings professional virtualization to your desktop. Run virtual machines on your own PC — intuitive, powerful, and free.',
    vmm_feat1_title: 'Native Desktop App',
    vmm_feat1_desc: 'A modern desktop application for Windows and Linux. Manage your VMs with an intuitive graphical interface — no command line required.',
    vmm_feat2_title: 'For Everyone',
    vmm_feat2_desc: 'Whether developer, student, or tech enthusiast — VMManager makes creating and running virtual machines as easy as installing an app.',
    vmm_cta: 'Download VMManager',
    vmm_platforms: 'Available for Windows and Linux',

    // CTA
    cta_title: 'Ready to replace your hypervisor?',
    cta_subtitle: 'Download the ISO, boot it, and have a production-ready hypervisor in minutes. Or build from source if you prefer.',
    cta_download: 'Download ISO',
    cta_build: 'Build from Source',
    cta_docs: 'Read the Docs',

    // Coming Soon Modal
    coming_soon_title: 'Coming Soon',
    coming_soon_desc: 'The CoreVM ISO download is not yet available. We are working hard to deliver the first public release.',
    coming_soon_hint: 'In the meantime, you can build from source on GitHub.',
    coming_soon_close: 'Got it',

    // Legal
    legal_imprint: 'Imprint',
    legal_privacy: 'Privacy Policy',
    legal_responsible: 'Responsible persons',
    legal_country_ch: 'Switzerland',
    legal_country_de: 'Germany',
    legal_contact: 'Contact',
    legal_disclaimer_title: 'Disclaimer',
    legal_disclaimer_text: 'The content of this website has been prepared with the greatest possible care. However, no guarantee is given for the accuracy, completeness, or timeliness of the content provided.',

    // Privacy
    privacy_responsible_title: 'Responsible party',
    privacy_responsible_text: 'The responsible parties for data processing on this website are Mike Strathmann and Christian Möller (see Imprint). Contact: info@corevm.io',
    privacy_hosting_title: 'Hosting & server logs',
    privacy_hosting_text: 'This website is hosted by a third-party provider. The hosting provider may collect and store server log files (IP address, date/time, pages requested, browser type) as technically necessary. This data is not merged with other data sources. The legal basis is Art. 6(1)(f) GDPR (legitimate interest in secure and efficient operation).',
    privacy_cookies_title: 'Cookies & tracking',
    privacy_cookies_text: 'This website does not use cookies, analytics, or tracking tools of any kind. No personal data is collected beyond what is described above.',
    privacy_localstorage_title: 'Local storage',
    privacy_localstorage_text: 'This website stores your language preference (DE/EN) in your browser\'s local storage. This data never leaves your device and is not transmitted to any server.',
    privacy_rights_title: 'Your rights',
    privacy_rights_text: 'You have the right to request information about your stored data, as well as the right to correction, deletion, or restriction of processing. Contact: info@corevm.io',

    // Footer
    footer_desc: 'The open bare-metal hypervisor. Powered by KVM, with enterprise clustering and a modern web UI.',
    footer_product: 'Product',
    footer_resources: 'Resources',
    footer_documentation: 'Documentation',
    footer_api_reference: 'API Reference',
    footer_github: 'GitHub',
    footer_rights: 'All rights reserved.',
  },
  de: {
    // Nav
    nav_appliance: 'Appliance',
    nav_features: 'Features',
    nav_screenshots: 'Screenshots',
    nav_cluster: 'Cluster & SDN',
    nav_architecture: 'Architektur',
    nav_get_started: 'ISO herunterladen',

    // Hero
    hero_badge: 'Der offene Hypervisor',
    hero_title_1: 'Ihre Infrastruktur.',
    hero_title_2: 'Ihre Regeln.',
    hero_subtitle: 'CoreVM ist ein schlüsselfertiger Bare-Metal-Hypervisor — ISO booten, Installer durchlaufen, VMs verwalten. KVM-Beschleunigung, Enterprise-Clustering und moderne Web-UI. Keine Lizenzkosten. Kein Vendor-Lock-in.',
    hero_cta_primary: 'ISO herunterladen',
    hero_cta_secondary: 'Auf GitHub ansehen',
    hero_compare: 'Open-Source Bare-Metal-Hypervisor',

    // Stats
    stat_devices: 'Emulierte Geräte',
    stat_loc: 'Zeilen Code',
    stat_api: 'REST-API-Endpunkte',
    stat_boot: 'Boot bis zu VMs',

    // Appliance
    appliance_badge: 'Schlüsselfertige Appliance',
    appliance_title: 'Ein ISO. Kompletter Hypervisor.',
    appliance_subtitle: 'ISO herunterladen, auf einem beliebigen x86-Server booten und dem geführten Installer folgen. In wenigen Minuten haben Sie einen produktionsbereiten Hypervisor — standalone oder als Teil eines Clusters.',
    appliance_step1_title: 'ISO booten',
    appliance_step1_desc: 'Auf USB schreiben oder via IPMI mounten. UEFI und Legacy-BIOS werden unterstützt. Bootet automatisch in den geführten Installer.',
    appliance_step2_title: 'In Minuten installiert',
    appliance_step2_desc: 'Standalone oder Cluster-Modus wählen. Netzwerk, Speicher und Zugangsdaten konfigurieren. Automatische Disk-Partitionierung mit isoliertem VM-Speicher.',
    appliance_step3_title: 'Per Web-UI verwalten',
    appliance_step3_desc: 'Modernes Dashboard von jedem Browser aus nutzen. VMs erstellen, Speicher verwalten, Performance überwachen — alles in einer Oberfläche.',
    appliance_includes: 'Was enthalten ist',
    appliance_kernel: 'Optimierter Linux-Kernel',
    appliance_installer: 'Geführter TUI-Installer',
    appliance_dcui: 'Direkte Konsole (DCUI)',
    appliance_firewall: 'nftables-Firewall',
    appliance_tls: 'Auto-TLS-Zertifikate',
    appliance_updates: 'Offline-Update-System',
    appliance_standalone: 'Standalone',
    appliance_standalone_desc: 'Einzelner Hypervisor-Node mit vollständiger Web-UI, REST-API und lokaler VM-Verwaltung. Ideal für Labs, Edge-Deployments oder dedizierte Workloads.',
    appliance_cluster_mode: 'Cluster Controller',
    appliance_cluster_desc: 'Zentrale Autorität zur Verwaltung mehrerer Nodes mit DRS, Hochverfügbarkeit, Live-Migration und Software-Defined Networking. Enterprise-Grade-Orchestrierung.',

    // Features
    features_badge: 'Plattform',
    features_title: 'Enterprise-Virtualisierung, neu gedacht',
    features_subtitle: 'Alles, was Sie von einem professionellen Hypervisor erwarten — von Grund auf gebaut, ohne Legacy-Ballast.',

    feat_hw_title: 'KVM-Hardware-Beschleunigung',
    feat_hw_desc: 'Direkte KVM-Integration für nahezu native VM-Performance. Kein Emulations-Overhead für CPU-intensive Workloads.',
    feat_web_title: 'Moderne Web-Verwaltung',
    feat_web_desc: 'React-Dashboard mit Echtzeit-Metriken, Live-VGA-Konsole via WebSocket, Speicherverwaltung und vollständiger VM-Lifecycle-Kontrolle.',
    feat_api_title: '40+ REST-API-Endpunkte',
    feat_api_desc: 'Vollständige Automatisierung via REST-API. JWT-Authentifizierung, rollenbasierte Zugriffskontrolle, Audit-Logging und WebSocket-Streaming.',
    feat_devices_title: '25+ Emulierte Geräte',
    feat_devices_desc: 'AHCI/SATA, Intel E1000, VMware SVGA II, AC\'97 Audio, APIC, HPET, PS/2, UART, PCI-Bus, Q35-Chipsatz und mehr.',
    feat_dcui_title: 'Direkte Konsole (DCUI)',
    feat_dcui_desc: 'Dedizierte Server-Konsole mit Netzwerkkonfiguration, Service-Management, Diagnose, Log-Viewer und Factory-Reset — direkt am Server.',
    feat_security_title: 'Sicher ab Werk',
    feat_security_desc: 'TLS überall, nftables-Firewall, kein Root-SSH, speichersicherer Kern. Von Anfang an gehärtet.',

    // Screenshots
    screenshots_badge: 'Oberfläche',
    screenshots_title: 'Für Administratoren entwickelt',
    screenshots_subtitle: 'Eine saubere, responsive Weboberfläche, die Ihnen volle Kontrolle über Ihre virtuelle Infrastruktur gibt — am Desktop oder mobil.',
    screenshot_dashboard: 'Dashboard',
    screenshot_vms: 'Virtuelle Maschinen',
    screenshot_settings: 'VM-Einstellungen',
    screenshot_storage: 'Speicher',
    screenshot_network: 'Netzwerk',
    screenshot_mobile: 'Mobil',

    // Cluster & SDN
    cluster_badge: 'Cluster & SDN',
    cluster_title: 'Von einem Node auf viele skalieren',
    cluster_subtitle: 'Verwandeln Sie einzelne Hosts in eine einheitliche, resiliente Virtualisierungsplattform mit Enterprise-Grade-Orchestrierung und Software-Defined Networking.',

    cluster_drs: 'DRS — Distributed Resource Scheduler',
    cluster_drs_desc: 'Automatische Lastverteilung im gesamten Cluster. VMs werden basierend auf Echtzeit-CPU-, Speicher- und I/O-Metriken platziert und migriert.',
    cluster_ha: 'Hochverfügbarkeit',
    cluster_ha_desc: 'Bei Host-Ausfall starten betroffene VMs automatisch auf gesunden Nodes innerhalb von Sekunden neu. Konfigurierbare Richtlinien pro VM.',
    cluster_migration: 'Live-Migration',
    cluster_migration_desc: 'Laufende VMs ohne Downtime zwischen Hosts verschieben. Direkter Host-zu-Host-Transfer mit einmaligen Sicherheits-Tokens.',
    cluster_sdn: 'Software-Defined Networking',
    cluster_sdn_desc: 'Erstellen Sie virtuelle Netzwerke, die Ihren gesamten Cluster überspannen. Integrierter DHCP-Server, DNS und PXE-Boot-Services. Vollständige Netzwerkisolierung zwischen Mandanten.',
    cluster_storage: 'Shared Storage',
    cluster_storage_desc: 'Storage-Wizard für NFS, GlusterFS und CephFS. Gemeinsame Datenspeicher, die von allen Cluster-Nodes für Live-Migration erreichbar sind.',
    cluster_ldap: 'LDAP / Active Directory',
    cluster_ldap_desc: 'Zentralisierte Authentifizierung mit Ihrem bestehenden Verzeichnisdienst. Rollenbasierte Zugriffskontrolle wird auf LDAP-Gruppen gemappt.',
    cluster_maintenance: 'Wartungsmodus',
    cluster_maintenance_desc: 'Host vor der Wartung entleeren. VMs werden automatisch auf andere Nodes migriert. Kein Downtime für Ihre Workloads.',
    cluster_notifications: 'Alerting & Benachrichtigungen',
    cluster_notifications_desc: 'E-Mail-, Webhook- und Log-basierte Alerts für Cluster-Events, Failovers, Ressourcen-Schwellwerte und VM-Statusänderungen.',

    // Architecture
    arch_badge: 'Unter der Haube',
    arch_title: 'Von Bare Metal an.',
    arch_subtitle: 'CoreVM implementiert einen kompletten x86-PC — vom eigenen BIOS-Firmware über den PCI-Bus bis zur High-Level-Geräteemulation.',
    arch_cpu_title: 'Vollständige x86 ISA',
    arch_cpu_desc: '16-Bit Real Mode, 32-Bit Protected Mode, 64-Bit Long Mode. Kompletter Befehlssatz mit hardware-beschleunigter Ausführung über KVM.',
    arch_memory_title: 'Erweitertes Speicher-Subsystem',
    arch_memory_desc: 'Flat-, MMIO-, Segment- und Paging-Modi. Effiziente Guest-Physical zu Host-Virtual Adressübersetzung.',
    arch_bios_title: 'Eigenes NASM-BIOS',
    arch_bios_desc: '64 KB Custom-BIOS in NASM Assembly mit vollständigen INT-Services. SeaBIOS-kompatibel für maximale Gast-OS-Unterstützung.',
    arch_ffi_title: 'C-FFI — Einbettbar',
    arch_ffi_desc: '58 C-kompatible Exports. CoreVM-Engine über libcorevm dynamisch in jede Anwendung einbetten.',

    // VMManager
    vmm_badge: 'Für Jedermann',
    vmm_title: 'Desktopvirtualisierung für Jedermann!',
    vmm_subtitle: 'Der CoreVM VMManager bringt professionelle Virtualisierung auf Ihren Desktop. Virtuelle Maschinen auf dem eigenen PC betreiben — intuitiv, leistungsstark und kostenlos.',
    vmm_feat1_title: 'Native Desktop-App',
    vmm_feat1_desc: 'Eine moderne Desktop-Anwendung für Windows und Linux. Verwalten Sie Ihre VMs mit einer intuitiven grafischen Oberfläche — keine Kommandozeile nötig.',
    vmm_feat2_title: 'Für Jedermann',
    vmm_feat2_desc: 'Ob Entwickler, Student oder Technik-Enthusiast — VMManager macht das Erstellen und Betreiben virtueller Maschinen so einfach wie eine App-Installation.',
    vmm_cta: 'VMManager herunterladen',
    vmm_platforms: 'Verfügbar für Windows und Linux',

    // CTA
    cta_title: 'Bereit, Ihren Hypervisor zu ersetzen?',
    cta_subtitle: 'ISO herunterladen, booten und in Minuten einen produktionsbereiten Hypervisor haben. Oder aus dem Quellcode bauen, wenn Sie das bevorzugen.',
    cta_download: 'ISO herunterladen',
    cta_build: 'Aus Quellcode bauen',
    cta_docs: 'Dokumentation lesen',

    // Coming Soon Modal
    coming_soon_title: 'Demnächst verfügbar',
    coming_soon_desc: 'Der CoreVM ISO-Download ist noch nicht verfügbar. Wir arbeiten intensiv am ersten öffentlichen Release.',
    coming_soon_hint: 'In der Zwischenzeit können Sie den Quellcode auf GitHub bauen.',
    coming_soon_close: 'Verstanden',

    // Legal
    legal_imprint: 'Impressum',
    legal_privacy: 'Datenschutzerklärung',
    legal_responsible: 'Verantwortliche Personen',
    legal_country_ch: 'Schweiz',
    legal_country_de: 'Deutschland',
    legal_contact: 'Kontakt',
    legal_disclaimer_title: 'Haftungsausschluss',
    legal_disclaimer_text: 'Die Inhalte dieser Website wurden mit grösstmöglicher Sorgfalt erstellt. Für die Richtigkeit, Vollständigkeit und Aktualität der bereitgestellten Inhalte wird jedoch keine Gewähr übernommen.',

    // Datenschutz
    privacy_responsible_title: 'Verantwortliche Stelle',
    privacy_responsible_text: 'Verantwortlich für die Datenverarbeitung auf dieser Website sind Mike Strathmann und Christian Möller (siehe Impressum). Kontakt: info@corevm.io',
    privacy_hosting_title: 'Hosting & Server-Logfiles',
    privacy_hosting_text: 'Diese Website wird bei einem externen Anbieter gehostet. Der Hosting-Anbieter kann Server-Logfiles (IP-Adresse, Datum/Uhrzeit, aufgerufene Seiten, Browsertyp) als technisch notwendig erheben und speichern. Diese Daten werden nicht mit anderen Datenquellen zusammengeführt. Rechtsgrundlage ist Art. 6 Abs. 1 lit. f DSGVO (berechtigtes Interesse am sicheren und effizienten Betrieb).',
    privacy_cookies_title: 'Cookies & Tracking',
    privacy_cookies_text: 'Diese Website verwendet keine Cookies, Analyse- oder Tracking-Tools jeglicher Art. Über die oben beschriebenen Daten hinaus werden keine personenbezogenen Daten erhoben.',
    privacy_localstorage_title: 'Lokaler Speicher',
    privacy_localstorage_text: 'Diese Website speichert Ihre Spracheinstellung (DE/EN) im lokalen Speicher Ihres Browsers. Diese Daten verlassen Ihr Gerät nicht und werden an keinen Server übermittelt.',
    privacy_rights_title: 'Ihre Rechte',
    privacy_rights_text: 'Sie haben das Recht auf Auskunft über Ihre gespeicherten Daten sowie das Recht auf Berichtigung, Löschung oder Einschränkung der Verarbeitung. Kontakt: info@corevm.io',

    // Footer
    footer_desc: 'Der offene Bare-Metal-Hypervisor. Von KVM angetrieben, mit Enterprise-Clustering und moderner Web-UI.',
    footer_product: 'Produkt',
    footer_resources: 'Ressourcen',
    footer_documentation: 'Dokumentation',
    footer_api_reference: 'API-Referenz',
    footer_github: 'GitHub',
    footer_rights: 'Alle Rechte vorbehalten.',
  },
} as const;

export type TranslationKey = keyof typeof translations.en;

export function t(lang: Lang, key: TranslationKey): string {
  return translations[lang][key] ?? translations.en[key] ?? key;
}

export function getInitialLang(): Lang {
  const stored = localStorage.getItem('corevm-lang');
  if (stored === 'de' || stored === 'en') return stored;
  return navigator.language.startsWith('de') ? 'de' : 'en';
}

export function persistLang(lang: Lang) {
  localStorage.setItem('corevm-lang', lang);
}
