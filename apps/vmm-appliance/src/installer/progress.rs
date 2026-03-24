use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::thread;

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::Frame;

use crate::common::{
    certs::{generate_self_signed, import_certificates},
    config::{ApplianceConfig, ApplianceRole, write_vmm_cluster_config, write_vmm_server_config},
    firewall::{write_nftables_config, FirewallConfig},
    network::write_networkd_config,
    system::{
        configure_chrony, configure_fstab, create_user, extract_rootfs, format_partitions,
        install_grub, is_efi_booted, mount_target, partition_disk, reboot, set_hostname,
        set_locale, set_root_password, set_timezone, unmount_target,
    },
    widgets::ProgressDisplay,
};

use super::{InstallConfig, ScreenResult};

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
    display: ProgressDisplay,
    rx: Option<mpsc::Receiver<ProgressMsg>>,
    started: bool,
    done: bool,
    done_url: String,
    error: Option<String>,
}

impl ProgressState {
    pub fn new() -> Self {
        Self {
            display: ProgressDisplay::new("Installing CoreVM Appliance"),
            rx: None,
            started: false,
            done: false,
            done_url: String::new(),
            error: None,
        }
    }

    fn start(&mut self, config: &InstallConfig) {
        self.started = true;

        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);

        // Clone everything the thread needs
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
            send(ProgressMsg::Update(0.20, "Extracting Debian rootfs...".into()));
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
            send(ProgressMsg::Update(0.60, "Configuring NTP...".into()));
            let ntp_srv = if ntp_server.is_empty() { "pool.ntp.org".to_string() } else { ntp_server };
            if let Err(e) = configure_chrony(target, ntp_enabled, &ntp_srv) {
                send(ProgressMsg::Error(format!("configure_chrony failed: {}", e)));
                return;
            }

            // 8. Create users
            send(ProgressMsg::Update(0.65, "Creating users...".into()));
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
            send(ProgressMsg::Update(0.70, "Configuring certificates...".into()));
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
            let web_url = match &role {
                ApplianceRole::Server => {
                    let log_file = "/var/log/vmm/vmm-server.log";
                    if let Err(e) = write_vmm_server_config(target, server_port, data_dir, log_file) {
                        send(ProgressMsg::Error(format!("write_vmm_server_config failed: {}", e)));
                        return;
                    }
                    format!("https://{}:{}", hostname, server_port)
                }
                ApplianceRole::Cluster => {
                    let log_file = "/var/log/vmm/vmm-cluster.log";
                    if let Err(e) = write_vmm_cluster_config(target, cluster_port, data_dir, log_file) {
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

            // 11. Firewall
            send(ProgressMsg::Update(0.80, "Configuring firewall...".into()));
            let fw_config = FirewallConfig {
                ssh_port: 22,
                vmm_server_port: if matches!(role, ApplianceRole::Server) { Some(server_port) } else { None },
                vmm_cluster_port: if matches!(role, ApplianceRole::Cluster) { Some(cluster_port) } else { None },
            };
            if let Err(e) = write_nftables_config(target, &fw_config) {
                send(ProgressMsg::Error(format!("write_nftables_config failed: {}", e)));
                return;
            }

            // 12. SSH hardening
            send(ProgressMsg::Update(0.85, "Configuring SSH...".into()));
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
            send(ProgressMsg::Update(0.90, "Installing GRUB bootloader...".into()));
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
                    self.display.set_progress(progress, &status);
                }
                ProgressMsg::Done(url) => {
                    self.display.set_progress(1.0, "Installation complete.");
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

        let outer = Block::default()
            .title(" CoreVM Appliance Installer — Installing ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

        let inner = outer.inner(area);
        frame.render_widget(outer, area);

        if let Some(ref err) = self.error {
            let msg = format!("Installation failed:\n\n{}", err);
            let para = Paragraph::new(msg.as_str())
                .style(Style::default().fg(Color::Red))
                .wrap(ratatui::widgets::Wrap { trim: true });
            frame.render_widget(para, inner);
            return;
        }

        if self.done {
            // Split inner area for progress widget + completion message
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Min(0),
                ])
                .split(inner);

            // Progress bar (completed)
            frame.render_widget(ProgressDisplayWidget(&self.display), chunks[0]);

            // Completion message
            let msg = format!(
                "Installation complete!\n\nWeb UI: {}\n\nPress Enter to reboot.",
                self.done_url
            );
            let para = Paragraph::new(msg.as_str())
                .style(Style::default().fg(Color::Green))
                .wrap(ratatui::widgets::Wrap { trim: true });
            frame.render_widget(para, chunks[1]);
        } else {
            // During install: just progress widget centered
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(4),
                    Constraint::Min(0),
                ])
                .split(inner);

            frame.render_widget(ProgressDisplayWidget(&self.display), chunks[0]);
        }
    }
}

// ---------------------------------------------------------------------------
// Adapter to render ProgressDisplay as a Widget
// ---------------------------------------------------------------------------

struct ProgressDisplayWidget<'a>(&'a ProgressDisplay);

impl<'a> Widget for ProgressDisplayWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        self.0.render(area, buf);
    }
}
