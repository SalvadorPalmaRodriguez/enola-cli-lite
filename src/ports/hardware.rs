use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GpuBrand {
    Nvidia,
    Amd,
    Intel,
    None,
    Unknown(String),
}

impl fmt::Display for GpuBrand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GpuBrand::Nvidia => write!(f, "NVIDIA"),
            GpuBrand::Amd => write!(f, "AMD"),
            GpuBrand::Intel => write!(f, "Intel"),
            GpuBrand::None => write!(f, "None"),
            GpuBrand::Unknown(s) => write!(f, "Unknown ({})", s),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BatteryState {
    Charging(f32),    // Percentage
    Discharging(f32), // Percentage
    Full,
    Unknown,
    AcConnected, // Desktop
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GpuInfo {
    pub index: u32,
    pub name: String,
    pub brand: GpuBrand,
    pub vram_total_mb: u64,
    pub vram_used_mb: u64,
    pub vram_free_mb: u64,
    pub temperature_c: Option<u32>,
    pub utilization_gpu: Option<u32>, // %
    pub driver_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemHardwareSpecs {
    pub cpu_cores: usize,
    pub ram_total_mb: u64,
    pub ram_available_mb: u64,
    pub ram_used_mb: u64,
    pub gpu_brand: GpuBrand, // Primary GPU brand
    pub gpus: Vec<GpuInfo>,
    pub battery_status: BatteryState,
    pub platform: String,
}

#[async_trait]
pub trait HardwareProbePort: Send + Sync {
    /// Detect system hardware capabilities
    async fn probe(&self) -> Result<SystemHardwareSpecs, anyhow::Error>;

    /// Quick check for GPU availability
    async fn has_gpu(&self) -> bool {
        match self.probe().await {
            Ok(specs) => !specs.gpus.is_empty(),
            Err(_) => false,
        }
    }
}

#[cfg(test)]
mockall::mock! {
    pub HardwareProbePort {}
    #[async_trait]
    impl HardwareProbePort for HardwareProbePort {
        async fn probe(&self) -> Result<SystemHardwareSpecs, anyhow::Error>;
        async fn has_gpu(&self) -> bool;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_hardware_probe_with_gpu() {
        let mut mock = MockHardwareProbePort::new();
        mock.expect_probe().returning(|| {
            Ok(SystemHardwareSpecs {
                cpu_cores: 8,
                ram_total_mb: 16384,
                ram_available_mb: 8000,
                ram_used_mb: 8384,
                gpu_brand: GpuBrand::Nvidia,
                gpus: vec![GpuInfo {
                    index: 0,
                    name: "RTX 4060".into(),
                    brand: GpuBrand::Nvidia,
                    vram_total_mb: 8192,
                    vram_used_mb: 1024,
                    vram_free_mb: 7168,
                    temperature_c: Some(45),
                    utilization_gpu: Some(10),
                    driver_version: Some("535.0".into()),
                }],
                battery_status: BatteryState::AcConnected,
                platform: "linux".into(),
            })
        });
        mock.expect_has_gpu().returning(|| true);

        let specs = mock.probe().await.unwrap();
        assert_eq!(specs.cpu_cores, 8);
        assert_eq!(specs.gpus.len(), 1);
        assert!(mock.has_gpu().await);
    }

    #[tokio::test]
    async fn test_mock_hardware_probe_no_gpu() {
        let mut mock = MockHardwareProbePort::new();
        mock.expect_has_gpu().returning(|| false);
        assert!(!mock.has_gpu().await);
    }

    #[test]
    fn test_gpu_brand_display() {
        assert_eq!(GpuBrand::Nvidia.to_string(), "NVIDIA");
        assert_eq!(GpuBrand::None.to_string(), "None");
        assert_eq!(
            GpuBrand::Unknown("test".into()).to_string(),
            "Unknown (test)"
        );
    }
}
