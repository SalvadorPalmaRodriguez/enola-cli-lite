use axum::extract::{Path, Query};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::application::web_errors::{ApiError, ApiResult};
use crate::application::web_server::AppState;
use crate::cli::commands;

/// Strip ANSI escape codes from a string (for web UI display).
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip escape sequence: ESC [ ... letter
            if chars.peek() == Some(&'[') {
                chars.next();
                for c2 in chars.by_ref() {
                    if c2.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                // Skip single-char escape (ESC X)
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

pub fn api_routes(state: Arc<AppState>) -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(api_status))
        .route("/services", get(api_list_services))
        // Tor
        .route("/tor", get(api_tor_list))
        .route("/tor/create", post(api_tor_create))
        .route("/tor/{name}/start", post(api_tor_start))
        .route("/tor/{name}/stop", post(api_tor_stop))
        .route("/tor/{name}/remove", post(api_tor_remove))
        .route("/tor/{name}/edit", post(api_tor_edit))
        .route("/tor/{name}/detail", get(api_tor_detail))
        .route("/tor/{name}/rotate", post(api_tor_rotate))
        .route("/tor/publish/{service_type}/{name}", post(api_tor_publish))
        .route("/tor/hide/{service_type}/{name}", post(api_tor_hide))
        // Tor Auth
        .route("/tor/auth/{service}/list", get(api_tor_auth_list))
        .route("/tor/auth/{service}/enable", post(api_tor_auth_enable))
        .route("/tor/auth/{service}/disable", post(api_tor_auth_disable))
        .route("/tor/auth/{service}/add", post(api_tor_auth_add))
        .route("/tor/auth/{service}/revoke", post(api_tor_auth_revoke))
        .route("/tor/auth/generate", post(api_tor_auth_generate))
        .route("/tor/auth/{service}/rotate", post(api_tor_auth_rotate))
        // Console (universal CLI access)
        .route("/console/help", get(api_console_help))
        .route("/console/help/{command}", get(api_console_help_command))
        .route("/console/run", post(api_console_run))
        // Git
        .route("/git", get(api_git_list))
        .route("/git/create", post(api_git_create))
        .route("/git/{name}/start", post(api_git_start))
        .route("/git/{name}/stop", post(api_git_stop))
        .route("/git/{name}/status", get(api_git_status))
        .route("/git/{name}/delete", post(api_git_delete))
        .route("/git/{name}/publish", post(api_git_publish))
        .route("/git/{name}/hide", post(api_git_hide))
        .route("/git/{name}/registration", post(api_git_registration))
        .route(
            "/git/{name}/registration/status",
            get(api_git_registration_status),
        )
        .route("/git/{name}/edit", post(api_git_edit))
        .route("/git/user/list", post(api_git_user_list))
        .route("/git/user/create", post(api_git_user_create))
        .route("/git/user/delete", post(api_git_user_delete))
        .route("/git/watcher", post(api_git_watcher))
        // WordPress
        .route("/wp", get(api_wp_list))
        .route("/wp/create", post(api_wp_create))
        .route("/wp/{name}/start", post(api_wp_start))
        .route("/wp/{name}/stop", post(api_wp_stop))
        .route("/wp/{name}/restart", post(api_wp_restart))
        .route("/wp/{name}/delete", post(api_wp_delete))
        .route("/wp/{name}/publish", post(api_wp_publish))
        .route("/wp/{name}/hide", post(api_wp_hide))
        .route("/wp/{name}/update", post(api_wp_update))
        .route("/wp/{name}/config", get(api_wp_config))
        .route("/wp/{name}/status", get(api_wp_status))
        .route("/wp/{name}/edit", post(api_wp_edit))
        // CMS (Drupal, Ghost, Magnolia, Strapi, Wagtail)
        .route("/cms/{cms_type}/list", get(api_cms_list))
        .route("/cms/{cms_type}/create", post(api_cms_create))
        .route("/cms/{cms_type}/{name}/start", post(api_cms_start))
        .route("/cms/{cms_type}/{name}/stop", post(api_cms_stop))
        .route("/cms/{cms_type}/{name}/delete", post(api_cms_delete))
        .route("/cms/{cms_type}/{name}/status", get(api_cms_status))
        .route("/cms/{cms_type}/{name}/edit", post(api_cms_edit))
        .route("/cms/{cms_type}/{name}/publish", post(api_cms_publish))
        .route("/cms/{cms_type}/{name}/hide", post(api_cms_hide))
        .route("/cms/strapi/build-image", post(api_strapi_build_image))
        // Files
        .route("/files", get(api_files_list))
        .route("/files/create", post(api_files_create))
        .route("/files/{name}/delete", post(api_files_delete))
        .route("/files/{name}/edit", post(api_files_edit))
        .route("/files/{name}/fix-perms", post(api_files_fix_perms))
        // Ports
        .route("/ports", get(api_ports_list))
        // Doctor
        .route("/doctor", get(api_doctor))
        // Firewall
        .route("/firewall/status", get(api_firewall_status))
        .route("/firewall/setup", post(api_firewall_setup))
        .route("/firewall/allow", post(api_firewall_allow))
        .route("/firewall/deny", post(api_firewall_deny))
        // AppArmor
        .route("/apparmor/status", get(api_apparmor_status))
        .route("/apparmor/setup", post(api_apparmor_setup))
        .route("/apparmor/mode", post(api_apparmor_mode))
        // VPN
        .route("/vpn/list", get(api_vpn_list))
        .route("/vpn/status/{interface}", get(api_vpn_status))
        .route("/vpn/create", post(api_vpn_create))
        .route("/vpn/{interface}/start", post(api_vpn_start))
        .route("/vpn/{interface}/stop", post(api_vpn_stop))
        .route("/vpn/{interface}/delete", post(api_vpn_delete))
        .route("/vpn/peer/add", post(api_vpn_peer_add))
        .route("/vpn/peer/add-pubkey", post(api_vpn_peer_add_pubkey))
        .route("/vpn/peer/remove", post(api_vpn_peer_remove))
        // Logs
        .route("/logs/sources", get(api_logs_sources))
        .route("/logs/view", get(api_logs_view))
        .route("/logs/install", get(api_logs_install))
        .route("/logs/smoke-test", get(api_logs_smoke_test))
        // Maintenance
        .route("/maintenance/status", get(api_maintenance_status))
        .route("/maintenance/smoke-test", post(api_maintenance_smoke_test))
        .route(
            "/maintenance/enable-checks",
            post(api_maintenance_enable_checks),
        )
        .route(
            "/maintenance/disable-checks",
            post(api_maintenance_disable_checks),
        )
        .route(
            "/maintenance/timer-status",
            get(api_maintenance_timer_status),
        )
        .route("/maintenance/ssh-config", get(api_maintenance_ssh_config))
        .route(
            "/maintenance/ssh-harden-pqc",
            post(api_maintenance_ssh_harden_pqc),
        )
        .route("/maintenance/backup", post(api_maintenance_backup))
        .route("/maintenance/cleanup", post(api_maintenance_cleanup))
        // Diagnostics
        .route("/diag/summary", get(api_diag_summary))
        .route("/diag/nginx", get(api_diag_nginx))
        .route("/diag/tor", get(api_diag_tor))
        .route("/diag/ssh", get(api_diag_ssh))
        .route("/diag/wordpress", get(api_diag_wordpress))
        .route("/diag/wp-sync", get(api_diag_wp_sync))
        .route("/diag/nginx-test", get(api_diag_nginx_test))
        .route("/diag/resources", get(api_diag_resources))
        // Test
        .route("/test/run", post(api_test_run))
        .route("/test/list", get(api_test_list))
        .route("/test/benchmark", post(api_test_benchmark))
        .route("/test/results", get(api_test_results))
        .route("/test/clean", post(api_test_clean))
        // Setup
        .route("/setup", post(api_setup))
        .route("/setup/pqc-tls", get(api_setup_pqc_tls_sse))
        // System (quickref, license, config, verify, uninstall)
        .route("/quickref", get(api_quickref))
        .route("/license", get(api_license))
        .route("/config/show", get(api_config_show))
        .route("/config/validate", post(api_config_validate))
        .route("/verify", post(api_verify))
        .route("/uninstall", post(api_uninstall))
        // Docs
        .route("/docs/{*topic}", get(api_docs))
        // Update
        .route("/update/check", post(api_update_check))
        .route("/update/schema", get(api_update_schema))
        .route("/update/download", post(api_update_download))
        .route("/update/apply", post(api_update_apply))
        .route("/update/verify-feed", post(api_update_verify_feed))
        // Doctor security
        .route("/doctor/security", get(api_doctor_security))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::application::web_server::auth_middleware,
        ))
}

// ── Status ───────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct StatusResponse {
    version: &'static str,
    status: &'static str,
}

async fn api_status() -> ApiResult<StatusResponse> {
    Ok(Json(StatusResponse {
        version: env!("CARGO_PKG_VERSION"),
        status: "ok",
    }))
}

// ── Services (aggregated) ─────────────────────────────────────────────────────

#[derive(Serialize)]
struct ServiceSummary {
    service_type: String,
    name: String,
    status: String,
    onion: Option<String>,
    port: Option<u16>,
}

async fn api_list_services() -> ApiResult<Vec<ServiceSummary>> {
    let mut services: Vec<ServiceSummary> = Vec::new();

    let git = commands::git::list().await.map_err(ApiError::from)?;
    for g in git {
        services.push(ServiceSummary {
            service_type: "git".to_string(),
            name: g.name,
            status: g.status,
            onion: g.onion_address,
            port: g.http_port,
        });
    }

    let wp = commands::wordpress::list().await.map_err(ApiError::from)?;
    for w in wp {
        services.push(ServiceSummary {
            service_type: "wordpress".to_string(),
            name: w.name,
            status: w.status,
            onion: w.onion_address,
            port: w.port,
        });
    }

    let tor = commands::tor::list().await.map_err(ApiError::from)?;
    for t in tor {
        services.push(ServiceSummary {
            service_type: "tor".to_string(),
            name: t.name,
            status: if t.active {
                "active".to_string()
            } else {
                "inactive".to_string()
            },
            onion: Some(t.hostname),
            port: t.ports.first().map(|(p, _)| *p),
        });
    }

    Ok(Json(services))
}

