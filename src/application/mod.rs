// Application Layer
// Use cases and orchestration logic.

pub mod add_ssh_key;
pub mod app_deployer; // TASK-D3: App deployment from Git repos
pub mod apparmor_manager; // AA-004..007 (204-207) — AppArmor orchestration
pub mod backup_system;
pub mod cleanup_service;
pub mod config_inspector; // CFG-NEW-001: inspector de configuracin centralizada
pub mod dependency_manager;
pub mod deploy_fileserver;
pub mod deploy_git_server;
pub mod deploy_ssh_hidden_service;
pub mod deploy_static_site;
pub mod deploy_tor_service;
pub mod deploy_tor_web_service;
pub mod deploy_wordpress;
pub mod edit_port_config;
pub mod edit_wordpress_config;
pub mod firewall_manager; // UFW-004 (192)
pub mod fwknop_config; // Added FwknopConfig
pub mod git_registration_toggle; // Added GitRegistrationToggle
pub mod list_tor_services;
pub mod manage_client_auth;
pub mod nginx_status_checker;
pub mod port_validator; // PORTS-001 (175)
pub mod release_verify; // RELEASE-VERIFY (PQC-030): verificación de releases (ML-DSA-65 + SHA-256) desde enola-cli
pub mod remove_tor_service;
pub mod resource_alerts; // Task 15.6: Resource Alerts
pub mod rotate_tor_identity;
pub mod secure_wordpress_update; // Added SecureWordPressUpdate
pub mod ssh_status_check;
pub mod start_tor_service;
pub mod stop_tor_service;
pub mod system_doctor; // OBS-001: diagnstico global multi-seccin
pub mod system_health_check;
pub mod system_resource_monitor;
pub mod toggle_wordpress;
pub mod tor_service_manager;
pub mod update_checker; // UPD-CLI-001/002: verificador de actualizaciones y feed advisories
pub mod update_wordpress;
pub mod vpn_manager; // Tarea 162 — WireGuard VPN orchestration
pub mod vpn_tor_manager; // VPN over Tor — socat bridge + hidden service orchestration
pub mod web_api; // WEB-GUI: REST API handlers
pub mod web_errors;
pub mod web_server; // WEB-GUI: embedded web dashboard server
pub mod wordpress_config_editor; // Added WordPressConfigEditor
pub mod wordpress_connector;
pub mod wordpress_status_check; // Added WordPressStatusCheck // DEP-001..003 — Setup/Doctor dependency management // WEB-GUI: error conversion to JSON HTTP responses
