use crate::domain::error::{EnolaError, Result};
use crate::ports::container::{
    ContainerConfig, ContainerInfo, ContainerPort, ContainerStats, ImageBuildConfig,
};
use bollard::container::{
    Config, CreateContainerOptions, InspectContainerOptions, ListContainersOptions, LogsOptions,
    RemoveContainerOptions, RestartContainerOptions, StartContainerOptions, StopContainerOptions,
    WaitContainerOptions,
};
use bollard::exec::CreateExecOptions;
use bollard::image::{BuildImageOptions, CreateImageOptions};
use bollard::models::DeviceRequest;
use bollard::network::CreateNetworkOptions;
use bollard::service::{HostConfig, PortBinding, RestartPolicy, RestartPolicyNameEnum};
use bollard::Docker;
use futures::StreamExt;
use std::collections::HashMap;

pub struct BollardDockerAdapter {
    docker: Docker,
}

type ExposedPorts = HashMap<String, HashMap<(), ()>>;
type PortBindings = HashMap<String, Option<Vec<PortBinding>>>;

impl BollardDockerAdapter {
    pub fn new() -> Result<Self> {
        let docker = Docker::connect_with_local_defaults().map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to connect to Docker: {}", e))
        })?;
        Ok(Self { docker })
    }

    /// Pull an image if it doesn't exist locally.
    /// For locally-built images (no registry prefix), skip pull and error if missing.
    async fn ensure_image(&self, image: &str) -> Result<()> {
        // Check if image exists locally
        if self.docker.inspect_image(image).await.is_ok() {
            return Ok(());
        }

        // Determine if this is a registry image or a locally-built one.
        // Official Docker Hub images (mariadb, ghost, mysql, postgres, etc.)
        // do NOT contain '/' but are still pullable from Docker Hub.
        // Only images prefixed with "enola/" or "enola-" are locally-built.
        let is_local_image = image.starts_with("enola/") || image.starts_with("enola-");

        if is_local_image {
            return Err(EnolaError::NotFound(format!(
                "Image '{}' not found locally. It may need to be built first.",
                image
            )));
        }

        // Pull the image from registry
        let options = Some(CreateImageOptions {
            from_image: image,
            ..Default::default()
        });

        let mut stream = self.docker.create_image(options, None, None);

        while let Some(result) = stream.next().await {
            if let Err(e) = result {
                return Err(EnolaError::InfrastructureError(format!(
                    "Failed to pull image {}: {}",
                    image, e
                )));
            }
        }

        Ok(())
    }

    fn build_localhost_port_maps(ports: &HashMap<u16, u16>) -> (ExposedPorts, PortBindings) {
        let mut exposed_ports = HashMap::new();
        let mut port_bindings = HashMap::new();
        for (host_port, container_port) in ports {
            let container_port_str = format!("{}/tcp", container_port);
            exposed_ports.insert(container_port_str.clone(), HashMap::new());
            port_bindings.insert(
                container_port_str,
                Some(vec![PortBinding {
                    // SEC-EXT-DOCKER-042: never bind service ports to 0.0.0.0.
                    host_ip: Some("127.0.0.1".to_string()),
                    host_port: Some(host_port.to_string()),
                }]),
            );
        }
        (exposed_ports, port_bindings)
    }

    fn effective_security_opt(user_opts: Vec<String>) -> Option<Vec<String>> {
        Some(crate::infrastructure::security_opt::build_default_security_opt(user_opts))
    }
}