// ── Tor ───────────────────────────────────────────────────────────────────────

async fn api_tor_list() -> ApiResult<Vec<crate::ports::tor::TorServiceInfo>> {
    let list = commands::tor::list().await.map_err(ApiError::from)?;
    Ok(Json(list))
}

#[derive(Deserialize)]
struct TorPublishRequest {
    ssl: Option<bool>,
}

async fn api_tor_publish(
    Path((service_type, name)): Path<(String, String)>,
    Json(req): Json<TorPublishRequest>,
) -> ApiResult<String> {
    let ssl = req.ssl.unwrap_or(false);
    let result = match service_type.as_str() {
        "git" => commands::git::publish(&name, ssl)
            .await
            .map_err(ApiError::from)?,
        "wordpress" | "wp" => commands::wordpress::publish(&name)
            .await
            .map_err(ApiError::from)?,
        "drupal" => commands::drupal::publish(&name)
            .await
            .map_err(ApiError::from)?,
        "ghost" => commands::ghost::publish(&name)
            .await
            .map_err(ApiError::from)?,
        "magnolia" => commands::magnolia::publish(&name)
            .await
            .map_err(ApiError::from)?,
        "strapi" => commands::strapi::publish(&name)
            .await
            .map_err(ApiError::from)?,
        "wagtail" => commands::wagtail::publish(&name)
            .await
            .map_err(ApiError::from)?,
        _ => {
            return Err(ApiError {
                error: format!("Unknown service type: {}", service_type),
                code: 400,
            })
        }
    };
    Ok(Json(result))
}

async fn api_tor_hide(Path((service_type, name)): Path<(String, String)>) -> ApiResult<()> {
    match service_type.as_str() {
        "git" => commands::git::hide(&name).await.map_err(ApiError::from)?,
        "wordpress" | "wp" => commands::wordpress::hide(&name)
            .await
            .map_err(ApiError::from)?,
        "drupal" => commands::drupal::hide(&name)
            .await
            .map_err(ApiError::from)?,
        "ghost" => commands::ghost::hide(&name).await.map_err(ApiError::from)?,
        "magnolia" => commands::magnolia::hide(&name)
            .await
            .map_err(ApiError::from)?,
        "strapi" => commands::strapi::hide(&name)
            .await
            .map_err(ApiError::from)?,
        "wagtail" => commands::wagtail::hide(&name)
            .await
            .map_err(ApiError::from)?,
        _ => {
            return Err(ApiError {
                error: format!("Unknown service type: {}", service_type),
                code: 400,
            })
        }
    };
    Ok(Json(()))
}

async fn api_tor_rotate(Path(name): Path<String>) -> ApiResult<String> {
    match commands::tor::rotate(&name).await {
        Ok(result) => Ok(Json(result)),
        Err(e) => {
            let msg = format!("{}", e);
            if msg.contains("Timeout waiting for new address") {
                Ok(Json(format!(
                    "⚠️  Rotation in progress for '{}'. The new .onion address will appear within 30-60s. Use 'enola-cli tor list' to check.",
                    name
                )))
            } else {
                Err(ApiError::from(e))
            }
        }
    }
}

// ── Git ───────────────────────────────────────────────────────────────────────

async fn api_git_list() -> ApiResult<Vec<commands::git::GitServerInfo>> {
    let list = commands::git::list().await.map_err(ApiError::from)?;
    Ok(Json(list))
}

#[derive(Deserialize)]
struct GitCreateRequest {
    name: String,
    ssl: Option<bool>,
    admin_user: Option<String>,
    admin_pass: Option<String>,
    http_port: Option<u16>,
    ssh_port: Option<u16>,
}

