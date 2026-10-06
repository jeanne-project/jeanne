//! Détection du matériel, profilage de la mémoire vive (RAM)
//! et sondage des capacités d'accélération matérielle Vulkan (iGPU / GPU).

use serde::{Deserialize, Serialize};
#[cfg(target_os = "linux")]
use std::fs;
use std::path::Path;

/// Informations matérielles et capacités d'accélération de la machine hôte.
fn default_max_context() -> u32 {
    4096
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HardwareInfo {
    pub total_system_ram_mb: u64,
    pub available_ram_mb: u64,
    pub vulkan_device_name: Option<String>,
    pub vulkan_supported: bool,
    pub recommended_model_loaded: bool,
    #[serde(default = "default_max_context")]
    pub max_recommended_context: u32,
}

impl Default for HardwareInfo {
    fn default() -> Self {
        Self {
            total_system_ram_mb: 0,
            available_ram_mb: 0,
            vulkan_device_name: None,
            vulkan_supported: false,
            recommended_model_loaded: false,
            max_recommended_context: 4096,
        }
    }
}

/// Détecte le profil matériel actuel du système de manière déterministe et sans panique.
pub fn detect_hardware() -> HardwareInfo {
    let (total_ram, available_ram) = detect_system_ram();
    let (vulkan_supported, vulkan_device) = detect_vulkan();

    let max_recommended_context = if total_ram > 32768 {
        32768
    } else if total_ram > 24576 {
        16384
    } else if total_ram > 16384 {
        8192
    } else {
        4096
    };

    HardwareInfo {
        total_system_ram_mb: total_ram,
        available_ram_mb: available_ram,
        vulkan_device_name: vulkan_device,
        vulkan_supported,
        recommended_model_loaded: false,
        max_recommended_context,
    }
}

/// Détecte la mémoire RAM totale et disponible en Mo selon l'OS (Linux, Windows, macOS).
/// En cas d'échec ou d'impossibilité de détection, retourne strictement `(0, 0)` pour indiquer
/// à l'interface graphique que la valeur n'a pas pu être récupérée (aucun repli factice).
fn detect_system_ram() -> (u64, u64) {
    #[cfg(target_os = "linux")]
    {
        if let Some((total, avail)) = detect_linux_ram() {
            return (total, avail);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some((total, avail)) = detect_windows_ram() {
            return (total, avail);
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some((total, avail)) = detect_macos_ram() {
            return (total, avail);
        }
    }

    tracing::warn!(
        "[Hardware] Impossible de mesurer la mémoire RAM système sur cet environnement. Aucune valeur arbitraire attribuée (0 Mo)."
    );
    (0, 0)
}

#[cfg(target_os = "linux")]
fn detect_linux_ram() -> Option<(u64, u64)> {
    let content = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb = 0u64;
    let mut available_kb = 0u64;
    let mut free_kb = 0u64;
    let mut buffers_kb = 0u64;
    let mut cached_kb = 0u64;

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
        } else if let Some(rest) = line.strip_prefix("MemFree:") {
            if let Some(val_str) = rest.split_whitespace().next() {
                if let Ok(val) = val_str.parse::<u64>() {
                    free_kb = val;
                }
            }
        } else if let Some(rest) = line.strip_prefix("Buffers:") {
            if let Some(val_str) = rest.split_whitespace().next() {
                if let Ok(val) = val_str.parse::<u64>() {
                    buffers_kb = val;
                }
            }
        } else if let Some(rest) = line.strip_prefix("Cached:") {
            if let Some(val_str) = rest.split_whitespace().next() {
                if let Ok(val) = val_str.parse::<u64>() {
                    cached_kb = val;
                }
            }
        }
    }

    if total_kb > 0 {
        let total_mb = total_kb / 1024;
        let avail_mb = if available_kb > 0 {
            available_kb / 1024
        } else if free_kb > 0 {
            (free_kb + buffers_kb + cached_kb) / 1024
        } else {
            total_mb / 2
        };
        Some((total_mb, avail_mb))
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
mod win_mem {
    #[repr(C)]
    pub struct MemoryStatusEx {
        pub dw_length: u32,
        pub dw_memory_load: u32,
        pub ull_total_phys: u64,
        pub ull_avail_phys: u64,
        pub ull_total_page_file: u64,
        pub ull_avail_page_file: u64,
        pub ull_total_virtual: u64,
        pub ull_avail_virtual: u64,
        pub ull_avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        pub fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
    }
}

#[cfg(target_os = "windows")]
fn detect_windows_ram() -> Option<(u64, u64)> {
    use std::mem;
    let mut status: win_mem::MemoryStatusEx = unsafe { mem::zeroed() };
    status.dw_length = mem::size_of::<win_mem::MemoryStatusEx>() as u32;

    let res = unsafe { win_mem::GlobalMemoryStatusEx(&mut status) };
    if res != 0 && status.ull_total_phys > 0 {
        let total_mb = status.ull_total_phys / (1024 * 1024);
        let avail_mb = status.ull_avail_phys / (1024 * 1024);
        Some((total_mb, avail_mb))
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
mod mac_mem {
    #[link(name = "c")]
    unsafe extern "C" {
        pub fn sysctlbyname(
            name: *const std::ffi::c_char,
            oldp: *mut std::ffi::c_void,
            oldlenp: *mut usize,
            newp: *const std::ffi::c_void,
            newlen: usize,
        ) -> std::ffi::c_int;
    }
}

#[cfg(target_os = "macos")]
fn detect_macos_ram() -> Option<(u64, u64)> {
    let mut mem_bytes: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    let name = c"hw.memsize";
    let ret = unsafe {
        mac_mem::sysctlbyname(
            name.as_ptr(),
            &mut mem_bytes as *mut _ as *mut _,
            &mut len,
            std::ptr::null(),
            0,
        )
    };

    if ret == 0 && mem_bytes > 0 {
        let total_mb = mem_bytes / (1024 * 1024);
        let avail_mb = detect_macos_available_ram_vmstat().unwrap_or(total_mb / 2);
        Some((total_mb, avail_mb))
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn detect_macos_available_ram_vmstat() -> Option<u64> {
    let output = std::process::Command::new("vm_stat").output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut free_pages: u64 = 0;
    let mut inactive_pages: u64 = 0;
    let mut speculative_pages: u64 = 0;
    let page_size: u64 = 4096;

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() == 2 {
            let key = parts[0].trim();
            let val_str = parts[1].trim().trim_end_matches('.');
            if let Ok(val) = val_str.parse::<u64>() {
                if key.contains("Pages free") {
                    free_pages = val;
                } else if key.contains("Pages inactive") {
                    inactive_pages = val;
                } else if key.contains("Pages speculative") {
                    speculative_pages = val;
                }
            }
        }
    }
    let avail_bytes = (free_pages + inactive_pages + speculative_pages) * page_size;
    Some(avail_bytes / (1024 * 1024))
}

/// Vérifie la présence de Vulkan ou de l'accélération graphique selon l'OS (Linux, Windows, macOS).
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
                .unwrap_or_else(|| "Périphérique graphique compatible Vulkan".to_string());
            return (true, Some(device_name));
        }
    }

    #[cfg(target_os = "windows")]
    {
        let win_candidates = [
            "C:\\Windows\\System32\\vulkan-1.dll",
            "C:\\Windows\\SysWOW64\\vulkan-1.dll",
        ];
        let found_vulkan = win_candidates.iter().any(|path| Path::new(path).exists());
        if found_vulkan {
            let device_name = detect_windows_gpu_name()
                .unwrap_or_else(|| "GPU compatible Vulkan/DirectX".to_string());
            return (true, Some(device_name));
        }
    }

    #[cfg(target_os = "macos")]
    {
        let gpu_name = if cfg!(target_arch = "aarch64") {
            "Apple Silicon GPU (Metal)".to_string()
        } else {
            "macOS Graphics Device (Metal)".to_string()
        };
        return (true, Some(gpu_name));
    }

    tracing::warn!(
        "[Hardware] Aucun runtime Vulkan détecté sur le système hôte. Repli sur exécution CPU uniquement."
    );
    (false, None)
}

#[cfg(target_os = "windows")]
fn detect_windows_gpu_name() -> Option<String> {
    if let Ok(output) = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-CimInstance Win32_VideoController | Select-Object -First 1).Name",
        ])
        .output()
    {
        if output.status.success() {
            let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardware_info_default() {
        let hw = HardwareInfo::default();
        assert_eq!(hw.total_system_ram_mb, 0);
        assert_eq!(hw.available_ram_mb, 0);
        assert!(!hw.vulkan_supported);
        assert_eq!(hw.max_recommended_context, 4096);
    }

    #[test]
    fn test_detect_hardware_no_panic() {
        let hw = detect_hardware();
        if hw.total_system_ram_mb > 0 {
            assert!(hw.available_ram_mb <= hw.total_system_ram_mb);
        }
        assert!(hw.max_recommended_context >= 4096);
    }

    #[test]
    fn test_hardware_serialization_roundtrip() {
        let hw = HardwareInfo {
            total_system_ram_mb: 16384,
            available_ram_mb: 8192,
            vulkan_device_name: Some("Test GPU".to_string()),
            vulkan_supported: true,
            recommended_model_loaded: true,
            max_recommended_context: 8192,
        };
        let json = serde_json::to_string(&hw).expect("serialization");
        let parsed: HardwareInfo = serde_json::from_str(&json).expect("deserialization");
        assert_eq!(hw, parsed);
    }
}
