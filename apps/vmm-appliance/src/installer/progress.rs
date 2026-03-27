use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Wrap};
use ratatui::Frame;

use crate::common::{
    certs::{generate_self_signed, import_certificates},
    config::{ApplianceConfig, ApplianceRole, write_vmm_cluster_config, write_vmm_server_config, write_vmm_server_config_full},
    firewall::{write_nftables_config, FirewallConfig},
    network::write_networkd_config,
    system::{
        configure_chrony, configure_fstab, create_user, enable_service, extract_rootfs,
        format_partitions, install_grub, is_efi_booted, mount_target, partition_disk, reboot,
        set_hostname, set_locale, set_root_password, set_timezone, unmount_target,
    },
};

use super::{InstallConfig, ScreenResult};

// ---------------------------------------------------------------------------
// Info slides shown during installation
// ---------------------------------------------------------------------------

const INFO_SLIDES: &[(&str, &str)] = &[
    (
        "Welcome to CoreVM",
        "CoreVM is a lightweight virtualization platform\n\
         built for performance, simplicity, and security.\n\
         \n\
         Your appliance is being configured now.",
    ),
    (
        "Hardware Virtualization",
        "CoreVM leverages KVM for near-native performance.\n\
         Virtual machines run directly on the CPU with\n\
         minimal overhead, giving you the best possible\n\
         speed for your workloads.",
    ),
    (
        "Web Management",
        "Once installation completes, you can manage your\n\
         appliance through the built-in web interface.\n\
         Create and manage virtual machines, configure\n\
         networking, and monitor system health.",
    ),
    (
        "Storage Options",
        "CoreVM supports local storage, NFS shared storage,\n\
         GlusterFS distributed volumes, and Ceph for\n\
         highly available block storage. Scale your\n\
         infrastructure as your needs grow.",
    ),
    (
        "Clustering",
        "Deploy multiple CoreVM nodes as a cluster for\n\
         high availability and live migration. The cluster\n\
         controller coordinates workloads automatically\n\
         across all nodes.",
    ),
    (
        "Security First",
        "CoreVM comes hardened out of the box with nftables\n\
         firewall, SSH key authentication, TLS encryption,\n\
         and minimal attack surface. Your infrastructure\n\
         stays protected by default.",
    ),
    (
        "Direct Console UI",
        "After installation, the DCUI on tty1 provides\n\
         direct access to appliance management without\n\
         needing a browser. Configure network, passwords,\n\
         certificates, and more from the console.",
    ),
    (
        "Offline Updates",
        "Keep your appliance up to date with offline update\n\
         packages. No internet connection required for\n\
         updates — just upload the package through the\n\
         DCUI or web interface.",
    ),
];

const SLIDE_INTERVAL_SECS: u64 = 8;

// ---------------------------------------------------------------------------
// ProgressMsg
// ---------------------------------------------------------------------------

enum ProgressMsg {
    Update(f64, String),
    Done(String),
    Error(String),
}

// ---------------------------------------------------------------------------
// ProgressState
// ---------------------------------------------------------------------------

pub struct ProgressState {
    progress: f64,
    status_text: String,
    rx: Option<mpsc::Receiver<ProgressMsg>>,
    started: bool,
    start_time: Instant,
    done: bool,
    done_url: String,
    error: Option<String>,
}

impl ProgressState {
    pub fn new() -> Self {
        Self {
            progress: 0.0,
            status_text: String::new(),
            rx: None,
            started: false,
            start_time: Instant::now(),
            done: false,
            done_url: String::new(),
            error: None,
        }
    }

    fn current_slide(&self) -> usize {
        let elapsed = self.start_time.elapsed().as_secs();
        ((elapsed / SLIDE_INTERVAL_SECS) as usize) % INFO_SLIDES.len()
    }