async fn api_git_create(Json(req): Json<GitCreateRequest>) -> ApiResult<String> {
    use crate::adapters::infra::port_checker::PortCheckerAdapter;
    use crate::application::port_validator::{PortRanges, PortValidator};
    let validator = PortValidator::new(std::sync::Arc::new(PortCheckerAdapter::new()));
    let http_port = validator
        .resolve_port(req.http_port, PortRanges::GIT_HTTP, "http-port")
        .map_err(|e| ApiError {
            error: format!("{}", e),
            code: 400,
        })?;
    let ssh_port = validator
        .resolve_port(req.ssh_port, PortRanges::GIT_SSH, "ssh-port")
        .map_err(|e| ApiError {
            error: format!("{}", e),
            code: 400,
        })?;
    let result = commands::git::create(
        &req.name,
        req.ssl.unwrap_or(false),
        req.admin_user.as_deref(),
        req.admin_pass.as_deref(),
        http_port,
        ssh_port,
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn api_git_start(Path(name): Path<String>) -> ApiResult<()> {
    commands::git::start(&name).await.map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_git_stop(Path(name): Path<String>) -> ApiResult<()> {
    commands::git::stop(&name).await.map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_git_delete(Path(name): Path<String>) -> ApiResult<()> {
    commands::git::delete(&name).await.map_err(ApiError::from)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
struct GitPublishRequest {
    ssl: Option<bool>,
}

async fn api_git_publish(
    Path(name): Path<String>,
    Json(req): Json<GitPublishRequest>,
) -> ApiResult<String> {
    let result = commands::git::publish(&name, req.ssl.unwrap_or(false))
        .await
        .map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn api_git_hide(Path(name): Path<String>) -> ApiResult<()> {
    commands::git::hide(&name).await.map_err(ApiError::from)?;
    Ok(Json(()))
}

// ── WordPress ─────────────────────────────────────────────────────────────────

async fn api_wp_list() -> ApiResult<Vec<commands::wordpress::WordPressSiteInfo>> {
    let list = commands::wordpress::list().await.map_err(ApiError::from)?;
    Ok(Json(list))
}

#[derive(Deserialize)]
struct WpCreateRequest {
    name: String,
    http_port: Option<u16>,
}

async fn api_wp_create(Json(req): Json<WpCreateRequest>) -> ApiResult<String> {
    let result = commands::wordpress::create(&req.name, req.http_port)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn api_wp_start(Path(name): Path<String>) -> ApiResult<()> {
    commands::wordpress::start(&name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_wp_stop(Path(name): Path<String>) -> ApiResult<()> {
    commands::wordpress::stop(&name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_wp_delete(Path(name): Path<String>) -> ApiResult<()> {
    commands::wordpress::delete(&name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_wp_publish(Path(name): Path<String>) -> ApiResult<String> {
    let result = commands::wordpress::publish(&name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn api_wp_hide(Path(name): Path<String>) -> ApiResult<()> {
    commands::wordpress::hide(&name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

// ── CMS (Drupal, Ghost, Magnolia, Strapi, Wagtail) — publish/hide only ────────

async fn api_cms_publish(Path((cms_type, name)): Path<(String, String)>) -> ApiResult<String> {
    let result = match cms_type.as_str() {
        "drupal" => commands::drupal::publish(&name).await,
        "ghost" => commands::ghost::publish(&name).await,
        "magnolia" => commands::magnolia::publish(&name).await,
        "strapi" => commands::strapi::publish(&name).await,
        "wagtail" => commands::wagtail::publish(&name).await,
        _ => {
            return Err(ApiError {
                error: format!("Unknown CMS type: {}", cms_type),
                code: 400,
            })
        }
    }
    .map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn api_cms_hide(Path((cms_type, name)): Path<(String, String)>) -> ApiResult<()> {
    match cms_type.as_str() {
        "drupal" => commands::drupal::hide(&name).await,
        "ghost" => commands::ghost::hide(&name).await,
        "magnolia" => commands::magnolia::hide(&name).await,
        "strapi" => commands::strapi::hide(&name).await,
        "wagtail" => commands::wagtail::hide(&name).await,
        _ => {
            return Err(ApiError {
                error: format!("Unknown CMS type: {}", cms_type),
                code: 400,
            })
        }
    }
    .map_err(ApiError::from)?;
    Ok(Json(()))
}

// ── Files ─────────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
struct FileShareInfo {
    share_path: Option<String>,
    #[serde(flatten)]
    service: crate::ports::tor::TorServiceInfo,
}

fn read_share_path_from_nginx(service_name: &str) -> Option<String> {
    let candidates = [
        format!("/etc/nginx/sites-available/fileserver_{}", service_name),
        format!("/etc/nginx/sites-available/{}", service_name),
    ];
    for cfg_path in &candidates {
        if let Ok(content) = std::fs::read_to_string(cfg_path) {
            for line in content.lines() {
                let trimmed = line.trim();
                if let Some(rest) = trimmed.strip_prefix("root ") {
                    return Some(rest.trim_end_matches(';').trim().to_string());
                }
            }
        }
    }
    None
}

async fn api_files_list() -> ApiResult<Vec<FileShareInfo>> {
    let list = commands::files::list().await.map_err(ApiError::from)?;
    let enriched = list
        .into_iter()
        .map(|s| {
            let share_path = read_share_path_from_nginx(&s.name);
            FileShareInfo {
                service: s,
                share_path,
            }
        })
        .collect();
    Ok(Json(enriched))
}

#[derive(Deserialize)]
struct FilesCreateRequest {
    name: String,
    auth: Option<bool>,
    ssl: Option<bool>,
}

async fn api_files_create(Json(req): Json<FilesCreateRequest>) -> ApiResult<String> {
    let result = commands::files::create(
        &req.name,
        req.auth.unwrap_or(false),
        req.ssl.unwrap_or(false),
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn api_files_delete(Path(name): Path<String>) -> ApiResult<()> {
    commands::files::delete(&name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

// ── Ports ─────────────────────────────────────────────────────────────────────

async fn api_ports_list() -> ApiResult<Vec<commands::ports::PortEntry>> {
    let list = commands::ports::list_all_ports()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(list))
}

// ── Doctor ─────────────────────────────────────────────────────────────────────

async fn api_doctor() -> ApiResult<String> {
    let report = crate::application::system_doctor::full_report().await;
    Ok(Json(strip_ansi(&report)))
}

// ── Firewall ──────────────────────────────────────────────────────────────────

async fn api_firewall_status() -> ApiResult<crate::domain::firewall::FirewallStatus> {
    use crate::adapters::infra::ufw::UfwAdapter;
    use crate::application::firewall_manager::FirewallManager;
    let mgr = FirewallManager::new(std::sync::Arc::new(UfwAdapter::new()));
    let status = mgr.get_status().map_err(|e| ApiError {
        error: format!("{}", e),
        code: 500,
    })?;
    Ok(Json(status))
}

async fn api_apparmor_status() -> ApiResult<crate::domain::apparmor::AppArmorStatus> {
    use crate::adapters::infra::apparmor::AppArmorAdapter;
    use crate::application::apparmor_manager::AppArmorManager;
    let mgr = AppArmorManager::new(std::sync::Arc::new(AppArmorAdapter::new()));
    let status = mgr.get_status().map_err(|e| ApiError {
        error: format!("{}", e),
        code: 500,
    })?;
    Ok(Json(status))
}

#[derive(Deserialize)]
struct AppArmorModeRequest {
    mode: String,
    profile: Option<String>,
}

async fn api_apparmor_mode(Json(req): Json<AppArmorModeRequest>) -> ApiResult<String> {
    use crate::adapters::infra::apparmor::AppArmorAdapter;
    use crate::application::apparmor_manager::AppArmorManager;
    use crate::domain::apparmor::AppArmorMode;
    let mode: AppArmorMode = req.mode.parse().map_err(|e| ApiError {
        error: e,
        code: 400,
    })?;
    let mgr = AppArmorManager::new(std::sync::Arc::new(AppArmorAdapter::new()));
    let result = mgr
        .change_mode(mode, req.profile.as_deref())
        .map_err(|e| ApiError {
            error: format!("{}", e),
            code: 500,
        })?;
    Ok(Json(result))
}

// ── VPN ───────────────────────────────────────────────────────────────────────

async fn api_vpn_list() -> ApiResult<Vec<String>> {
    use crate::adapters::infra::vpn::WireGuardAdapter;
    use crate::application::vpn_manager::VpnManager;
    let mgr = VpnManager::new(std::sync::Arc::new(WireGuardAdapter::new()));
    let list = mgr.list_vpns().map_err(|e| ApiError {
        error: format!("{}", e),
        code: 500,
    })?;
    Ok(Json(list))
}

async fn api_vpn_status(
    Path(interface): Path<String>,
) -> ApiResult<crate::ports::vpn::VpnInterfaceStatus> {
    use crate::adapters::infra::vpn::WireGuardAdapter;
    use crate::application::vpn_manager::VpnManager;
    let mgr = VpnManager::new(std::sync::Arc::new(WireGuardAdapter::new()));
    let status = mgr.get_status(&interface).map_err(|e| ApiError {
        error: format!("{}", e),
        code: 500,
    })?;
    Ok(Json(status))
}

async fn api_vpn_start(Path(interface): Path<String>) -> ApiResult<()> {
    use crate::adapters::infra::vpn::WireGuardAdapter;
    use crate::application::vpn_manager::VpnManager;
    let mgr = VpnManager::new(std::sync::Arc::new(WireGuardAdapter::new()));
    mgr.start_vpn(&interface).map_err(|e| ApiError {
        error: format!("{}", e),
        code: 500,
    })?;
    Ok(Json(()))
}

async fn api_vpn_stop(Path(interface): Path<String>) -> ApiResult<()> {
    use crate::adapters::infra::vpn::WireGuardAdapter;
    use crate::application::vpn_manager::VpnManager;
    let mgr = VpnManager::new(std::sync::Arc::new(WireGuardAdapter::new()));
    mgr.stop_vpn(&interface).map_err(|e| ApiError {
        error: format!("{}", e),
        code: 500,
    })?;
    Ok(Json(()))
}

#[derive(Deserialize)]
struct VpnDeleteRequest {
    sync_firewall: Option<bool>,
}

async fn api_vpn_delete(
    Path(interface): Path<String>,
    Json(req): Json<VpnDeleteRequest>,
) -> ApiResult<()> {
    use crate::adapters::infra::vpn::WireGuardAdapter;
    use crate::application::vpn_manager::VpnManager;
    use crate::ports::manifest::ManifestPort;
    let mgr = VpnManager::new(std::sync::Arc::new(WireGuardAdapter::new()));

    let vpn_port = if req.sync_firewall.unwrap_or(false) {
        mgr.get_status(&interface).map(|s| s.listen_port).ok()
    } else {
        None
    };

    mgr.delete_vpn(&interface).map_err(|e| ApiError {
        error: format!("{}", e),
        code: 500,
    })?;

    let manifest = crate::adapters::infra::manifest::FileManifestAdapter::new();
    let _ = manifest.remove("vpn_config", &interface);
    let _ = manifest.remove("vpn_service", &format!("wg-quick@{}", interface));

    if let Some(port) = vpn_port {
        use crate::adapters::infra::ufw::UfwAdapter;
        use crate::domain::firewall::FirewallProtocol;
        use crate::ports::firewall::FirewallPort;
        let ufw = UfwAdapter::new();
        if ufw.is_active().unwrap_or(false) {
            if let Ok(()) = ufw.deny_port(port, FirewallProtocol::Udp) {
                let _ = manifest.remove("ufw_rule", &port.to_string());
            }
        }
    }

    Ok(Json(()))
}

// ── Logs ──────────────────────────────────────────────────────────────────────

async fn api_logs_sources() -> ApiResult<Vec<String>> {
    let sources = commands::logs::list().await.map_err(ApiError::from)?;
    Ok(Json(sources))
}

#[derive(Deserialize)]
struct LogsViewQuery {
    source: String,
    lines: Option<usize>,
}

async fn api_logs_view(Query(q): Query<LogsViewQuery>) -> ApiResult<Vec<String>> {
    let lines = q.lines.unwrap_or(50);
    let logs = commands::logs::view(&q.source, lines, false)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(logs))
}

// ── Tor lifecycle ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TorCreateRequest {
    name: String,
    service_type: String,
    virtual_port: Option<u16>,
    target_port: Option<u16>,
    ssl: Option<bool>,
}

async fn api_tor_create(Json(req): Json<TorCreateRequest>) -> ApiResult<String> {
    let result = commands::tor::create(
        &req.name,
        &req.service_type,
        req.virtual_port.unwrap_or(80),
        req.target_port,
        req.ssl.unwrap_or(false),
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn api_tor_start(Path(name): Path<String>) -> ApiResult<()> {
    commands::tor::start(&name).await.map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_tor_stop(Path(name): Path<String>) -> ApiResult<()> {
    commands::tor::stop(&name).await.map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_tor_remove(Path(name): Path<String>) -> ApiResult<()> {
    commands::tor::remove(&name).await.map_err(ApiError::from)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
struct TorEditRequest {
    virtual_port: Option<u16>,
    nginx_port: Option<u16>,
    target_port: Option<u16>,
    auto_ports: Option<bool>,
    force: Option<bool>,
}

#[derive(Serialize)]
struct TorEditResponse {
    message: String,
    warning: Option<String>,
    applied: bool,
}

async fn api_tor_edit(
    Path(name): Path<String>,
    Json(req): Json<TorEditRequest>,
) -> ApiResult<TorEditResponse> {
    // If virtual_port is non-standard and not forced, return warning without applying
    if let Some(vp) = req.virtual_port {
        if vp != 80 && vp != 443 && !req.force.unwrap_or(false) {
            return Ok(Json(TorEditResponse {
                message: format!(
                    "El puerto virtual {} no es estándar (80=HTTP, 443=HTTPS).",
                    vp
                ),
                warning: Some(format!(
                    "El puerto virtual {} no es estándar (80=HTTP, 443=HTTPS). \
                     Los visitantes tendrán que escribir .onion:{} en el navegador. \
                     ¿Continuar?",
                    vp, vp
                )),
                applied: false,
            }));
        }
    }

    let result = commands::tor::edit(
        &name,
        req.virtual_port,
        req.nginx_port,
        req.target_port,
        req.auto_ports.unwrap_or(false),
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(TorEditResponse {
        message: result,
        warning: None,
        applied: true,
    }))
}

#[derive(Serialize)]
struct TorPortDetail {
    virtual_port: u16,
    nginx_port: u16,
    backend_port: u16,
}

#[derive(Serialize)]
struct TorDetailResponse {
    name: String,
    hostname: String,
    active: bool,
    has_nginx: bool,
    has_ssl: bool,
    ports: Vec<TorPortDetail>,
}

async fn api_tor_detail(Path(name): Path<String>) -> ApiResult<TorDetailResponse> {
    use crate::adapters::tor::TorConfigAdapter;
    use crate::domain::naming::ServiceName;
    use crate::ports::tor::TorManagerPort;

    let tor_adapter = TorConfigAdapter::new();
    let services = tor_adapter
        .list_hidden_services()
        .await
        .map_err(|e| ApiError {
            error: format!("failed to list tor services: {}", e),
            code: 500,
        })?;

    let possible_names = ServiceName::possible_names_for_lookup(&name);
    let service = services
        .iter()
        .find(|s| possible_names.contains(&s.name))
        .ok_or_else(|| ApiError {
            error: format!("Service '{}' not found", name),
            code: 404,
        })?;

    let svc_name = &service.name;
    let hostname = service.hostname.clone();
    let active = service.active;

    // Detect if Nginx config exists
    let by_prefix = svc_name.starts_with("proxy_")
        || svc_name.starts_with("git_")
        || svc_name.starts_with("wp_")
        || svc_name.starts_with("ai_")
        || svc_name.starts_with("static_")
        || svc_name.starts_with("files_");

    let proxy_name = format!("proxy_{}", svc_name);
    let proxy_path = format!("/etc/nginx/sites-available/{}", proxy_name);
    let direct_path = format!("/etc/nginx/sites-available/{}", svc_name);

    let (has_nginx, nginx_name) = if by_prefix || std::path::Path::new(&proxy_path).exists() {
        (
            true,
            if by_prefix {
                svc_name.clone()
            } else {
                proxy_name
            },
        )
    } else if std::path::Path::new(&direct_path).exists() {
        (true, svc_name.clone())
    } else {
        (false, svc_name.clone())
    };

    let mut ports = Vec::new();

    for (virtual_port, target_str) in &service.ports {
        let nginx_port: u16 = target_str
            .split(':')
            .next_back()
            .and_then(|p| p.parse().ok())
            .unwrap_or(*virtual_port);

        let backend_port: u16 = if has_nginx {
            let nginx_config_path = format!("/etc/nginx/sites-available/{}", nginx_name);
            if let Ok(content) = tokio::fs::read_to_string(&nginx_config_path).await {
                content
                    .lines()
                    .find(|line| line.contains("proxy_pass"))
                    .and_then(|line| {
                        line.split(':')
                            .next_back()
                            .and_then(|s| s.trim_end_matches(';').trim().parse::<u16>().ok())
                    })
                    .unwrap_or(8080)
            } else {
                8080
            }
        } else {
            nginx_port
        };

        ports.push(TorPortDetail {
            virtual_port: *virtual_port,
            nginx_port,
            backend_port,
        });
    }

    // Check for SSL
    let has_ssl = if has_nginx {
        let nginx_config_path = format!("/etc/nginx/sites-available/{}", nginx_name);
        if let Ok(content) = tokio::fs::read_to_string(&nginx_config_path).await {
            content.contains("ssl_certificate")
        } else {
            false
        }
    } else {
        false
    };

    Ok(Json(TorDetailResponse {
        name: svc_name.clone(),
        hostname,
        active,
        has_nginx,
        has_ssl,
        ports,
    }))
}

// ── Console (universal CLI access) ────────────────────────────────────────────

#[derive(Deserialize)]
struct ConsoleRunRequest {
    args: Vec<String>,
    timeout_secs: Option<u64>,
}

#[derive(Serialize)]
struct ConsoleRunResponse {
    stdout: String,
    stderr: String,
    exit_code: i32,
}

#[derive(Serialize)]
struct ConsoleRunError {
    error: String,
    stdout: String,
    stderr: String,
    exit_code: i32,
}

impl axum::response::IntoResponse for ConsoleRunError {
    fn into_response(self) -> axum::response::Response {
        (axum::http::StatusCode::BAD_REQUEST, axum::Json(self)).into_response()
    }
}

#[derive(Serialize)]
struct ConsoleHelpResponse {
    commands: Vec<String>,
    help: String,
}

async fn api_console_help() -> ApiResult<ConsoleHelpResponse> {
    let commands = vec![
        "tor",
        "git",
        "wp",
        "drupal",
        "ghost",
        "magnolia",
        "strapi",
        "wagtail",
        "files",
        "vpn",
        "firewall",
        "apparmor",
        "ports",
        "setup",
        "doctor",
        "logs",
        "maintenance",
        "diag",
        "test",
        "quickref",
        "license",
        "uninstall",
        "config-show",
        "config-validate",
        "docs",
        "update",
        "verify",
    ]
    .into_iter()
    .map(String::from)
    .collect();
    let help = "Use /api/console/run with JSON: { args: [...], timeout_secs: 300 }.".to_string();
    Ok(Json(ConsoleHelpResponse { commands, help }))
}

async fn api_console_run(
    Json(req): Json<ConsoleRunRequest>,
) -> Result<Json<ConsoleRunResponse>, ConsoleRunError> {
    let binary = std::env::current_exe().map_err(|e| ConsoleRunError {
        error: format!("cannot locate binary: {}", e),
        stdout: String::new(),
        stderr: String::new(),
        exit_code: -1,
    })?;
    let timeout = std::time::Duration::from_secs(req.timeout_secs.unwrap_or(300));
    let output = tokio::time::timeout(
        timeout,
        tokio::process::Command::new(&binary)
            .args(&req.args)
            .output(),
    )
    .await
    .map_err(|_| ConsoleRunError {
        error: "command timed out".to_string(),
        stdout: String::new(),
        stderr: String::new(),
        exit_code: -1,
    })?
    .map_err(|e| ConsoleRunError {
        error: format!("failed to spawn command: {}", e),
        stdout: String::new(),
        stderr: String::new(),
        exit_code: -1,
    })?;
    let limit = 500_000;
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    let stdout = if stdout.len() > limit {
        stdout[..limit].to_string() + "\n[...truncated]"
    } else {
        stdout
    };
    let stderr = if stderr.len() > limit {
        stderr[..limit].to_string() + "\n[...truncated]"
    } else {
        stderr
    };
    let exit_code = output.status.code().unwrap_or(-1);
    if exit_code != 0 {
        Err(ConsoleRunError {
            error: format!("Command exited with code {}", exit_code),
            stdout,
            stderr,
            exit_code,
        })
    } else {
        Ok(Json(ConsoleRunResponse {
            stdout,
            stderr,
            exit_code,
        }))
    }
}

// ── Tor Auth ──────────────────────────────────────────────────────────────────

async fn api_tor_auth_list(Path(service): Path<String>) -> ApiResult<Vec<String>> {
    let clients = commands::tor::auth::list(&service)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(clients))
}

async fn api_tor_auth_enable(Path(service): Path<String>) -> ApiResult<String> {
    commands::tor::auth::enable(&service)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(format!(
        "✅ Client authorization enabled for '{}'",
        service
    )))
}

async fn api_tor_auth_disable(Path(service): Path<String>) -> ApiResult<String> {
    commands::tor::auth::disable(&service)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(format!(
        "✅ Client authorization disabled for '{}'",
        service
    )))
}

#[derive(Deserialize)]
struct TorAuthAddRequest {
    client: String,
    pubkey: String,
}

async fn api_tor_auth_add(
    Path(service): Path<String>,
    Json(req): Json<TorAuthAddRequest>,
) -> ApiResult<String> {
    commands::tor::auth::add(&service, &req.client, &req.pubkey)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(format!(
        "✅ Client '{}' added to service '{}'",
        req.client, service
    )))
}

#[derive(Deserialize)]
struct TorAuthRevokeRequest {
    client: String,
}

#[derive(Deserialize)]
struct TorAuthRotateRequest {
    client: Option<String>,
}

async fn api_tor_auth_revoke(
    Path(service): Path<String>,
    Json(req): Json<TorAuthRevokeRequest>,
) -> ApiResult<String> {
    commands::tor::auth::revoke(&service, &req.client)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(format!(
        "✅ Client '{}' revoked from service '{}'",
        req.client, service
    )))
}

#[derive(Deserialize)]
struct TorAuthGenerateRequest {
    client: String,
}

#[derive(Serialize)]
struct TorAuthGenerateResponse {
    public_key: String,
    private_key: String,
    message: String,
}

async fn api_tor_auth_generate(
    Json(req): Json<TorAuthGenerateRequest>,
) -> ApiResult<TorAuthGenerateResponse> {
    let (pubkey, privkey) = commands::tor::auth::generate(&req.client)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(TorAuthGenerateResponse {
        public_key: pubkey,
        private_key: privkey,
        message: format!(
            "🔐 Generated keypair for client '{}'. Send the PUBLIC key to the operator.",
            req.client
        ),
    }))
}

/// Rotate the x25519 keypair for a client on a Tor hidden service.
///
/// Generates a new keypair, revokes the old client entry (if any), and adds
/// the new public key. The revoke step is best-effort: if the client never
/// existed (e.g. first rotation), the error is silently ignored and the
/// new keypair is added anyway. This matches the semantics of "rotate" —
/// the caller wants fresh keys regardless of prior state.
///
/// `client` is optional in the request body. If omitted, defaults to
/// `"rotated-client"`. The frontend (`torAuthRotate`) does not send it.
async fn api_tor_auth_rotate(
    Path(service): Path<String>,
    Json(req): Json<TorAuthRotateRequest>,
) -> ApiResult<TorAuthGenerateResponse> {
    let client = req.client.unwrap_or_else(|| "rotated-client".to_string());
    let (pubkey, privkey) = commands::tor::auth::generate(&client)
        .await
        .map_err(ApiError::from)?;
    // Best-effort revoke: client may not exist yet (first rotation).
    // Silently ignore "not found" — the goal is to install fresh keys.
    let _ = commands::tor::auth::revoke(&service, &client).await;
    commands::tor::auth::add(&service, &client, &pubkey)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(TorAuthGenerateResponse {
        public_key: pubkey,
        private_key: privkey,
        message: format!(
            "🔄 Keypair rotated for client '{}' on service '{}'",
            client, service
        ),
    }))
}

// ── Git Complete ──────────────────────────────────────────────────────────────

async fn api_git_status(Path(name): Path<String>) -> ApiResult<commands::git::GitServerInfo> {
    let info = commands::git::status(&name).await.map_err(ApiError::from)?;
    Ok(Json(info))
}

#[derive(Deserialize)]
struct GitRegistrationRequest {
    enable: bool,
}

async fn api_git_registration(
    Path(name): Path<String>,
    Json(req): Json<GitRegistrationRequest>,
) -> ApiResult<()> {
    commands::git::registration(&name, req.enable)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_git_registration_status(Path(name): Path<String>) -> ApiResult<serde_json::Value> {
    let enabled = commands::git::registration_status(&name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "enabled": enabled })))
}

#[derive(Deserialize)]
struct GitEditRequest {
    http_port: Option<u16>,
    https_port: Option<u16>,
    ssh_port: Option<u16>,
    auto_ports: Option<bool>,
}

async fn api_git_edit(
    Path(name): Path<String>,
    Json(req): Json<GitEditRequest>,
) -> ApiResult<String> {
    let result = commands::git::edit(
        &name,
        req.http_port,
        req.https_port,
        req.ssh_port,
        req.auto_ports.unwrap_or(false),
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct GitUserListRequest {
    server: String,
    admin_user: Option<String>,
    admin_pass: Option<String>,
}

async fn api_git_user_list(
    Json(req): Json<GitUserListRequest>,
) -> ApiResult<Vec<commands::git::user::GitUserInfo>> {
    let result = commands::git::user::list(
        &req.server,
        req.admin_user.as_deref(),
        req.admin_pass.as_deref(),
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct GitUserCreateRequest {
    server: String,
    username: String,
    email: String,
    password: String,
    admin: Option<bool>,
    admin_user: Option<String>,
    admin_pass: Option<String>,
}

async fn api_git_user_create(Json(req): Json<GitUserCreateRequest>) -> ApiResult<()> {
    commands::git::user::create(
        &req.server,
        &req.username,
        &req.email,
        &req.password,
        req.admin.unwrap_or(false),
        req.admin_user.as_deref(),
        req.admin_pass.as_deref(),
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
struct GitUserDeleteRequest {
    server: String,
    username: String,
    admin_user: Option<String>,
    admin_pass: Option<String>,
}

async fn api_git_user_delete(Json(req): Json<GitUserDeleteRequest>) -> ApiResult<()> {
    commands::git::user::delete(
        &req.server,
        &req.username,
        req.admin_user.as_deref(),
        req.admin_pass.as_deref(),
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_git_watcher() -> ApiResult<String> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let args = vec!["git".to_string(), "watcher".to_string()];
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(&binary).args(&args).output(),
    )
    .await
    .map_err(|_| ApiError {
        error: "git watcher timed out (30s)".to_string(),
        code: 504,
    })?
    .map_err(|e| ApiError {
        error: format!("failed to spawn git watcher: {}", e),
        code: 500,
    })?;
    let text = strip_ansi(&format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ));
    Ok(Json(text))
}

// ── WordPress Complete ────────────────────────────────────────────────────────

async fn api_wp_restart(Path(name): Path<String>) -> ApiResult<()> {
    commands::wordpress::restart(&name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_wp_update(Path(name): Path<String>) -> ApiResult<()> {
    commands::wordpress::update(&name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_wp_config(Path(name): Path<String>) -> ApiResult<String> {
    let config = commands::wordpress::config(&name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(config))
}

async fn api_wp_status(
    Path(name): Path<String>,
) -> ApiResult<commands::wordpress::WordPressStatus> {
    let status = commands::wordpress::status(&name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(status))
}

#[derive(Deserialize)]
struct WpEditRequest {
    http_port: Option<u16>,
    https_port: Option<u16>,
    ssl: Option<bool>,
    auto_ports: Option<bool>,
}

async fn api_wp_edit(
    Path(name): Path<String>,
    Json(req): Json<WpEditRequest>,
) -> ApiResult<String> {
    let result = commands::wordpress::edit(
        &name,
        req.http_port,
        req.https_port,
        req.ssl,
        req.auto_ports.unwrap_or(false),
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(result))
}

// ── CMS Complete (Drupal, Ghost, Magnolia, Strapi, Wagtail) ───────────────────

fn build_cms_adapter(cms_type: &str) -> Result<Box<dyn crate::ports::cms::CmsLifecycle>, ApiError> {
    use crate::adapters::infra::docker::BollardDockerAdapter;
    use crate::adapters::infra::manifest::FileManifestAdapter;
    let docker = BollardDockerAdapter::new().map_err(|e| ApiError {
        error: format!("Docker unavailable: {}", e),
        code: 500,
    })?;
    let manifest = std::sync::Arc::new(FileManifestAdapter::new());
    let docker = std::sync::Arc::new(docker);
    match cms_type {
        "drupal" => Ok(
            Box::new(crate::adapters::cms::drupal::DrupalCmsAdapter::new(
                docker, manifest,
            )) as Box<dyn crate::ports::cms::CmsLifecycle>,
        ),
        "ghost" => Ok(Box::new(crate::adapters::cms::ghost::GhostCmsAdapter::new(
            docker, manifest,
        )) as Box<dyn crate::ports::cms::CmsLifecycle>),
        "magnolia" => Ok(
            Box::new(crate::adapters::cms::magnolia::MagnoliaCmsAdapter::new(
                docker, manifest,
            )) as Box<dyn crate::ports::cms::CmsLifecycle>,
        ),
        "strapi" => Ok(
            Box::new(crate::adapters::cms::strapi::StrapiCmsAdapter::new(
                docker, manifest,
            )) as Box<dyn crate::ports::cms::CmsLifecycle>,
        ),
        "wagtail" => Ok(
            Box::new(crate::adapters::cms::wagtail::WagtailCmsAdapter::new(
                docker, manifest,
            )) as Box<dyn crate::ports::cms::CmsLifecycle>,
        ),
        _ => Err(ApiError {
            error: format!("Unknown CMS type: {}", cms_type),
            code: 400,
        }),
    }
}

fn cms_prefix(cms_type: &str) -> &str {
    match cms_type {
        "drupal" => "drupal-",
        "ghost" => "ghost-",
        "magnolia" => "magnolia-",
        "strapi" => "strapi-",
        "wagtail" => "wagtail-",
        _ => "",
    }
}

async fn api_cms_list(Path(cms_type): Path<String>) -> ApiResult<Vec<serde_json::Value>> {
    let prefix = cms_prefix(&cms_type);
    let output = std::process::Command::new("docker")
        .args([
            "ps",
            "-a",
            "--filter",
            &format!("name={}", prefix),
            "--format",
            "{{.Names}}\t{{.Status}}\t{{.Ports}}",
        ])
        .output()
        .map_err(|e| ApiError {
            error: format!("docker ps failed: {}", e),
            code: 500,
        })?;
    let raw = String::from_utf8_lossy(&output.stdout);
    let mut sites = Vec::new();
    for line in raw.lines() {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() < 2 {
            continue;
        }
        let cname = parts[0];
        if !cname.starts_with(prefix) {
            continue;
        }
        let name = cname.strip_prefix(prefix).unwrap_or(cname).to_string();
        let status = if parts[1].contains("Up") {
            "running"
        } else {
            "stopped"
        };
        sites.push(serde_json::json!({
            "name": name,
            "container": cname,
            "status": status,
            "ports": parts.get(2).copied().unwrap_or(""),
        }));
    }
    Ok(Json(sites))
}

#[derive(Deserialize)]
struct CmsCreateRequest {
    name: String,
    http_port: Option<u16>,
}

async fn api_cms_create(
    Path(cms_type): Path<String>,
    Json(req): Json<CmsCreateRequest>,
) -> ApiResult<crate::domain::cms::CmsInstance> {
    let adapter = build_cms_adapter(&cms_type)?;
    let create_req = crate::domain::cms::CmsCreateRequest {
        name: req.name.clone(),
        http_port: req.http_port,
        db_password: None,
    };
    let inst = adapter.create(create_req).await.map_err(ApiError::from)?;
    Ok(Json(inst))
}

async fn api_cms_start(Path((cms_type, name)): Path<(String, String)>) -> ApiResult<()> {
    let adapter = build_cms_adapter(&cms_type)?;
    adapter.start(&name).await.map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_cms_stop(Path((cms_type, name)): Path<(String, String)>) -> ApiResult<()> {
    let adapter = build_cms_adapter(&cms_type)?;
    adapter.stop(&name).await.map_err(ApiError::from)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
struct CmsDeleteRequest {
    force: Option<bool>,
}

async fn api_cms_delete(
    Path((cms_type, name)): Path<(String, String)>,
    Json(req): Json<CmsDeleteRequest>,
) -> ApiResult<()> {
    let adapter = build_cms_adapter(&cms_type)?;
    adapter
        .delete(&name, req.force.unwrap_or(false))
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_cms_status(
    Path((cms_type, name)): Path<(String, String)>,
) -> ApiResult<crate::domain::cms::CmsInstance> {
    let adapter = build_cms_adapter(&cms_type)?;
    let inst = adapter.status(&name).await.map_err(ApiError::from)?;
    Ok(Json(inst))
}

#[derive(Deserialize)]
struct CmsEditRequest {
    http_port: Option<u16>,
}

async fn api_cms_edit(
    Path((cms_type, name)): Path<(String, String)>,
    Json(req): Json<CmsEditRequest>,
) -> ApiResult<String> {
    match cms_type.as_str() {
        "drupal" => {
            let result = commands::drupal::edit(&name, req.http_port)
                .await
                .map_err(ApiError::from)?;
            Ok(Json(result))
        }
        "ghost" => {
            let result = commands::ghost::edit(&name, req.http_port)
                .await
                .map_err(ApiError::from)?;
            Ok(Json(result))
        }
        _ => Err(ApiError {
            error: format!("Edit not supported for CMS type: {}", cms_type),
            code: 400,
        }),
    }
}

#[derive(Deserialize)]
struct StrapiBuildImageRequest {
    force: Option<bool>,
}

async fn api_strapi_build_image(Json(req): Json<StrapiBuildImageRequest>) -> ApiResult<String> {
    use crate::adapters::infra::docker::BollardDockerAdapter;
    use crate::infrastructure::embedded_scripts;
    use crate::ports::container::ContainerPort;
    use crate::ports::container::ImageBuildConfig;

    let tag = embedded_scripts::STRAPI_IMAGE_TAG;
    let docker = BollardDockerAdapter::new().map_err(|e| ApiError {
        error: format!("Docker unavailable: {}", e),
        code: 500,
    })?;

    if !req.force.unwrap_or(false) && docker.image_exists(tag).await.map_err(ApiError::from)? {
        return Ok(Json(format!(
            "✅ Strapi image '{}' already exists. Use --force to rebuild.",
            tag
        )));
    }

    let context_path = embedded_scripts::ensure_strapi_context().map_err(|e| ApiError {
        error: format!("Failed to prepare Strapi build context: {}", e),
        code: 500,
    })?;
    let dockerfile_path = context_path.join(embedded_scripts::STRAPI_DOCKERFILE_NAME);

    let build_config = ImageBuildConfig {
        dockerfile_path,
        context_path,
        tag: tag.to_string(),
        build_args: std::collections::HashMap::new(),
    };
    docker
        .build_image(build_config)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(format!(
        "✅ Strapi image '{}' built successfully.",
        tag
    )))
}

// ── Files Complete ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct FilesEditRequest {
    port: Option<u16>,
}

async fn api_files_edit(
    Path(name): Path<String>,
    Json(req): Json<FilesEditRequest>,
) -> ApiResult<String> {
    let result = commands::files::edit(&name, req.port)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn api_files_fix_perms(Path(name): Path<String>) -> ApiResult<()> {
    commands::files::fix_perms(&name)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

// ── Firewall Complete ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct FirewallSetupRequest {
    ssh_port: Option<u16>,
    #[allow(dead_code)]
    force: Option<bool>,
}

async fn api_firewall_setup(Json(req): Json<FirewallSetupRequest>) -> ApiResult<String> {
    use crate::adapters::infra::ufw::UfwAdapter;
    use crate::application::firewall_manager::FirewallManager;
    let mgr = FirewallManager::new(std::sync::Arc::new(UfwAdapter::new()));
    let ssh_port = req.ssh_port.unwrap_or(22);
    let result = mgr
        .setup_secure_defaults(ssh_port, &[])
        .map_err(|e| ApiError {
            error: format!("{}", e),
            code: 500,
        })?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct FirewallAllowRequest {
    port: u16,
    proto: String,
    from: Option<String>,
}

async fn api_firewall_allow(Json(req): Json<FirewallAllowRequest>) -> ApiResult<String> {
    use crate::adapters::infra::ufw::UfwAdapter;
    use crate::application::firewall_manager::FirewallManager;
    use crate::domain::firewall::FirewallProtocol;
    let mgr = FirewallManager::new(std::sync::Arc::new(UfwAdapter::new()));
    let proto: FirewallProtocol = req.proto.parse().map_err(|e: String| ApiError {
        error: e,
        code: 400,
    })?;
    let result = mgr
        .add_rule(req.port, proto, req.from)
        .map_err(|e| ApiError {
            error: format!("{}", e),
            code: 500,
        })?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct FirewallDenyRequest {
    port: u16,
    proto: String,
}

async fn api_firewall_deny(Json(req): Json<FirewallDenyRequest>) -> ApiResult<String> {
    use crate::adapters::infra::ufw::UfwAdapter;
    use crate::application::firewall_manager::FirewallManager;
    use crate::domain::firewall::FirewallProtocol;
    let mgr = FirewallManager::new(std::sync::Arc::new(UfwAdapter::new()));
    let proto: FirewallProtocol = req.proto.parse().map_err(|e: String| ApiError {
        error: e,
        code: 400,
    })?;
    let result = mgr.deny_rule(req.port, proto).map_err(|e| ApiError {
        error: format!("{}", e),
        code: 500,
    })?;
    Ok(Json(result))
}

// ── AppArmor Complete ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct AppArmorSetupRequest {
    mode: Option<String>,
    #[allow(dead_code)]
    force: Option<bool>,
}

async fn api_apparmor_setup(Json(req): Json<AppArmorSetupRequest>) -> ApiResult<String> {
    use crate::adapters::infra::apparmor::AppArmorAdapter;
    use crate::application::apparmor_manager::AppArmorManager;
    use crate::domain::apparmor::AppArmorMode;
    let mgr = AppArmorManager::new(std::sync::Arc::new(AppArmorAdapter::new()));
    let mode: AppArmorMode =
        req.mode
            .as_deref()
            .unwrap_or("enforce")
            .parse()
            .map_err(|e: String| ApiError {
                error: e,
                code: 400,
            })?;
    let result = mgr.setup_base_profiles(mode).map_err(|e| ApiError {
        error: format!("{}", e),
        code: 500,
    })?;
    Ok(Json(result))
}

// ── VPN Complete ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct VpnCreateRequestFull {
    interface: String,
    port: Option<u16>,
    subnet: Option<String>,
    autostart: Option<bool>,
    #[allow(dead_code)]
    sync_firewall: Option<bool>,
}

async fn api_vpn_create(Json(req): Json<VpnCreateRequestFull>) -> ApiResult<String> {
    use crate::adapters::infra::vpn::WireGuardAdapter;
    use crate::application::vpn_manager::VpnManager;
    let mgr = VpnManager::new(std::sync::Arc::new(WireGuardAdapter::new()));
    let result = mgr
        .create_vpn(
            &req.interface,
            req.port,
            req.subnet.as_deref(),
            req.autostart.unwrap_or(true),
        )
        .map_err(|e| ApiError {
            error: format!("{}", e),
            code: 500,
        })?;
    Ok(Json(result))
}

#[derive(Deserialize)]
struct VpnPeerAddRequest {
    interface: String,
    peer_name: String,
    endpoint: Option<String>,
    dns: Option<String>,
    psk: Option<bool>,
    ip: Option<String>,
}

async fn api_vpn_peer_add(Json(req): Json<VpnPeerAddRequest>) -> ApiResult<String> {
    use crate::adapters::infra::vpn::WireGuardAdapter;
    use crate::application::vpn_manager::VpnManager;
    let mgr = VpnManager::new(std::sync::Arc::new(WireGuardAdapter::new()));
    let status = mgr.get_status(&req.interface).map_err(|e| ApiError {
        error: format!("{}", e),
        code: 500,
    })?;
    let server_port = status.listen_port;
    let server_pub_key = status.public_key.clone();
    let peer_ip = if let Some(ip) = &req.ip {
        ip.clone()
    } else {
        let mut tmp_server =
            crate::domain::vpn::VpnServer::new(&req.interface, server_port, "10.8.0.0/24");
        for p in &status.peers {
            tmp_server.peers.push(crate::domain::vpn::VpnPeer::new(
                &p.public_key,
                &p.public_key,
                p.allowed_ips
                    .first()
                    .map(|s| s.trim_end_matches("/32"))
                    .unwrap_or("10.8.0.2"),
            ));
        }
        tmp_server.next_peer_ip().ok_or_else(|| ApiError {
            error: "VPN subnet is full".to_string(),
            code: 500,
        })?
    };
    let endpoint = req.endpoint.as_deref().unwrap_or("");
    let client_config = mgr
        .add_peer(
            &req.interface,
            &req.peer_name,
            endpoint,
            server_port,
            &server_pub_key,
            &peer_ip,
            req.psk.unwrap_or(false),
            req.dns.as_deref(),
        )
        .map_err(|e| ApiError {
            error: format!("{}", e),
            code: 500,
        })?;
    Ok(Json(format!(
        "✅ Peer '{}' added to VPN '{}' (IP: {})\n\n{}",
        req.peer_name, req.interface, peer_ip, client_config
    )))
}

#[derive(Deserialize)]
struct VpnPeerAddPubkeyRequest {
    interface: String,
    peer_name: String,
    public_key: String,
    ip: String,
}

async fn api_vpn_peer_add_pubkey(Json(req): Json<VpnPeerAddPubkeyRequest>) -> ApiResult<String> {
    use crate::adapters::infra::vpn::WireGuardAdapter;
    use crate::application::vpn_manager::VpnManager;
    let mgr = VpnManager::new(std::sync::Arc::new(WireGuardAdapter::new()));
    mgr.add_peer_by_pubkey(&req.interface, &req.peer_name, &req.public_key, &req.ip)
        .map_err(|e| ApiError {
            error: format!("{}", e),
            code: 500,
        })?;
    Ok(Json(format!(
        "✅ Peer '{}' (pubkey) added to VPN '{}' (IP: {})",
        req.peer_name, req.interface, req.ip
    )))
}

#[derive(Deserialize)]
struct VpnPeerRemoveRequest {
    interface: String,
    public_key: String,
}

async fn api_vpn_peer_remove(Json(req): Json<VpnPeerRemoveRequest>) -> ApiResult<String> {
    use crate::adapters::infra::vpn::WireGuardAdapter;
    use crate::application::vpn_manager::VpnManager;
    let mgr = VpnManager::new(std::sync::Arc::new(WireGuardAdapter::new()));
    mgr.remove_peer(&req.interface, &req.public_key)
        .map_err(|e| ApiError {
            error: format!("{}", e),
            code: 500,
        })?;
    Ok(Json(format!(
        "✅ Peer removed from VPN '{}'",
        req.interface
    )))
}

// ── Logs Complete ─────────────────────────────────────────────────────────────

async fn api_logs_install() -> ApiResult<Vec<String>> {
    let logs = commands::logs::view("install", 100, false)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(logs))
}

async fn api_logs_smoke_test() -> ApiResult<Vec<String>> {
    let logs = commands::logs::view("smoke-test", 100, false)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(logs))
}

// ── Maintenance ───────────────────────────────────────────────────────────────

async fn api_maintenance_status() -> ApiResult<String> {
    let status = commands::maintenance::status()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(strip_ansi(&status)))
}

async fn api_maintenance_smoke_test() -> ApiResult<String> {
    let result = commands::maintenance::smoke_test()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn api_maintenance_enable_checks() -> ApiResult<()> {
    commands::maintenance::enable_checks()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_maintenance_disable_checks() -> ApiResult<()> {
    commands::maintenance::disable_checks()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(()))
}

async fn api_maintenance_timer_status() -> ApiResult<String> {
    let status = commands::maintenance::timer_status()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(status))
}

async fn api_maintenance_ssh_config() -> ApiResult<commands::maintenance::SshConfigInfo> {
    let config = commands::maintenance::ssh_config()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(config))
}

#[derive(Deserialize)]
struct MaintenanceSshHardenPqcRequest {
    force: Option<bool>,
    dry_run: Option<bool>,
}

async fn api_maintenance_ssh_harden_pqc(
    Json(req): Json<MaintenanceSshHardenPqcRequest>,
) -> ApiResult<String> {
    let result = commands::maintenance::ssh_harden_pqc(
        req.force.unwrap_or(false),
        req.dry_run.unwrap_or(false),
    )
    .await
    .map_err(ApiError::from)?;
    Ok(Json(strip_ansi(&result)))
}

async fn api_maintenance_backup() -> ApiResult<String> {
    let result = commands::maintenance::backup()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(strip_ansi(&result)))
}

#[derive(Deserialize)]
struct MaintenanceCleanupRequest {
    target: String,
    dry_run: Option<bool>,
    force: Option<bool>,
    keep_days: Option<u32>,
}

async fn api_maintenance_cleanup(Json(req): Json<MaintenanceCleanupRequest>) -> ApiResult<String> {
    use crate::application::cleanup_service::CleanupService;
    let project_root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let dry_run = req.dry_run.unwrap_or(false);
    let service = if req.target == "docker" || req.target == "all" {
        use crate::adapters::infra::docker::BollardDockerAdapter;
        match BollardDockerAdapter::new() {
            Ok(docker) => CleanupService::new(project_root, dry_run)
                .with_container_manager(std::sync::Arc::new(docker)),
            Err(_) => CleanupService::new(project_root, dry_run),
        }
    } else {
        CleanupService::new(project_root, dry_run)
    };
    let result = service
        .cleanup(
            &req.target,
            req.keep_days.unwrap_or(30),
            req.force.unwrap_or(false),
        )
        .await
        .map_err(|e| ApiError {
            error: e.to_string(),
            code: 500,
        })?;
    Ok(Json(format!(
        "Cleanup completed: {} files, {} bytes freed",
        result.files_deleted, result.bytes_freed
    )))
}

// ── Diagnostics ───────────────────────────────────────────────────────────────

async fn api_diag_summary() -> ApiResult<String> {
    let status = commands::diagnostics::summary()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(strip_ansi(&status)))
}

async fn api_diag_nginx() -> ApiResult<String> {
    let status = commands::diagnostics::nginx()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(strip_ansi(&status)))
}

async fn api_diag_tor() -> ApiResult<Vec<crate::ports::tor::TorServiceInfo>> {
    let services = commands::diagnostics::tor().await.map_err(ApiError::from)?;
    Ok(Json(services))
}

async fn api_diag_ssh() -> ApiResult<String> {
    let status = commands::diagnostics::ssh().await.map_err(ApiError::from)?;
    Ok(Json(strip_ansi(&status)))
}

async fn api_diag_wordpress() -> ApiResult<commands::diagnostics::WordPressDiagnostics> {
    let status = commands::diagnostics::wordpress()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(status))
}

async fn api_diag_wp_sync() -> ApiResult<commands::diagnostics::WpSyncStatus> {
    let status = commands::diagnostics::wp_sync()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(status))
}

async fn api_diag_nginx_test() -> ApiResult<serde_json::Value> {
    let success = commands::diagnostics::nginx_test()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "valid": success })))
}

async fn api_diag_resources() -> ApiResult<String> {
    let status = commands::diagnostics::resources()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(strip_ansi(&status)))
}

// ── Test ──────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TestRunRequest {
    filter: Option<String>,
}

async fn api_test_run(Json(req): Json<TestRunRequest>) -> ApiResult<String> {
    let result = commands::test::run(req.filter.as_deref())
        .await
        .map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn api_test_list() -> ApiResult<Vec<String>> {
    let tests = commands::test::list().await.map_err(ApiError::from)?;
    Ok(Json(tests))
}

async fn api_test_benchmark() -> ApiResult<String> {
    let result = commands::test::benchmark().await.map_err(ApiError::from)?;
    Ok(Json(result))
}

async fn api_test_results() -> ApiResult<commands::test::TestResults> {
    let results = commands::test::results().await.map_err(ApiError::from)?;
    Ok(Json(results))
}

async fn api_test_clean() -> ApiResult<String> {
    let result = commands::test::clean().await.map_err(ApiError::from)?;
    Ok(Json(result))
}

// ── Setup ─────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct SetupRequest {
    all: Option<bool>,
    vpn: Option<bool>,
    security: Option<bool>,
    pqc_tls: Option<bool>,
}

async fn api_setup(Json(req): Json<SetupRequest>) -> ApiResult<String> {
    use crate::adapters::infra::dependencies::SystemDependencyAdapter;
    use crate::application::dependency_manager::DependencyManager;
    use crate::domain::dependencies::SetupScope;
    let scope = if req.all.unwrap_or(false) {
        SetupScope::All
    } else if req.vpn.unwrap_or(false) {
        SetupScope::Vpn
    } else if req.security.unwrap_or(false) {
        SetupScope::Security
    } else {
        SetupScope::Core
    };
    let adapter = std::sync::Arc::new(SystemDependencyAdapter::new());
    let mgr = DependencyManager::new(adapter);
    let result = mgr.setup(scope).map_err(|e| ApiError {
        error: format!("{}", e),
        code: 500,
    })?;
    let mut out = mgr.format_setup_result(&result);

    if req.pqc_tls.unwrap_or(false) {
        out.push_str(
            "\n\nPQC TLS stack installation started. Please check the SSE endpoint for progress.",
        );
    }

    Ok(Json(out))
}

async fn api_setup_pqc_tls_sse(
) -> Sse<impl futures::Stream<Item = Result<Event, std::convert::Infallible>>> {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};

    let (tx, rx) = futures::channel::mpsc::channel::<Result<Event, std::convert::Infallible>>(32);

    tokio::spawn(async move {
        use futures::SinkExt;
        let script_path = "/tmp/enola_install_pqc_tls_stack.sh";
        let mut tx = tx;

        if let Err(e) = tokio::fs::write(
            script_path,
            crate::infrastructure::pqc_tls::embedded_installer_script(),
        )
        .await
        {
            let _ = tx
                .send(Ok(
                    Event::default().data(format!("ERROR: Failed to write installer: {}", e))
                ))
                .await;
            return;
        }

        let _ = tx
            .send(Ok(
                Event::default().data("Writing PQC TLS installer script...")
            ))
            .await;

        let chmod = tokio::process::Command::new("chmod")
            .args(["700", script_path])
            .status()
            .await;
        match chmod {
            Ok(s) if s.success() => {}
            Ok(_) => {
                let _ = tx
                    .send(Ok(Event::default().data("ERROR: Failed to chmod installer")))
                    .await;
                return;
            }
            Err(e) => {
                let _ = tx
                    .send(Ok(
                        Event::default().data(format!("ERROR: chmod failed: {}", e))
                    ))
                    .await;
                return;
            }
        }

        let _ = tx
            .send(Ok(Event::default().data(
                "Starting PQC TLS installer (compiles OpenSSL from source, may take 10-30 min)...",
            )))
            .await;

        let mut child = match tokio::process::Command::new("sudo")
            .arg(script_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = tx
                    .send(Ok(
                        Event::default().data(format!("ERROR: Failed to start installer: {}", e))
                    ))
                    .await;
                return;
            }
        };

        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();
        let mut stdout_lines = BufReader::new(stdout).lines();
        let mut stderr_lines = BufReader::new(stderr).lines();

        let timeout_dur = std::time::Duration::from_secs(1800);
        let deadline = tokio::time::Instant::now() + timeout_dur;

        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => {
                    let _ = child.kill().await;
                    let _ = tx.send(Ok(Event::default().data("TIMEOUT: Installer exceeded 30 minutes. The installer compiles OpenSSL from source and may take longer on slow hardware."))).await;
                    let _ = tx.send(Ok(Event::default().data("SOLUTION: Run 'enola-cli setup --pqc-tls' directly in a terminal where it won't time out."))).await;
                    return;
                }
                line = stdout_lines.next_line() => {
                    match line {
                        Ok(Some(text)) => { let _ = tx.send(Ok(Event::default().data(text))).await; }
                        Ok(None) => break,
                        Err(e) => { let _ = tx.send(Ok(Event::default().data(format!("[read error: {}]", e)))).await; }
                    }
                }
                line = stderr_lines.next_line() => {
                    match line {
                        Ok(Some(text)) => { let _ = tx.send(Ok(Event::default().data(format!("[stderr] {}", text)))).await; }
                        Ok(None) => {}
                        Err(e) => { let _ = tx.send(Ok(Event::default().data(format!("[stderr read error: {}]", e)))).await; }
                    }
                }
            }
        }

        let status = child.wait().await;
        match status {
            Ok(s) if s.success() => {
                let _ = tx
                    .send(Ok(
                        Event::default().data("SUCCESS: PQC TLS stack installed.")
                    ))
                    .await;
            }
            Ok(s) => {
                let _ = tx
                    .send(Ok(
                        Event::default().data(format!("FAILED: Installer exited with {}", s))
                    ))
                    .await;
                let _ = tx.send(Ok(Event::default().data("SOLUTION: Check the output above for errors. You can retry with 'enola-cli setup --pqc-tls' in a terminal."))).await;
            }
            Err(e) => {
                let _ = tx
                    .send(Ok(Event::default()
                        .data(format!("ERROR: Failed to wait for installer: {}", e))))
                    .await;
            }
        }
    });

    Sse::new(rx).keep_alive(KeepAlive::default())
}

// ── Console Help Per Command ──────────────────────────────────────────────────

async fn api_console_help_command(Path(command): Path<String>) -> ApiResult<serde_json::Value> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new(&binary)
            .args([&command, "--help"])
            .output(),
    )
    .await
    .map_err(|_| ApiError {
        error: "help command timed out".to_string(),
        code: 504,
    })?
    .map_err(|e| ApiError {
        error: format!("failed to spawn help: {}", e),
        code: 500,
    })?;
    let help_text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(Json(serde_json::json!({
        "command": command,
        "help": help_text,
    })))
}

// ── Quickref ──────────────────────────────────────────────────────────────────

async fn api_quickref() -> ApiResult<String> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let output = tokio::process::Command::new(&binary)
        .arg("quickref")
        .output()
        .await
        .map_err(|e| ApiError {
            error: format!("failed to run quickref: {}", e),
            code: 500,
        })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(Json(strip_ansi(&text)))
}

// ── License ───────────────────────────────────────────────────────────────────

async fn api_license() -> ApiResult<String> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let output = tokio::process::Command::new(&binary)
        .arg("license")
        .output()
        .await
        .map_err(|e| ApiError {
            error: format!("failed to run license: {}", e),
            code: 500,
        })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(Json(strip_ansi(&text)))
}

// ── Config Show ───────────────────────────────────────────────────────────────

async fn api_config_show() -> ApiResult<String> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let output = tokio::process::Command::new(&binary)
        .args(["config-show", "--json"])
        .output()
        .await
        .map_err(|e| ApiError {
            error: format!("failed to run config-show: {}", e),
            code: 500,
        })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(Json(strip_ansi(&text)))
}

// ── Config Validate ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct ConfigValidateRequest {
    reachable: Option<bool>,
}

async fn api_config_validate(Json(req): Json<ConfigValidateRequest>) -> ApiResult<String> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let mut args = vec!["config-validate".to_string(), "--json".to_string()];
    if req.reachable.unwrap_or(false) {
        args.push("--reachable".to_string());
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(&binary).args(&args).output(),
    )
    .await
    .map_err(|_| ApiError {
        error: "config validate timed out".to_string(),
        code: 504,
    })?
    .map_err(|e| ApiError {
        error: format!("failed to spawn config-validate: {}", e),
        code: 500,
    })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(Json(strip_ansi(&text)))
}

