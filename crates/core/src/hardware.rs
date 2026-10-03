//! Détection du matériel, profilage de la mémoire vive (RAM)
//! et sondage des capacités d'accélération matérielle Vulkan (iGPU / GPU).

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// Informations matérielles et capacités d'accélération de la machine hôte.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareInfo {
    pub total_system_ram_mb: u64,
    pub available_ram_mb: u64,
    pub vulkan_device_name: Option<String>,
    pub vulkan_supported: bool,
    pub recommended_model_loaded: bool,
}

impl Default for HardwareInfo {
    fn default() -> Self {
        Self {
            total_system_ram_mb: 16384,
            available_ram_mb: 8192,
            vulkan_device_name: None,
            vulkan_supported: false,
            recommended_model_loaded: false,
        }
    }
}

/// Détecte le profil matériel actuel du système de manière déterministe et sans panique.
pub fn detect_hardware() -> HardwareInfo {
    let (total_ram, available_ram) = detect_system_ram();
    let (vulkan_supported, vulkan_device) = detect_vulkan();

    HardwareInfo {
        total_system_ram_mb: total_ram,
        available_ram_mb: available_ram,
        vulkan_device_name: vulkan_device,
        vulkan_supported,
        recommended_model_loaded: false,
    }
}

/// Détecte la mémoire RAM totale et disponible en Mo.
fn detect_system_ram() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(content) = fs::read_to_string("/proc/meminfo") {
            let mut total_kb = 0u64;
            let mut available_kb = 0u64;

            for line in content.lines() {
                if let Some(rest) = line.strip_prefix("MemTotal:") {
                    if let Some(val_str) = rest.split_whitespace().next() {
                        if let Ok(val) = val_str.parse::<u64>() {
                            total_kb = val;
                        }
                    }
                } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
                    if let Some(val_str) = rest.split_whitespace().next() {
                        if let Ok(val) = val_str.parse::<u64>() {
                            available_kb = val;
                        }
                    }
                }
            }

            if total_kb > 0 {
                let total_mb = total_kb / 1024;
                let avail_mb = if available_kb > 0 {
                    available_kb / 1024
                } else {
                    total_mb / 2
                };
                return (total_mb, avail_mb);
            }
        }
    }

    // Repli sécurisé pour autres OS ou conteneurs
    (16384, 8192)
}

/// Vérifie la présence du runtime Vulkan sur la machine.
fn detect_vulkan() -> (bool, Option<String>) {
    #[cfg(target_os = "linux")]
    {
        let candidate_libs = [
            "/lib/x86_64-linux-gnu/libvulkan.so.1",
            "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
            "/usr/lib64/libvulkan.so.1",
            "/usr/lib/libvulkan.so.1",
            "/etc/vulkan",
        ];

        let mut found_lib = false;
        for lib in candidate_libs {
            if Path::new(lib).exists() {
                found_lib = true;
                break;
            }
        }

        if found_lib {
            // Lecture du premier GPU Vulkan si accessible ou nom standard
            let device_name = detect_linux_gpu_name()
                .unwrap_or_else(|| "Vulkan Compatible Graphics Device".to_string());
            return (true, Some(device_name));
        }
    }

    #[cfg(target_os = "windows")]
    {
        let win_candidates = [
            "C:\\Windows\\System32\\vulkan-1.dll",
            "C:\\Windows\\SysWOW64\\vulkan-1.dll",
        ];
        for path in win_candidates {
            if Path::new(path).exists() {
                return (true, Some("Vulkan Compatible GPU".to_string()));
            }
        }
    }

    (false, None)
}

#[cfg(target_os = "linux")]
fn detect_linux_gpu_name() -> Option<String> {
    // Vérification des périphériques DRM dans sysfs
    if let Ok(entries) = fs::read_dir("/sys/class/drm") {
        for entry in entries.flatten() {
            let path = entry.path().join("device");
            if path.exists() {
                // Tente de lire le driver ou identifiant uevent
                if let Ok(uevent) = fs::read_to_string(path.join("uevent")) {
                    for line in uevent.lines() {
                        if line.contains("DRIVER=i915") || line.contains("DRIVER=xe") {
                            return Some("Intel Iris Xe Graphics (Vulkan)".to_string());
                        } else if line.contains("DRIVER=amdgpu") {
                            return Some("AMD Radeon Graphics (Vulkan)".to_string());
                        } else if line.contains("DRIVER=nouveau") || line.contains("DRIVER=nvidia")
                        {
                            return Some("NVIDIA GeForce (Vulkan)".to_string());
                        }
                    }
                }
            }
        }
    }
    None
}