#[async_trait::async_trait]
impl ContainerPort for BollardDockerAdapter {
    async fn list_containers(&self, all: bool) -> Result<Vec<ContainerInfo>> {
        let options = ListContainersOptions::<String> {
            all,
            ..Default::default()
        };

        let containers = self
            .docker
            .list_containers(Some(options))
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Docker list failed: {}", e)))?;

        let result = containers
            .into_iter()
            .map(|c| ContainerInfo {
                id: c.id.unwrap_or_default(),
                name: c
                    .names
                    .and_then(|n| n.first().cloned())
                    .unwrap_or_else(|| "unknown".to_string())
                    // Docker names often start with /, removing it for cleaner UI
                    .trim_start_matches('/')
                    .to_string(),
                image: c.image.unwrap_or_default(),
                status: c.status.unwrap_or_default(),
                ports: c
                    .ports
                    .map(|ports| {
                        ports
                            .into_iter()
                            .map(|p| {
                                let proto = match &p.typ {
                                    Some(bollard::models::PortTypeEnum::TCP) => "tcp",
                                    Some(bollard::models::PortTypeEnum::UDP) => "udp",
                                    Some(bollard::models::PortTypeEnum::SCTP) => "sctp",
                                    _ => "tcp",
                                };
                                match (p.ip.as_deref(), p.public_port) {
                                    (Some(ip), Some(pub_port)) => {
                                        format!("{}:{}->{}/{}", ip, pub_port, p.private_port, proto)
                                    }
                                    (None, Some(pub_port)) => {
                                        format!("{}->{}/{}", pub_port, p.private_port, proto)
                                    }
                                    _ => {
                                        format!("{}/{}", p.private_port, proto)
                                    }
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            })
            .collect();

        Ok(result)
    }

    async fn create_container(&self, mut config: ContainerConfig) -> Result<String> {
        // SEC-EXT-DOCKER-040: aplicar limites CPU/RAM/PIDs por defecto si el caller
        // no los especifico. Operador puede sobreescribir con env vars
        // ENOLA_DOCKER_DEFAULT_{MEMORY_BYTES,NANO_CPUS,PIDS}.
        crate::infrastructure::container_limits::apply_default_limits(&mut config);

        // SEC-013: Use image@sha256:... if digest is provided
        let effective_image = match &config.image_digest {
            Some(digest) => format!("{}@{}", config.image, digest),
            None => config.image.clone(),
        };

        // Ensure image exists (pull if needed)
        self.ensure_image(&effective_image).await?;

        let options = Some(CreateContainerOptions {
            name: config.name.clone(),
            platform: None,
        });
        let (exposed_ports, port_bindings) = Self::build_localhost_port_maps(&config.ports);
        let mut binds = Vec::new();
        for (host_path, container_path) in config.volumes {
            binds.push(format!("{}:{}", host_path, container_path));
        }
        // SEC-005: mount secret files as read-only at /run/secrets/{name}
        for (secret_name, host_path) in &config.secrets {
            binds.push(format!("{}:/run/secrets/{}:ro", host_path, secret_name));
        }
        let env: Vec<String> = config
            .env
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        let device_requests = if config.gpu_support {
            Some(vec![DeviceRequest {
                driver: Some("nvidia".to_string()),
                count: Some(-1),
                capabilities: Some(vec![vec![
                    "gpu".to_string(),
                    "compute".to_string(),
                    "utility".to_string(),
                ]]),
                ..Default::default()
            }])
        } else {
            None
        };
        // AA-002 + SEC-007: AppArmor / security-opt
        // Centralised in infrastructure::security_opt — always adds `no-new-privileges:true`
        // and emits an info log if AppArmor is enabled in the kernel but no profile was
        // provided. Silently degrades on WSL2 (§13.27).
        let security_opt = Self::effective_security_opt(config.security_opt);

        // DK-002: Resource limits
        let memory = config.memory_limit;
        let nano_cpus = config.nano_cpus;
        let pids_limit = config.pids_limit;

        // SEC-019: read-only root filesystem
        let read_only_rootfs = config.read_only_rootfs;

        // SEC-012: capabilities to drop
        let cap_drop = if config.cap_drop.is_empty() {
            None
        } else {
            Some(config.cap_drop.clone())
        };

        // SEC-012: capabilities to add back (e.g. SETUID, SETGID for su-exec)
        let cap_add = if config.cap_add.is_empty() {
            None
        } else {
            Some(config.cap_add.clone())
        };

        let host_config = HostConfig {
            binds: Some(binds),
            port_bindings: Some(port_bindings),
            network_mode: config.network,
            restart_policy: config.restart_policy.map(|p| RestartPolicy {
                name: Some(match p.as_str() {
                    "always" => RestartPolicyNameEnum::ALWAYS,
                    "unless-stopped" => RestartPolicyNameEnum::UNLESS_STOPPED,
                    "on-failure" => RestartPolicyNameEnum::ON_FAILURE,
                    _ => RestartPolicyNameEnum::NO,
                }),
                maximum_retry_count: None,
            }),
            device_requests,
            security_opt,
            memory,
            nano_cpus,
            pids_limit,
            readonly_rootfs: Some(read_only_rootfs),
            cap_drop,
            cap_add,
            ..Default::default()
        };
        let container_config = Config {
            image: Some(effective_image),
            exposed_ports: Some(exposed_ports),
            host_config: Some(host_config),
            env: Some(env),
            cmd: config.command,
            ..Default::default()
        };
        let container = self
            .docker
            .create_container(options, container_config)
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to create container: {}", e))
            })?;
        let id = container.id;
        self.docker
            .start_container(&id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to start container {}: {}", id, e))
            })?;
        Ok(id)
    }

    async fn start_container(&self, id: &str) -> Result<()> {
        self.docker
            .start_container(id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to start container {}: {}", id, e))
            })
    }

    async fn stop_container(&self, id: &str) -> Result<()> {
        self.docker
            .stop_container(id, None::<StopContainerOptions>)
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to stop container {}: {}", id, e))
            })
    }

    async fn remove_container(&self, id: &str) -> Result<()> {
        self.docker
            .stop_container(id, None::<StopContainerOptions>)
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to stop container {}: {}", id, e))
            })?;
        let options = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };
        self.docker
            .remove_container(id, Some(options))
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to remove container {}: {}", id, e))
            })
    }

    async fn restart_container(&self, id: &str) -> Result<()> {
        self.docker
            .restart_container(id, None::<RestartContainerOptions>)
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "Failed to restart container {}: {}",
                    id, e
                ))
            })
    }

    async fn get_logs(&self, id: &str, tail: usize) -> Result<String> {
        let options = LogsOptions::<String> {
            stdout: true,
            stderr: true,
            tail: tail.to_string(),
            ..Default::default()
        };

        let mut stream = self.docker.logs(id, Some(options));
        let mut output = String::new();

        while let Some(log_result) = stream.next().await {
            match log_result {
                Ok(log_output) => {
                    output.push_str(&log_output.to_string());
                }
                Err(e) => {
                    return Err(EnolaError::InfrastructureError(format!(
                        "Failed to get logs for {}: {}",
                        id, e
                    )));
                }
            }
        }

        Ok(output)
    }

    async fn execute_command(&self, id: &str, cmd: Vec<String>) -> Result<String> {
        let cmd_refs: Vec<&str> = cmd.iter().map(|s| s.as_str()).collect();
        let config = CreateExecOptions {
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            cmd: Some(cmd_refs),
            ..Default::default()
        };

        let exec = self.docker.create_exec(id, config).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to create exec for {}: {}", id, e))
        })?;

        let exec_result = self.docker.start_exec(&exec.id, None).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to start exec for {}: {}", id, e))
        })?;

        if let bollard::exec::StartExecResults::Attached { mut output, .. } = exec_result {
            let mut result_str = String::new();
            while let Some(msg) = output.next().await {
                if let Ok(msg) = msg {
                    result_str.push_str(&msg.to_string());
                }
            }
            Ok(result_str)
        } else {
            Err(EnolaError::InfrastructureError(
                "Exec failed to attach".to_string(),
            ))
        }
    }

    async fn inspect_container(&self, id: &str) -> Result<HashMap<String, String>> {
        let inspect = self
            .docker
            .inspect_container(id, None::<InspectContainerOptions>)
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to inspect {}: {}", id, e))
            })?;
        let mut info = HashMap::new();
        if let Some(state) = inspect.state {
            if let Some(status) = state.status {
                info.insert("status".to_string(), status.to_string());
            }
            info.insert(
                "started_at".to_string(),
                state.started_at.unwrap_or_default(),
            );
        }
        info.insert("name".to_string(), inspect.name.unwrap_or_default());

        Ok(info)
    }

    // Add network management methods if needed, or just helpers
    async fn create_network(&self, name: &str) -> Result<()> {
        let config = CreateNetworkOptions {
            name,
            check_duplicate: true,
            ..Default::default()
        };

        self.docker.create_network(config).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to create network {}: {}", name, e))
        })?;
        Ok(())
    }

    async fn remove_network(&self, name: &str) -> Result<()> {
        self.docker.remove_network(name).await.map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to remove network {}: {}", name, e))
        })?;
        Ok(())
    }

    async fn connect_container_to_network(&self, network: &str, container: &str) -> Result<()> {
        let config = bollard::network::ConnectNetworkOptions {
            container,
            ..Default::default()
        };
        self.docker
            .connect_network(network, config)
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!(
                    "Failed to connect {} to {}: {}",
                    container, network, e
                ))
            })?;
        Ok(())
    }

    async fn image_exists(&self, image: &str) -> Result<bool> {
        match self.docker.inspect_image(image).await {
            Ok(_) => Ok(true),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(false),
            Err(e) => Err(EnolaError::InfrastructureError(format!(
                "Failed to check image {}: {}",
                image, e
            ))),
        }
    }

    async fn build_image(&self, config: ImageBuildConfig) -> Result<String> {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        use tar::Builder;

        println!("🏗️  Construyendo imagen Docker: {}", config.tag);
        println!("    📁 Contexto: {:?}", config.context_path);
        println!("    📄 Dockerfile: {:?}", config.dockerfile_path);
        println!();

        // Create tar archive of the build context
        let context_path = &config.context_path;
        if !context_path.exists() {
            return Err(EnolaError::NotFound(format!(
                "Build context not found: {:?}",
                context_path
            )));
        }

        print!("    📦 Preparando contexto de build...");
        std::io::stdout().flush().ok();

        // Build tar archive in memory
        let mut tar_gz = GzEncoder::new(Vec::new(), Compression::default());
        {
            let mut tar_builder = Builder::new(&mut tar_gz);

            // Add all files from context directory
            if context_path.is_dir() {
                tar_builder.append_dir_all(".", context_path).map_err(|e| {
                    EnolaError::InfrastructureError(format!("Failed to create tar: {}", e))
                })?;
            }

            tar_builder.finish().map_err(|e| {
                EnolaError::InfrastructureError(format!("Failed to finalize tar: {}", e))
            })?;
        }

        let tar_bytes = tar_gz.finish().map_err(|e| {
            EnolaError::InfrastructureError(format!("Failed to compress tar: {}", e))
        })?;

        println!(" ✅ ({} bytes)", tar_bytes.len());

        // Determine dockerfile path relative to context
        let dockerfile = config
            .dockerfile_path
            .strip_prefix(&config.context_path)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| {
                config
                    .dockerfile_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Dockerfile".to_string())
            });

        let build_options = BuildImageOptions {
            dockerfile: dockerfile.as_str(),
            t: config.tag.as_str(),
            rm: true,
            ..Default::default()
        };

        println!("    🔨 Ejecutando docker build...");
        println!("    ─────────────────────────────────────────");

        let mut stream = self
            .docker
            .build_image(build_options, None, Some(tar_bytes.into()));

        let mut output = String::new();
        let mut step_count = 0;
        while let Some(result) = stream.next().await {
            match result {
                Ok(info) => {
                    if let Some(stream_msg) = info.stream {
                        output.push_str(&stream_msg);
                        // Print build progress with better formatting
                        let trimmed = stream_msg.trim();
                        if trimmed.starts_with("Step ") {
                            step_count += 1;
                            print!("\r    📌 {}", trimmed);
                            std::io::stdout().flush().ok();
                        } else if !trimmed.is_empty() && !trimmed.starts_with("---> ") {
                            // Show other meaningful messages
                            if trimmed.len() < 80 {
                                println!("       {}", trimmed);
                            }
                        }
                    }
                    if let Some(error) = info.error {
                        println!();
                        println!("    ❌ Error en build: {}", error);
                        return Err(EnolaError::InfrastructureError(format!(
                            "Docker build error: {}",
                            error
                        )));
                    }
                }
                Err(e) => {
                    println!();
                    println!("    ❌ Build falló: {}", e);
                    return Err(EnolaError::InfrastructureError(format!(
                        "Docker build failed: {}",
                        e
                    )));
                }
            }
        }

        println!();
        println!("    ─────────────────────────────────────────");
        println!(
            "    ✅ Imagen construida: {} ({} pasos)",
            config.tag, step_count
        );
        println!();

        Ok(config.tag)
    }

    async fn run_ephemeral_container(&self, mut config: ContainerConfig) -> Result<(i64, String)> {
        use std::io::Write;

        // SEC-EXT-DOCKER-040: aplicar limites CPU/RAM/PIDs por defecto.
        crate::infrastructure::container_limits::apply_default_limits(&mut config);

        println!("🐳 Preparando contenedor: {}", config.name);

        // SEC-013: Use image@sha256:... if digest is provided
        let effective_image = match &config.image_digest {
            Some(digest) => format!("{}@{}", config.image, digest),
            None => config.image.clone(),
        };

        // Ensure image exists (pull if needed)
        print!("    📥 Verificando imagen {}...", effective_image);
        std::io::stdout().flush().ok();
        self.ensure_image(&effective_image).await?;
        println!(" ✅");

        let container_name = config.name.clone();

        // Remove any existing container with the same name
        let _ = self.remove_container(&container_name).await;

        print!("    🔧 Creando contenedor...");
        std::io::stdout().flush().ok();

        let options = Some(CreateContainerOptions {
            name: container_name.clone(),
            platform: None,
        });

        let mut binds = Vec::new();
        for (host_path, container_path) in &config.volumes {
            binds.push(format!("{}:{}", host_path, container_path));
        }
        // SEC-005: mount secret files as read-only at /run/secrets/{name}
        for (secret_name, host_path) in &config.secrets {
            binds.push(format!("{}:/run/secrets/{}:ro", host_path, secret_name));
        }

        let env: Vec<String> = config
            .env
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();

        let device_requests = if config.gpu_support {
            Some(vec![DeviceRequest {
                driver: Some("nvidia".to_string()),
                count: Some(-1),
                capabilities: Some(vec![vec![
                    "gpu".to_string(),
                    "compute".to_string(),
                    "utility".to_string(),
                ]]),
                ..Default::default()
            }])
        } else {
            None
        };

        // AA-002 + SEC-007: AppArmor / security-opt (delegado al helper — se respeta config.security_opt).
        let security_opt = Self::effective_security_opt(std::mem::take(&mut config.security_opt));

        // SEC-019: read-only root filesystem
        let read_only_rootfs = config.read_only_rootfs;

        // SEC-012: capabilities to drop
        let cap_drop = if config.cap_drop.is_empty() {
            None
        } else {
            Some(config.cap_drop.clone())
        };

        // SEC-012: capabilities to add back
        let cap_add = if config.cap_add.is_empty() {
            None
        } else {
            Some(config.cap_add.clone())
        };

        let host_config = HostConfig {
            binds: Some(binds),
            network_mode: config.network.clone(),
            // If auto_remove is true and we don't need logs, Docker removes container automatically
            // If false, we handle removal manually after getting logs
            auto_remove: Some(config.auto_remove),
            device_requests,
            security_opt,
            memory: config.memory_limit,
            nano_cpus: config.nano_cpus,
            pids_limit: config.pids_limit,
            readonly_rootfs: Some(read_only_rootfs),
            cap_drop,
            cap_add,
            ..Default::default()
        };

        let container_config = Config {
            image: Some(effective_image),
            host_config: Some(host_config),
            env: Some(env),
            cmd: config.command,
            working_dir: config.working_dir,
            ..Default::default()
        };

        // Create container
        let container = self
            .docker
            .create_container(options, container_config)
            .await
            .map_err(|e| {
                println!(" ❌");
                EnolaError::InfrastructureError(format!("Failed to create container: {}", e))
            })?;

        let container_id = container.id.clone();
        println!(" ✅");

        // Start container
        print!("    🚀 Iniciando contenedor...");
        std::io::stdout().flush().ok();

        self.docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| {
                println!(" ❌");
                EnolaError::InfrastructureError(format!("Failed to start container: {}", e))
            })?;

        println!(" ✅");
        println!("    ⏳ Ejecutando (esto puede tomar varios minutos)...");
        println!();

        // Wait for container to finish
        let wait_options = WaitContainerOptions {
            condition: "not-running",
        };

        let start_time = std::time::Instant::now();
        let mut wait_stream = self
            .docker
            .wait_container(&container_id, Some(wait_options));
        let mut exit_code: i64 = -1;

        while let Some(result) = wait_stream.next().await {
            match result {
                Ok(wait_response) => {
                    exit_code = wait_response.status_code;
                }
                Err(e) => {
                    // Container might have been removed already
                    tracing::warn!("Wait container error: {}", e);
                }
            }
        }

        let duration = start_time.elapsed();
        let duration_str = if duration.as_secs() > 60 {
            format!("{}m {}s", duration.as_secs() / 60, duration.as_secs() % 60)
        } else {
            format!("{}s", duration.as_secs())
        };

        // Get logs before removal (if auto_remove is false, container still exists)
        print!("    📋 Recuperando logs...");
        std::io::stdout().flush().ok();
        let logs = self
            .get_logs(&container_id, 10000)
            .await
            .unwrap_or_default();
        println!(" ✅");

        // Remove container (cleanup) - only if auto_remove was false
        if !config.auto_remove {
            print!("    🧹 Limpiando contenedor...");
            std::io::stdout().flush().ok();
            let remove_options = RemoveContainerOptions {
                force: true,
                ..Default::default()
            };
            let _ = self
                .docker
                .remove_container(&container_id, Some(remove_options))
                .await;
            println!(" ✅");
        };

        println!();
        if exit_code == 0 {
            println!(
                "    ✅ Contenedor finalizado exitosamente (duración: {})",
                duration_str
            );
        } else {
            println!(
                "    ❌ Contenedor falló con código: {} (duración: {})",
                exit_code, duration_str
            );
        }

        Ok((exit_code, logs))
    }

    async fn prune_system(&self) -> Result<()> {
        use bollard::container::PruneContainersOptions;
        use bollard::image::PruneImagesOptions;

        // Prune stopped containers
        self.docker
            .prune_containers(None::<PruneContainersOptions<String>>)
            .await
            .map_err(|e| {
                EnolaError::InfrastructureError(format!("Container prune failed: {}", e))
            })?;

        // Prune dangling images
        self.docker
            .prune_images(None::<PruneImagesOptions<String>>)
            .await
            .map_err(|e| EnolaError::InfrastructureError(format!("Image prune failed: {}", e)))?;

        Ok(())
    }

    async fn pull_image(&self, image: &str) -> Result<()> {
        let options = Some(CreateImageOptions {
            from_image: image,
            ..Default::default()
        });

        let mut stream = self.docker.create_image(options, None, None);

        while let Some(result) = stream.next().await {
            if let Err(e) = result {
                return Err(EnolaError::InfrastructureError(format!(
                    "Failed to pull image {}: {}",
                    image, e
                )));
            }
        }

        Ok(())
    }

    async fn get_container_stats(&self, id: &str) -> Result<ContainerStats> {
        use bollard::container::StatsOptions;

        let mut stream = self.docker.stats(
            id,
            Some(StatsOptions {
                stream: false,
                one_shot: true,
            }),
        );

        match stream.next().await {
            Some(Ok(stats)) => {
                let cpu_percent = calculate_cpu_percent(&stats);
                let memory_usage = stats.memory_stats.usage.unwrap_or(0);
                let memory_limit = stats.memory_stats.limit.unwrap_or(0);

                Ok(ContainerStats {
                    cpu_percent,
                    memory_usage,
                    memory_limit,
                })
            }
            Some(Err(e)) => Err(EnolaError::InfrastructureError(format!(
                "Failed to get stats for container {}: {}",
                id, e
            ))),
            None => Err(EnolaError::InfrastructureError(format!(
                "No stats returned for container {}",
                id
            ))),
        }
    }
}