// ── Verify (PQC release verification) ─────────────────────────────────────────

#[derive(Deserialize)]
struct VerifyRequest {
    file: String,
    pqsig: Option<String>,
    pubkey: Option<String>,
}

async fn api_verify(Json(req): Json<VerifyRequest>) -> ApiResult<serde_json::Value> {
    let report = crate::application::release_verify::run(
        &req.file,
        req.pqsig.as_deref(),
        req.pubkey.as_deref(),
    );
    Ok(Json(report.json_value()))
}

// ── Uninstall ─────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct UninstallRequest {
    yes: Option<bool>,
    keep_data: Option<bool>,
    only: Option<String>,
    force: Option<bool>,
    remove_deps: Option<bool>,
}

async fn api_uninstall(Json(req): Json<UninstallRequest>) -> ApiResult<ConsoleRunResponse> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let mut args = vec!["uninstall".to_string()];
    if req.yes.unwrap_or(false) {
        args.push("--yes".to_string());
    }
    if req.keep_data.unwrap_or(false) {
        args.push("--keep-data".to_string());
    }
    if let Some(only) = req.only {
        args.push("--only".to_string());
        args.push(only);
    }
    if req.force.unwrap_or(false) {
        args.push("--force".to_string());
    }
    if req.remove_deps.unwrap_or(false) {
        args.push("--remove-deps".to_string());
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        tokio::process::Command::new(&binary).args(&args).output(),
    )
    .await
    .map_err(|_| ApiError {
        error: "uninstall timed out".to_string(),
        code: 504,
    })?
    .map_err(|e| ApiError {
        error: format!("failed to spawn uninstall: {}", e),
        code: 500,
    })?;
    let stdout = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    let stderr = strip_ansi(&String::from_utf8_lossy(&output.stderr));
    Ok(Json(ConsoleRunResponse {
        stdout: stdout.to_owned(),
        stderr: stderr.to_owned(),
        exit_code: output.status.code().unwrap_or(-1),
    }))
}