    fn start(&mut self, config: &InstallConfig) {
        self.started = true;
        self.start_time = Instant::now();

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        let disk = config.disk.clone().unwrap_or_default();
        let network = config.network.clone();
        let timezone = config.timezone.clone();
        let ntp_enabled = config.ntp_enabled;
        let ntp_server = config.ntp_server.clone();
        let root_password = config.root_password.clone();
        let username = config.username.clone();
        let user_password = config.user_password.clone();
        let server_port = config.server_port;
        let cluster_port = config.cluster_port;
        let cli_access_enabled = config.cli_access_enabled;
        let self_signed_cert = config.self_signed_cert;
        let cert_path = config.cert_path.clone();
        let key_path = config.key_path.clone();
        let language = config.language.clone();
        let role = config.role.clone().unwrap_or(ApplianceRole::Server);

        thread::spawn(move || {
            let send = |msg: ProgressMsg| {
                let _ = tx.send(msg);
            };

            let target = Path::new("/mnt/target");
            let efi = is_efi_booted();

            // 1. Partition disk
            send(ProgressMsg::Update(0.05, "Partitioning disk...".into()));
            if let Err(e) = partition_disk(&disk, efi) {
                send(ProgressMsg::Error(format!("Partition failed: {}", e)));
                return;
            }

            // 2. Format partitions
            send(ProgressMsg::Update(0.10, "Formatting partitions...".into()));
            if let Err(e) = format_partitions(&disk, efi) {
                send(ProgressMsg::Error(format!("Format failed: {}", e)));
                return;
            }

            // 3. Mount target
            send(ProgressMsg::Update(0.15, "Mounting target filesystem...".into()));
            if let Err(e) = mount_target(&disk, target, efi) {
                send(ProgressMsg::Error(format!("Mount failed: {}", e)));
                return;
            }

            // 4. Extract rootfs
            send(ProgressMsg::Update(0.20, "Extracting system files...".into()));
            if let Err(e) = extract_rootfs(Path::new("/opt/vmm/rootfs.tar.gz"), target) {
                send(ProgressMsg::Error(format!("Rootfs extraction failed: {}", e)));
                return;
            }

            // 5. Configure system
            send(ProgressMsg::Update(0.50, "Configuring system...".into()));
            let hostname = if let Some(net) = &network {
                net.hostname.clone()
            } else {
                "corevm".to_string()
            };
            if let Err(e) = set_hostname(target, &hostname) {
                send(ProgressMsg::Error(format!("set_hostname failed: {}", e)));
                return;
            }
            let locale = if language.is_empty() { "en_US.UTF-8".to_string() } else { language };
            if let Err(e) = set_locale(target, &locale) {
                send(ProgressMsg::Error(format!("set_locale failed: {}", e)));
                return;
            }
            let tz = if timezone.is_empty() { "UTC".to_string() } else { timezone };
            if let Err(e) = set_timezone(target, &tz) {
                send(ProgressMsg::Error(format!("set_timezone failed: {}", e)));
                return;
            }
            if let Err(e) = configure_fstab(target, &disk, efi) {
                send(ProgressMsg::Error(format!("configure_fstab failed: {}", e)));
                return;
            }

            // 6. Configure network
            send(ProgressMsg::Update(0.55, "Configuring network...".into()));
            if let Some(net_config) = &network {
                if let Err(e) = write_networkd_config(target, net_config) {
                    send(ProgressMsg::Error(format!("write_networkd_config failed: {}", e)));
                    return;
                }
            }

            // 7. Configure NTP
            send(ProgressMsg::Update(0.60, "Configuring time synchronization...".into()));
            let ntp_srv = if ntp_server.is_empty() { "pool.ntp.org".to_string() } else { ntp_server };
            if let Err(e) = configure_chrony(target, ntp_enabled, &ntp_srv) {
                send(ProgressMsg::Error(format!("configure_chrony failed: {}", e)));
                return;
            }

            // 8. Create users
            send(ProgressMsg::Update(0.65, "Creating user accounts...".into()));
            if let Err(e) = set_root_password(target, &root_password) {
                send(ProgressMsg::Error(format!("set_root_password failed: {}", e)));
                return;
            }
            if !username.is_empty() {
                if let Err(e) = create_user(target, &username, &user_password, true) {
                    send(ProgressMsg::Error(format!("create_user failed: {}", e)));
                    return;
                }
            }

            // 9. Certificates
            send(ProgressMsg::Update(0.70, "Generating TLS certificates...".into()));
            if self_signed_cert {
                if let Err(e) = generate_self_signed(target, &hostname) {
                    send(ProgressMsg::Error(format!("generate_self_signed failed: {}", e)));
                    return;
                }
            } else if let (Some(cp), Some(kp)) = (&cert_path, &key_path) {
                if let Err(e) = import_certificates(target, cp, kp) {
                    send(ProgressMsg::Error(format!("import_certificates failed: {}", e)));
                    return;
                }
            }

            // 10. Service config
            send(ProgressMsg::Update(0.75, "Writing service configuration...".into()));
            let data_dir = "/var/lib/vmm";

            // vmm-server config is always written (every node runs vmm-server)
            let server_log = "/var/log/vmm/vmm-server.log";
            if let Err(e) = write_vmm_server_config(target, server_port, data_dir, server_log) {
                send(ProgressMsg::Error(format!("write_vmm_server_config failed: {}", e)));
                return;
            }

            let web_url = match &role {
                ApplianceRole::Server => {
                    let log_file = "/var/log/vmm/vmm-server.log";
                    if let Err(e) = write_vmm_server_config_full(target, server_port, data_dir, log_file, cli_access_enabled) {
                        send(ProgressMsg::Error(format!("write_vmm_server_config failed: {}", e)));
                        return;
                    }
                    format!("https://{}:{}", hostname, server_port)
                }
                ApplianceRole::Cluster => {
                    let cluster_log = "/var/log/vmm/vmm-cluster.log";
                    if let Err(e) = write_vmm_cluster_config(target, cluster_port, data_dir, cluster_log) {
                        send(ProgressMsg::Error(format!("write_vmm_cluster_config failed: {}", e)));
                        return;
                    }
                    format!("https://{}:{}", hostname, cluster_port)
                }
            };
            let appliance_config = ApplianceConfig {
                role: role.clone(),
                language: locale.clone(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            };
            let config_path = target.join("etc/vmm/appliance.toml");
            if let Err(e) = appliance_config.save(&config_path) {
                send(ProgressMsg::Error(format!("ApplianceConfig save failed: {}", e)));
                return;
            }

            // 10b. Enable services
            // vmm-server and vmm-san always run (every node is a hypervisor + storage node)
            for svc in &["vmm-server.service", "vmm-san.service"] {
                if let Err(e) = enable_service(target, svc) {
                    send(ProgressMsg::Error(format!("enable_service({}) failed: {}", svc, e)));
                    return;
                }
            }
            // Cluster-Management role additionally needs vmm-cluster
            if matches!(role, ApplianceRole::Cluster) {
                if let Err(e) = enable_service(target, "vmm-cluster.service") {
                    send(ProgressMsg::Error(format!("enable_service(vmm-cluster) failed: {}", e)));
                    return;
                }
            }

            // 11. Firewall
            send(ProgressMsg::Update(0.80, "Configuring firewall...".into()));
            let fw_config = FirewallConfig {
                ssh_port: 22,
                vmm_server_port: Some(server_port),
                vmm_cluster_port: if matches!(role, ApplianceRole::Cluster) { Some(cluster_port) } else { None },
                vmm_san_port: Some(7443),
                vmm_san_peer_port: Some(7444),
                discovery_port: Some(7445),
            };
            if let Err(e) = write_nftables_config(target, &fw_config) {
                send(ProgressMsg::Error(format!("write_nftables_config failed: {}", e)));
                return;
            }

            // 12. SSH hardening
            send(ProgressMsg::Update(0.85, "Hardening SSH configuration...".into()));
            let sshd_drop_dir = target.join("etc/ssh/sshd_config.d");
            if let Err(e) = fs::create_dir_all(&sshd_drop_dir) {
                send(ProgressMsg::Error(format!("Failed to create sshd_config.d: {}", e)));
                return;
            }
            let sshd_conf = "PermitRootLogin no\n";
            if let Err(e) = fs::write(sshd_drop_dir.join("10-corevm.conf"), sshd_conf) {
                send(ProgressMsg::Error(format!("Failed to write sshd config: {}", e)));
                return;
            }

            // 13. Install GRUB
            send(ProgressMsg::Update(0.90, "Installing bootloader...".into()));
            if let Err(e) = install_grub(target, &disk, efi) {
                send(ProgressMsg::Error(format!("install_grub failed: {}", e)));
                return;
            }

            // 14. Unmount
            send(ProgressMsg::Update(0.95, "Finalizing installation...".into()));
            if let Err(e) = unmount_target(target) {
                send(ProgressMsg::Error(format!("unmount_target failed: {}", e)));
                return;
            }

            // Done
            send(ProgressMsg::Done(web_url));
        });
    }

    pub fn tick(&mut self) {
        let msgs: Vec<ProgressMsg> = if let Some(rx) = &self.rx {
            rx.try_iter().collect()
        } else {
            return;
        };

        for msg in msgs {
            match msg {
                ProgressMsg::Update(progress, status) => {
                    self.progress = progress.clamp(0.0, 1.0);
                    self.status_text = status;
                }
                ProgressMsg::Done(url) => {
                    self.progress = 1.0;
                    self.status_text = "Installation complete.".into();
                    self.done_url = url;
                    self.done = true;
                }
                ProgressMsg::Error(err) => {
                    self.error = Some(err);
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, _config: &mut InstallConfig) -> ScreenResult {
        if self.done {
            if key.code == KeyCode::Enter {
                let _ = reboot();
            }
        }
        ScreenResult::Continue
    }

    pub fn render(&mut self, frame: &mut Frame, config: &InstallConfig) {
        if !self.started {
            self.start(config);
        }

        self.tick();

        let area = frame.area();
        let buf = frame.buffer_mut();

        // Clear screen
        Block::default()
            .style(Style::default().bg(Color::Black))
            .render(area, buf);

        // Error state
        if let Some(ref err) = self.error {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints([
                    Constraint::Length(1), // title
                    Constraint::Length(1), // gap
                    Constraint::Min(0),   // error
                ])
                .split(area);

            Paragraph::new("Installation Failed")
                .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center)
                .render(chunks[0], buf);

            Paragraph::new(err.as_str())
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: true })
                .render(chunks[2], buf);
            return;
        }

        // Completion state
        if self.done {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(2)
                .constraints([
                    Constraint::Length(1),  // title
                    Constraint::Length(1),  // gap
                    Constraint::Min(0),    // message area
                    Constraint::Length(1),  // gap
                    Constraint::Length(3),  // progress bar area
                    Constraint::Length(1),  // help
                ])
                .split(area);

            Paragraph::new("Installation Complete")
                .style(Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
                .alignment(Alignment::Center)
                .render(chunks[0], buf);

            let msg = format!(
                "CoreVM has been successfully installed.\n\n\
                 Web UI:  {}\n\n\
                 The system will reboot into the CoreVM Appliance.\n\
                 After reboot, the DCUI will be available on tty1.",
                self.done_url
            );
            let content_area = centered_horizontal(chunks[2], 60);
            Paragraph::new(msg)
                .style(Style::default().fg(Color::White))
                .wrap(Wrap { trim: true })
                .render(content_area, buf);

            let bar_area = centered_horizontal(chunks[4], 60);
            Gauge::default()
                .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
                .percent(100)
                .label("100%")
                .render(bar_area, buf);

            Paragraph::new("[Enter] Reboot")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center)
                .render(chunks[5], buf);
            return;
        }

        // Installing state — info slides + progress bar
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(2)
            .constraints([
                Constraint::Length(1),  // 0: title bar
                Constraint::Length(2),  // 1: gap
                Constraint::Min(0),    // 2: slide content area
                Constraint::Length(2),  // 3: gap
                Constraint::Length(1),  // 4: status text
                Constraint::Length(1),  // 5: progress bar
                Constraint::Length(1),  // 6: percentage
            ])
            .split(area);

        // Title
        Paragraph::new("Installing CoreVM Appliance")
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .render(chunks[0], buf);

        // Info slide
        let slide_idx = self.current_slide();
        let (slide_title, slide_body) = INFO_SLIDES[slide_idx];

        let slide_area = centered_horizontal(chunks[2], 56);
        let slide_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // slide title
                Constraint::Length(1), // gap
                Constraint::Min(0),   // slide body
            ])
            .split(slide_area);

