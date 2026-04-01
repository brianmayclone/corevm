//! API router assembly — all REST endpoints for the cluster.

use axum::{Router, extract::DefaultBodyLimit, routing::{get, post, put, delete}};
use std::sync::Arc;
use crate::state::ClusterState;

pub mod auth;
pub mod system;
pub mod users;
pub mod clusters;
pub mod hosts;
pub mod vms;
pub mod storage;
pub mod events;
pub mod tasks;
pub mod drs;
pub mod migration;
pub mod alarms;
pub mod activity;
pub mod notifications;
pub mod cluster_settings;
pub mod network;
pub mod viswitch;
pub mod storage_wizard;
pub mod discovery;
pub mod san;
pub mod logs;

pub fn router() -> Router<Arc<ClusterState>> {
    Router::new()
        // ── Auth ────────────────────────────────────────
        .route("/api/auth/login", post(auth::login))
        .route("/api/auth/me", get(auth::me))

        // ── System ──────────────────────────────────────
        .route("/api/system/info", get(system::info))
        .route("/api/system/stats", get(system::stats))
        .route("/api/system/activity", get(activity::activity))

        // ── Users (admin only) ──────────────────────────
        .route("/api/users", get(users::list).post(users::create))
        .route("/api/users/{id}", put(users::update).delete(users::delete))
        .route("/api/users/{id}/password", put(users::change_password))

        // ── Clusters ────────────────────────────────────
        .route("/api/clusters", get(clusters::list).post(clusters::create))
        .route("/api/clusters/{id}", get(clusters::get).put(clusters::update).delete(clusters::delete))

        // ── Hosts ───────────────────────────────────────
        .route("/api/hosts", get(hosts::list).post(hosts::register))
        .route("/api/hosts/{id}", get(hosts::get).delete(hosts::deregister))
        .route("/api/hosts/{id}/maintenance", post(hosts::enter_maintenance))
        .route("/api/hosts/{id}/activate", post(hosts::exit_maintenance))
        .route("/api/hosts/{id}/rename", put(cluster_settings::rename_host))

        // ── VMs (cluster authority) ─────────────────────
        .route("/api/vms", get(vms::list).post(vms::create))
        .route("/api/vms/{id}", get(vms::get).delete(vms::delete))
        .route("/api/vms/{id}/start", post(vms::start))
        .route("/api/vms/{id}/stop", post(vms::stop))
        .route("/api/vms/{id}/force-stop", post(vms::force_stop))
        .route("/api/vms/{id}/migrate", post(migration::migrate))

        // ── Storage (cluster-wide datastores + compat) ──
        .route("/api/storage/datastores", get(storage::list_datastores).post(storage::create_datastore))
        .route("/api/storage/datastores/{id}", get(storage::get_datastore).delete(storage::delete_datastore))
        .route("/api/storage/pools", get(activity::list_storage_pools))
        .route("/api/storage/pools/{id}", put(activity::update_storage_pool).delete(activity::delete_storage_pool))
        .route("/api/storage/pools/{id}/browse", get(activity::browse_storage_pool))
        .route("/api/storage/stats", get(activity::storage_stats))
        .route("/api/storage/images", get(activity::list_images))
        .route("/api/storage/isos", get(activity::list_isos))
        // Storage Wizard
        .route("/api/storage/wizard/check", post(storage_wizard::check_hosts))
        .route("/api/storage/wizard/install", post(storage_wizard::install_packages))
        .route("/api/storage/wizard/setup", post(storage_wizard::setup))

        // ── Resource Groups (compat) ────────────────────
        .route("/api/resource-groups", get(activity::list_resource_groups))
        .route("/api/resource-groups/permissions-list", get(activity::resource_group_permissions_list))

        // ── Settings (compat + cluster-specific) ────────
        .route("/api/settings/server", get(activity::settings_server))
        .route("/api/settings/time", get(activity::settings_time))
        .route("/api/settings/security", get(activity::settings_security))
        .route("/api/settings/groups", get(activity::list_settings_groups).post(activity::create_settings_group))
        .route("/api/settings/groups/{id}", delete(activity::delete_settings_group))
        .route("/api/settings/smtp", get(cluster_settings::get_smtp).put(cluster_settings::set_smtp))

        // ── SDN Networks (virtual networks with DHCP/DNS/PXE) ───
        .route("/api/networks", get(network::list_networks).post(network::create_network))
        .route("/api/networks/{id}", get(network::get_network).put(network::update_network).delete(network::delete_network))

        // ── Network DHCP reservations + DNS records ─────
        .route("/api/networks/{id}/reservations", post(network::create_reservation))
        .route("/api/networks/{network_id}/{reservation_id}/reservation", delete(network::delete_reservation))
        .route("/api/networks/{id}/dns-records", post(network::create_dns_record))
        .route("/api/networks/{network_id}/{record_id}/dns-record", delete(network::delete_dns_record))
        .route("/api/networks/{id}/pxe-entries", get(network::list_pxe_entries).post(network::create_pxe_entry))
        .route("/api/networks/{network_id}/{entry_id}/pxe-entry", delete(network::delete_pxe_entry))

        // ── viSwitches ──────────────────────────────────
        .route("/api/viswitches", get(viswitch::list).post(viswitch::create))
        .route("/api/viswitches/host-nics", get(viswitch::host_nics))
        .route("/api/viswitches/configure-ip", post(viswitch::configure_ip))
        .route("/api/viswitches/{id}", get(viswitch::get).put(viswitch::update).delete(viswitch::delete))
        .route("/api/viswitches/{id}/uplinks", post(viswitch::add_uplink))
        .route("/api/viswitches/{id}/ports", get(viswitch::list_ports))
        .route("/api/viswitches/{vid}/{uid}/uplink", delete(viswitch::remove_uplink))

        // ── Network (compat stubs) ──────────────────────
        .route("/api/network/interfaces", get(activity::network_interfaces))
        .route("/api/network/stats", get(activity::network_stats))

        // ── Host Logs ──────────────────────────────────
        .route("/api/hosts/{id}/logs", get(logs::host_logs))
        .route("/api/logs", get(logs::all_logs))

        // ── Events ──────────────────────────────────────
        .route("/api/events", get(events::list))
        .route("/api/events/ingest", post(events::ingest))

        // ── Tasks ───────────────────────────────────────
        .route("/api/tasks", get(tasks::list))

        // ── DRS ─────────────────────────────────────────
        .route("/api/drs/recommendations", get(drs::list))
        .route("/api/drs/{id}/apply", post(drs::apply))
        .route("/api/drs/{id}/dismiss", post(drs::dismiss))
        .route("/api/drs/rules", get(drs::list_rules).post(drs::create_rule))
        .route("/api/drs/rules/{id}", put(drs::update_rule).delete(drs::delete_rule))
        .route("/api/drs/exclusions", get(cluster_settings::list_drs_exclusions).post(cluster_settings::create_drs_exclusion))
        .route("/api/drs/exclusions/{id}", delete(cluster_settings::delete_drs_exclusion))

        // ── Alarms ──────────────────────────────────────
        .route("/api/alarms", get(alarms::list))
        .route("/api/alarms/{id}/acknowledge", post(alarms::acknowledge))

        // ── Notifications ──────────────────────────────
        .route("/api/notifications/channels", get(notifications::list_channels).post(notifications::create_channel))
        .route("/api/notifications/channels/{id}", put(notifications::update_channel).delete(notifications::delete_channel))
        .route("/api/notifications/channels/{id}/test", post(notifications::test_channel))
        .route("/api/notifications/rules", get(notifications::list_rules).post(notifications::create_rule))
        .route("/api/notifications/rules/{id}", put(notifications::update_rule).delete(notifications::delete_rule))
        .route("/api/notifications/log", get(notifications::notification_log))

        // ── LDAP / Active Directory ─────────────────────
        .route("/api/ldap", get(cluster_settings::list_ldap).post(cluster_settings::create_ldap))
        .route("/api/ldap/{id}", put(cluster_settings::update_ldap).delete(cluster_settings::delete_ldap))
        .route("/api/ldap/{id}/test", post(cluster_settings::test_ldap))

        // ── Network Discovery ──────────────────────────
        .route("/api/discovery/nodes", get(discovery::list_nodes))
        .route("/api/discovery/servers", get(discovery::unmanaged_servers))
        .route("/api/discovery/san", get(discovery::san_nodes))

        // ── CoreSAN (proxied to vmm-san hosts) ───────
        .route("/api/san/status", get(san::status))
        .route("/api/san/health", get(san::health))
        .route("/api/san/volumes", get(san::list_volumes).post(san::create_volume))
        .route("/api/san/volumes/{id}", get(san::get_volume).put(san::update_volume).delete(san::delete_volume))
        .route("/api/san/volumes/{id}/backends", get(san::list_backends).post(san::add_backend))
        .route("/api/san/volumes/{vid}/backends/{bid}", delete(san::remove_backend))
        .route("/api/san/peers", get(san::list_peers))
        .route("/api/san/disks", get(san::list_disks))
        .route("/api/san/disks/{host_id}/{device_name}/smart", get(san::disk_smart_detail))
        .route("/api/san/disks/claim", post(san::claim_disk))
        .route("/api/san/disks/release", post(san::release_disk))
        .route("/api/san/disks/reset", post(san::reset_disk))
        .route("/api/san/disks/create-file", post(san::create_file_disk))
        .route("/api/san/volumes/{id}/health", get(san::volume_health))
        .route("/api/san/volumes/{id}/repair", post(san::volume_repair))
        .route("/api/san/volumes/{id}/remove-host", post(san::volume_remove_host))
        .route("/api/san/volumes/{id}/chunk-map", get(san::chunk_map))
        .route("/api/san/volumes/{id}/allocate-disk", post(san::allocate_disk))
        .route("/api/san/volumes/{id}/browse", get(san::browse_volume_root))
        .route("/api/san/volumes/{id}/browse/{*path}", get(san::browse_volume))
        .route("/api/san/volumes/{id}/mkdir", post(san::mkdir_volume))
        .route("/api/san/volumes/{id}/files/{*path}", put(san::upload_file).delete(san::delete_file))
        .route("/api/san/benchmark", get(san::benchmark_matrix))
        .route("/api/san/benchmark/matrix", get(san::benchmark_matrix))
        .route("/api/san/benchmark/run", post(san::run_benchmark))
        .route("/api/san/witness/{node_id}", get(san::witness))

        // ── S3 Credentials (proxied) ─────────────────────
        .route("/api/san/s3/credentials", get(san::list_s3_credentials).post(san::create_s3_credential))
        .route("/api/san/s3/credentials/{id}", delete(san::delete_s3_credential))

        // ── WebSocket ───────────────────────────────────
        .route("/ws/console/{vm_id}", get(crate::ws::console_bridge::handler))
        .route("/ws/terminal", get(crate::ws::terminal::ws_terminal))
        // Allow uploads up to 10 GB (ISOs, disk images forwarded to SAN)
        .layer(DefaultBodyLimit::max(10 * 1024 * 1024 * 1024))
}