// ── Docs ──────────────────────────────────────────────────────────────────────

async fn api_docs(Path(topic): Path<String>) -> ApiResult<String> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let parts: Vec<&str> = topic.splitn(2, '/').collect();
    let mut args = vec!["docs".to_string()];
    for p in &parts {
        args.push(p.to_string());
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::process::Command::new(&binary).args(&args).output(),
    )
    .await
    .map_err(|_| ApiError {
        error: "docs command timed out".to_string(),
        code: 504,
    })?
    .map_err(|e| ApiError {
        error: format!("failed to spawn docs: {}", e),
        code: 500,
    })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(Json(strip_ansi(&text)))
}

// ── Update ────────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct UpdateCheckRequest {
    force: Option<bool>,
}

async fn api_update_check(Json(req): Json<UpdateCheckRequest>) -> ApiResult<String> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let mut args = vec![
        "update".to_string(),
        "check".to_string(),
        "--json".to_string(),
    ];
    if req.force.unwrap_or(false) {
        args.push("--force".to_string());
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(&binary).args(&args).output(),
    )
    .await
    .map_err(|_| ApiError {
        error: "update check timed out".to_string(),
        code: 504,
    })?
    .map_err(|e| ApiError {
        error: format!("failed to spawn update check: {}", e),
        code: 500,
    })?;
    let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    Ok(Json(text))
}

