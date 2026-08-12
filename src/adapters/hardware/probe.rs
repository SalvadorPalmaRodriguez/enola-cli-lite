use crate::ports::hardware::{
    BatteryState, GpuBrand, GpuInfo, HardwareProbePort, SystemHardwareSpecs,
};
use async_trait::async_trait;
use nvml_wrapper::Nvml;
use std::sync::Arc;
use sysinfo::System;
use tokio::sync::Mutex;

pub struct EnolaHardwareProbe {
    system: Arc<Mutex<System>>,
}

impl Default for EnolaHardwareProbe {
    fn default() -> Self {
        Self::new()
    }
}

impl EnolaHardwareProbe {
    pub fn new() -> Self {
        Self {
            system: Arc::new(Mutex::new(System::new_all())),
        }
    }

    fn check_nvidia_gpu() -> Vec<GpuInfo> {
        let mut gpus = Vec::new();

        // 1. Intentar inicializar NVML (API Nativa)
        let nvml_result = Nvml::init();
        match nvml_result {
            Ok(nvml) => match nvml.device_count() {
                Ok(count) => {
                    for i in 0..count {
                        if let Ok(device) = nvml.device_by_index(i) {
                            let name = device
                                .name()
                                .unwrap_or_else(|_| "Unknown NVIDIA GPU".to_string());
                            let memory = device.memory_info().ok();
                            let temp = device
                                .temperature(
                                    nvml_wrapper::enum_wrappers::device::TemperatureSensor::Gpu,
                                )
                                .ok();
                            let util = device.utilization_rates().ok();

                            let total_mem =
                                memory.as_ref().map(|m| m.total / 1024 / 1024).unwrap_or(0);
                            let used_mem =
                                memory.as_ref().map(|m| m.used / 1024 / 1024).unwrap_or(0);
                            let free_mem =
                                memory.as_ref().map(|m| m.free / 1024 / 1024).unwrap_or(0);

                            gpus.push(GpuInfo {
                                index: i,
                                name,
                                brand: GpuBrand::Nvidia,
                                vram_total_mb: total_mem,
                                vram_used_mb: used_mem,
                                vram_free_mb: free_mem,
                                temperature_c: temp,
                                utilization_gpu: util.map(|u| u.gpu),
                                driver_version: nvml.sys_driver_version().ok(),
                            });
                        }
                    }
                }
                Err(e) => eprintln!("⚠️ NVML device count error: {}", e),
            },
            Err(_) => {
                // NVML no disponible (normal en WSL2) - nvidia-smi funciona como fallback
                // No mostrar warning ya que es comportamiento esperado
            }
        }

        // 2. Fallback: Intentar nvidia-smi CLI si NVML falló o no encontró nada
        if gpus.is_empty() {
            // SEC-EXT-PRIV-020: nvidia-smi es solo lectura  bajamos privilegios
            // al usuario invocador (SUDO_UID) si corremos como root.
            use crate::infrastructure::drop_privs::command_as_invoking_user;

            // Rutas posibles para nvidia-smi
            let paths = vec![
                "nvidia-smi",
                "/usr/lib/wsl/lib/nvidia-smi",
                "/usr/bin/nvidia-smi",
            ];

            for cmd_path in paths {
                let output = command_as_invoking_user(cmd_path)
                    .args([
                        "--query-gpu=name,memory.total,memory.used,memory.free",
                        "--format=csv,noheader,nounits",
                    ])
                    .output();

                match output {
                    Ok(o) => {
                        if o.status.success() {
                            let stdout = String::from_utf8_lossy(&o.stdout);
                            for (i, line) in stdout.lines().enumerate() {
                                let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
                                if parts.len() >= 4 {
                                    let name = parts[0].to_string();
                                    let total = parts[1].parse::<u64>().unwrap_or(0);
                                    let used = parts[2].parse::<u64>().unwrap_or(0);
                                    let free = parts[3].parse::<u64>().unwrap_or(0);

                                    gpus.push(GpuInfo {
                                        index: i as u32,
                                        name,
                                        brand: GpuBrand::Nvidia,
                                        vram_total_mb: total,
                                        vram_used_mb: used,
                                        vram_free_mb: free,
                                        temperature_c: None,
                                        utilization_gpu: None,
                                        driver_version: None,
                                    });
                                }
                            }
                            // Si tuvimos éxito con una ruta, paramos de buscar
                            if !gpus.is_empty() {
                                break;
                            }
                        }
                    }
                    Err(_) => {
                        // Continuar con la siguiente ruta
                        continue;
                    }
                }
            }
        }

        gpus
    }
}

#[async_trait]
impl HardwareProbePort for EnolaHardwareProbe {
    async fn probe(&self) -> Result<SystemHardwareSpecs, anyhow::Error> {
        let mut sys = self.system.lock().await;
        sys.refresh_all(); // Refresh CPU/RAM usage

        let cpu_cores = sys.cpus().len();
        let ram_total_mb = sys.total_memory() / 1024 / 1024;
        let ram_available_mb = sys.available_memory() / 1024 / 1024;
        let ram_used_mb = sys.used_memory() / 1024 / 1024;

        let platform = format!(
            "{} {}",
            System::name().unwrap_or_default(),
            System::kernel_version().unwrap_or_default()
        );

        // Detectar GPUs
        let nvidia_gpus = Self::check_nvidia_gpu();
        let gpus = nvidia_gpus;

        // TODO: Implement AMD detection via rocm-smi if needed in future

        let gpu_brand = if !gpus.is_empty() {
            gpus[0].brand.clone()
        } else {
            GpuBrand::None
        };

        // Battery state logic placeholder (sysinfo might not give battery nicely on servers)
        // En servidores, esto suele ser irrelevante, pero para portátiles dev sí.
        let battery_status = BatteryState::AcConnected; // Default for server

        Ok(SystemHardwareSpecs {
            cpu_cores,
            ram_total_mb,
            ram_available_mb,
            ram_used_mb,
            gpu_brand,
            gpus,
            battery_status,
            platform,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_constructor() {
        let probe = EnolaHardwareProbe::new();
        let _ = probe;
    }

    #[test]
    fn test_check_nvidia_gpu_no_panic() {
        // Should not panic even without NVIDIA GPU
        let gpus = EnolaHardwareProbe::check_nvidia_gpu();
        // May be empty or not — just verifying no crash
        let _ = gpus;
    }

    #[tokio::test]
    async fn test_probe_returns_specs() {
        let probe = EnolaHardwareProbe::new();
        let specs = probe.probe().await.unwrap();
        assert!(specs.cpu_cores > 0);
        assert!(specs.ram_total_mb > 0);
    }
}