        Paragraph::new(slide_title)
            .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
            .alignment(Alignment::Center)
            .render(slide_chunks[0], buf);

        Paragraph::new(slide_body)
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true })
            .render(slide_chunks[2], buf);

        // Slide indicator dots
        let dots: String = (0..INFO_SLIDES.len())
            .map(|i| if i == slide_idx { "o" } else { "." })
            .collect::<Vec<_>>()
            .join(" ");
        // Render dots below slide body if space allows
        if slide_chunks[2].height > 6 {
            let dot_area = Rect {
                y: slide_chunks[2].y + slide_chunks[2].height - 1,
                height: 1,
                ..slide_chunks[2]
            };
            Paragraph::new(dots)
                .style(Style::default().fg(Color::DarkGray))
                .alignment(Alignment::Center)
                .render(dot_area, buf);
        }

        // Status text
        Paragraph::new(self.status_text.as_str())
            .style(Style::default().fg(Color::Gray))
            .alignment(Alignment::Center)
            .render(chunks[4], buf);

        // Progress bar
        let bar_area = centered_horizontal(chunks[5], 60);
        let pct = (self.progress * 100.0) as u16;
        Gauge::default()
            .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
            .percent(pct)
            .render(bar_area, buf);

        // Percentage text
        Paragraph::new(format!("{}%", pct))
            .style(Style::default().fg(Color::Cyan))
            .alignment(Alignment::Center)
            .render(chunks[6], buf);
    }
}

fn centered_horizontal(area: Rect, width: u16) -> Rect {
    let w = width.min(area.width);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    Rect { x, width: w, ..area }
}