async fn api_update_schema() -> ApiResult<String> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let output = tokio::process::Command::new(&binary)
        .args(["update", "schema", "--json"])
        .output()
        .await
        .map_err(|e| ApiError {
            error: format!("failed to spawn update schema: {}", e),
            code: 500,
        })?;
    let text = strip_ansi(&String::from_utf8_lossy(&output.stdout));
    Ok(Json(text))
}

#[derive(Deserialize)]
struct UpdateDownloadRequest {
    yes: Option<bool>,
    dry_run: Option<bool>,
    force: Option<bool>,
    allow_unsigned: Option<bool>,
}

async fn api_update_download(Json(req): Json<UpdateDownloadRequest>) -> ApiResult<String> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let mut args = vec![
        "update".to_string(),
        "download".to_string(),
        "--json".to_string(),
    ];
    if req.yes.unwrap_or(false) {
        args.push("--yes".to_string());
    }
    if req.dry_run.unwrap_or(false) {
        args.push("--dry-run".to_string());
    }
    if req.force.unwrap_or(false) {
        args.push("--force".to_string());
    }
    if req.allow_unsigned.unwrap_or(false) {
        args.push("--allow-unsigned".to_string());
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        tokio::process::Command::new(&binary).args(&args).output(),
    )
    .await
    .map_err(|_| ApiError {
        error: "update download timed out".to_string(),
        code: 504,
    })?
    .map_err(|e| ApiError {
        error: format!("failed to spawn update download: {}", e),
        code: 500,
    })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(Json(strip_ansi(&text)))
}