fn calculate_cpu_percent(stats: &bollard::container::Stats) -> f32 {
    let cpu_stats = &stats.cpu_stats;
    let precpu_stats = &stats.precpu_stats;

    let cpu_delta =
        cpu_stats.cpu_usage.total_usage as i64 - precpu_stats.cpu_usage.total_usage as i64;
    let system_delta = cpu_stats.system_cpu_usage.unwrap_or(0) as i64
        - precpu_stats.system_cpu_usage.unwrap_or(0) as i64;
    let online_cpus = cpu_stats.online_cpus.unwrap_or(1) as f32;

    if system_delta > 0 && cpu_delta >= 0 {
        (cpu_delta as f32 / system_delta as f32) * online_cpus * 100.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::container::ContainerPort;

    // ═══════════════════════════════════════════════════════════════
    // Unit tests (no Docker needed)
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn test_adapter_new_succeeds_with_docker() {
        // Should succeed if Docker daemon is accessible
        let result = BollardDockerAdapter::new();
        // In CI without Docker this would fail, so just assert it doesn't panic
        let _ = result;
    }

    #[test]
    fn localhost_port_maps_always_bind_127_0_0_1() {
        let mut ports = HashMap::new();
        ports.insert(8080, 80);
        ports.insert(8443, 443);
        let (_exposed, bindings) = BollardDockerAdapter::build_localhost_port_maps(&ports);

        assert_eq!(bindings.len(), 2);
        for entries in bindings.values() {
            let list = entries.as_ref().expect("binding list");
            for binding in list {
                assert_eq!(binding.host_ip.as_deref(), Some("127.0.0.1"));
            }
        }
    }

    #[test]
    fn effective_security_opt_always_contains_no_new_privileges() {
        let out = BollardDockerAdapter::effective_security_opt(vec![]).expect("opts present");
        assert!(out.iter().any(|v| v == "no-new-privileges:true"));
    }

    // ═══════════════════════════════════════════════════════════════
    // Integration tests (require Docker daemon)
    // Run with: cargo test docker_integration -- --ignored
    // ═══════════════════════════════════════════════════════════════

    #[tokio::test]
    #[ignore = "requires Docker daemon"]
    async fn docker_integration_connect() {
        let adapter = BollardDockerAdapter::new().expect("Docker should be available");
        // Just verify we can connect
        let containers = adapter.list_containers(true).await;
        assert!(containers.is_ok(), "Should be able to list containers");
    }

    #[tokio::test]
    #[ignore = "requires Docker daemon"]
    async fn docker_integration_list_containers() {
        let adapter = BollardDockerAdapter::new().expect("Docker should be available");
        let containers = adapter.list_containers(false).await.unwrap();
        // We don't know how many, just that it doesn't error
        let _ = containers.len();
    }

    #[tokio::test]
    #[ignore = "requires Docker daemon"]
    async fn docker_integration_image_exists() {
        let adapter = BollardDockerAdapter::new().expect("Docker should be available");

        // A very common image — if not present, it's fine to be false
        let exists = adapter.image_exists("hello-world:latest").await.unwrap();
        // Just verify the call works, result depends on local cache
        let _ = exists;
    }

    #[tokio::test]
    #[ignore = "requires Docker daemon"]
    async fn docker_integration_create_start_stop_remove() {
        let adapter = BollardDockerAdapter::new().expect("Docker should be available");
        let test_name = "enola-test-integration";

        // Cleanup from previous runs
        let _ = adapter.remove_container(test_name).await;

        // Create container with alpine (very small image)
        let config = ContainerConfig {
            name: test_name.to_string(),
            image: "alpine:latest".to_string(),
            command: Some(vec!["sleep".to_string(), "30".to_string()]),
            ..Default::default()
        };

        let id = adapter.create_container(config).await;
        assert!(id.is_ok(), "Should create container: {:?}", id.err());
        let container_id = id.unwrap();

        // Inspect
        let info = adapter.inspect_container(&container_id).await;
        assert!(info.is_ok(), "Should inspect container");

        // Stop
        let stop = adapter.stop_container(&container_id).await;
        assert!(stop.is_ok(), "Should stop container");

        // Remove
        let remove = adapter.remove_container(&container_id).await;
        assert!(remove.is_ok(), "Should remove container");
    }

    #[tokio::test]
    #[ignore = "requires Docker daemon"]
    async fn docker_integration_get_logs() {
        let adapter = BollardDockerAdapter::new().expect("Docker should be available");
        let test_name = "enola-test-logs";

        // Cleanup
        let _ = adapter.remove_container(test_name).await;

        // Run container that produces output
        let config = ContainerConfig {
            name: test_name.to_string(),
            image: "alpine:latest".to_string(),
            command: Some(vec!["echo".to_string(), "hello-enola".to_string()]),
            ..Default::default()
        };

        let id = adapter.create_container(config).await.unwrap();

        // Wait a moment for it to finish
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Get logs
        let logs = adapter.get_logs(&id, 10).await;
        assert!(logs.is_ok(), "Should get logs");
        let log_content = logs.unwrap();
        assert!(
            log_content.contains("hello-enola"),
            "Logs should contain output: {}",
            log_content
        );

        // Cleanup
        let _ = adapter.remove_container(&id).await;
    }

    #[tokio::test]
    #[ignore = "requires Docker daemon"]
    async fn docker_integration_execute_command() {
        let adapter = BollardDockerAdapter::new().expect("Docker should be available");
        let test_name = "enola-test-exec";

        // Cleanup
        let _ = adapter.remove_container(test_name).await;

        // Create a running container
        let config = ContainerConfig {
            name: test_name.to_string(),
            image: "alpine:latest".to_string(),
            command: Some(vec!["sleep".to_string(), "30".to_string()]),
            ..Default::default()
        };

        let id = adapter.create_container(config).await.unwrap();

        // Wait for it to be ready
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        // Execute command inside
        let result = adapter
            .execute_command(&id, vec!["echo".to_string(), "from-exec".to_string()])
            .await;
        assert!(result.is_ok(), "Should execute command: {:?}", result.err());
        assert!(result.unwrap().contains("from-exec"));

        // Cleanup
        let _ = adapter.remove_container(&id).await;
    }

    #[tokio::test]
    #[ignore = "requires Docker daemon"]
    async fn docker_integration_network_lifecycle() {
        let adapter = BollardDockerAdapter::new().expect("Docker should be available");
        let test_network = "enola-test-network";

        // Cleanup from previous runs — red puede existir por test anterior fallido
        let _ = tokio::process::Command::new("docker")
            .args(["network", "rm", test_network])
            .output()
            .await;

        // Create network
        let create = adapter.create_network(test_network).await;
        assert!(create.is_ok(), "Should create network");

        // Create a container and connect to network
        let test_name = "enola-test-net-container";
        let _ = adapter.remove_container(test_name).await;

        let config = ContainerConfig {
            name: test_name.to_string(),
            image: "alpine:latest".to_string(),
            command: Some(vec!["sleep".to_string(), "10".to_string()]),
            ..Default::default()
        };

        let id = adapter.create_container(config).await.unwrap();

        let connect = adapter
            .connect_container_to_network(test_network, &id)
            .await;
        assert!(connect.is_ok(), "Should connect container to network");

        // Cleanup
        let _ = adapter.remove_container(&id).await;
        // Remove network via Docker CLI (Bollard doesn't have remove_network in our port)
        let _ = tokio::process::Command::new("docker")
            .args(["network", "rm", test_network])
            .output()
            .await;
    }

    #[tokio::test]
    #[ignore = "requires Docker daemon"]
    async fn docker_integration_inspect_nonexistent() {
        let adapter = BollardDockerAdapter::new().expect("Docker should be available");
        let result = adapter
            .inspect_container("nonexistent-container-12345")
            .await;
        assert!(result.is_err(), "Should error on nonexistent container");
    }

    #[tokio::test]
    #[ignore = "requires Docker daemon"]
    async fn docker_integration_prune_system() {
        let adapter = BollardDockerAdapter::new().expect("Docker should be available");
        let result = adapter.prune_system().await;
        assert!(result.is_ok(), "Prune should succeed");
    }
}
