use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemResources {
    pub global_cpu_usage: f32,
    pub global_memory_used: u64,
    pub global_memory_total: u64,
    pub disks: Vec<DiskResource>,
    pub services: Vec<ServiceResource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskResource {
    pub mount_point: String,
    pub total: u64,
    pub available: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceResource {
    pub name: String,
    pub status: String, // "active", "inactive", "failed"
    pub cpu_usage: f32,
    pub memory_bytes: u64,
    pub uptime_seconds: u64,
}