#[derive(Deserialize)]
struct UpdateApplyRequest {
    binary: Option<String>,
    allow_unsigned: Option<bool>,
}

async fn api_update_apply(Json(req): Json<UpdateApplyRequest>) -> ApiResult<String> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let mut args = vec![
        "update".to_string(),
        "apply".to_string(),
        "--json".to_string(),
    ];
    if let Some(b) = req.binary {
        args.push("--binary".to_string());
        args.push(b);
    }
    if req.allow_unsigned.unwrap_or(false) {
        args.push("--allow-unsigned".to_string());
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new(&binary).args(&args).output(),
    )
    .await
    .map_err(|_| ApiError {
        error: "update apply timed out".to_string(),
        code: 504,
    })?
    .map_err(|e| ApiError {
        error: format!("failed to spawn update apply: {}", e),
        code: 500,
    })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(Json(text))
}

#[derive(Deserialize)]
struct UpdateVerifyFeedRequest {
    source: String,
    signature: Option<String>,
}

async fn api_update_verify_feed(Json(req): Json<UpdateVerifyFeedRequest>) -> ApiResult<String> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let mut args = vec![
        "update".to_string(),
        "verify-feed".to_string(),
        req.source.clone(),
        "--json".to_string(),
    ];
    if let Some(sig) = &req.signature {
        args.push("--signature".to_string());
        args.push(sig.clone());
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new(&binary).args(&args).output(),
    )
    .await
    .map_err(|_| ApiError {
        error: "verify-feed timed out".to_string(),
        code: 504,
    })?
    .map_err(|e| ApiError {
        error: format!("failed to spawn verify-feed: {}", e),
        code: 500,
    })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(Json(strip_ansi(&text)))
}

// ── Doctor Security ───────────────────────────────────────────────────────────

async fn api_doctor_security() -> ApiResult<String> {
    let binary = std::env::current_exe().map_err(|e| ApiError {
        error: format!("cannot locate binary: {}", e),
        code: 500,
    })?;
    let output = tokio::process::Command::new(&binary)
        .args(["doctor", "--security"])
        .output()
        .await
        .map_err(|e| ApiError {
            error: format!("failed to spawn doctor: {}", e),
            code: 500,
        })?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(Json(text))
}
