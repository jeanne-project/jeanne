"""Automatic host hardware detection (CPU, RAM, GPU, Vulkan) for Jeanne benchmark."""

import os
import platform
import subprocess
from typing import Any, Dict, Optional, Tuple


def detect_cpu() -> Tuple[str, int]:
    """Detects CPU brand name and logical thread count."""
    cpu_name = platform.processor() or "Unknown CPU"
    cores = os.cpu_count() or 1
    system = platform.system()

    if system == "Linux":
        try:
            with open("/proc/cpuinfo", "r", encoding="utf-8", errors="replace") as f:
                for line in f:
                    if "model name" in line:
                        cpu_name = line.split(":", 1)[1].strip()
                        break
        except Exception:
            pass
    elif system == "Windows":
        try:
            # Check processor identifier env var
            proc_id = os.environ.get("PROCESSOR_IDENTIFIER")
            if proc_id:
                cpu_name = proc_id
            # Try wmic or powershell
            cmd = "powershell -Command \"(Get-CimInstance Win32_Processor).Name\""
            res = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=3)
            if res.returncode == 0 and res.stdout.strip():
                cpu_name = res.stdout.strip().splitlines()[0]
        except Exception:
            pass
    elif system == "Darwin":
        try:
            res = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True, timeout=3)
            if res.returncode == 0 and res.stdout.strip():
                cpu_name = res.stdout.strip()
        except Exception:
            pass

    return cpu_name, cores


def detect_ram_gb() -> float:
    """Detects total physical system RAM in Gigabytes."""
    system = platform.system()

    if system == "Linux":
        try:
            with open("/proc/meminfo", "r", encoding="utf-8", errors="replace") as f:
                for line in f:
                    if line.startswith("MemTotal:"):
                        kb_val = int(line.split()[1])
                        return round(kb_val / (1024 * 1024), 1)
        except Exception:
            pass
    elif system == "Windows":
        try:
            import ctypes
            class MEMORYSTATUSEX(ctypes.Structure):
                _fields_ = [
                    ("dwLength", ctypes.c_ulong),
                    ("dwMemoryLoad", ctypes.c_ulong),
                    ("ullTotalPhys", ctypes.c_ulonglong),
                    ("ullAvailPhys", ctypes.c_ulonglong),
                    ("ullTotalPageFile", ctypes.c_ulonglong),
                    ("ullAvailPageFile", ctypes.c_ulonglong),
                    ("ullTotalVirtual", ctypes.c_ulonglong),
                    ("ullAvailVirtual", ctypes.c_ulonglong),
                    ("sullAvailExtendedVirtual", ctypes.c_ulonglong),
                ]
            stat = MEMORYSTATUSEX()
            stat.dwLength = ctypes.sizeof(MEMORYSTATUSEX)
            if ctypes.windll.kernel32.GlobalMemoryStatusEx(ctypes.byref(stat)):
                return round(stat.ullTotalPhys / (1024**3), 1)
        except Exception:
            pass
    elif system == "Darwin":
        try:
            res = subprocess.run(["sysctl", "-n", "hw.memsize"], capture_output=True, text=True, timeout=3)
            if res.returncode == 0 and res.stdout.strip():
                bytes_val = int(res.stdout.strip())
                return round(bytes_val / (1024**3), 1)
        except Exception:
            pass

    return 16.0  # Default Jeanne target baseline


def detect_gpu() -> Tuple[Optional[str], bool]:
    """
    Detects GPU vendor/model and Vulkan/Metal compute capability.
    Returns: (gpu_name, has_acceleration)
    """
    system = platform.system()

    # 1. Try NVIDIA-SMI first
    try:
        res = subprocess.run(
            ["nvidia-smi", "--query-gpu=name,memory.total", "--format=csv,noheader"],
            capture_output=True,
            text=True,
            timeout=3,
        )
        if res.returncode == 0 and res.stdout.strip():
            first_gpu = res.stdout.strip().splitlines()[0]
            return f"NVIDIA {first_gpu}", True
    except Exception:
        pass

    # 2. Linux DRM sysfs inspection (mirrors Jeanne's hardware.rs)
    if system == "Linux":
        vulkan_present = any(
            os.path.exists(p)
            for p in [
                "/lib/x86_64-linux-gnu/libvulkan.so.1",
                "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
                "/usr/lib64/libvulkan.so.1",
                "/usr/lib/libvulkan.so.1",
                "/etc/vulkan",
            ]
        )

        drm_dir = "/sys/class/drm"
        if os.path.exists(drm_dir):
            try:
                for entry in sorted(os.listdir(drm_dir)):
                    uevent_path = os.path.join(drm_dir, entry, "device", "uevent")
                    if os.path.exists(uevent_path):
                        with open(uevent_path, "r", errors="replace") as f:
                            content = f.read()
                            if "DRIVER=amdgpu" in content:
                                accel_tag = " (Vulkan)" if vulkan_present else ""
                                return f"AMD Radeon Graphics{accel_tag}", True
                            elif "DRIVER=i915" in content or "DRIVER=xe" in content:
                                accel_tag = " (Vulkan)" if vulkan_present else ""
                                return f"Intel Iris Xe / UHD Graphics{accel_tag}", True
                            elif "DRIVER=nvidia" in content:
                                return "NVIDIA GPU (Vulkan)", True
            except Exception:
                pass

        if vulkan_present:
            return "Périphérique graphique compatible Vulkan", True

    # 3. Windows PowerShell GPU query
    elif system == "Windows":
        vulkan_win = any(
            os.path.exists(p)
            for p in ["C:\\Windows\\System32\\vulkan-1.dll", "C:\\Windows\\SysWOW64\\vulkan-1.dll"]
        )
        try:
            cmd = "powershell -Command \"(Get-CimInstance Win32_VideoController).Name\""
            res = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=4)
            if res.returncode == 0 and res.stdout.strip():
                gpus = [g.strip() for g in res.stdout.strip().splitlines() if g.strip()]
                if gpus:
                    gpu_str = " / ".join(gpus)
                    accel_tag = " (Vulkan)" if vulkan_win else ""
                    return f"{gpu_str}{accel_tag}", True
        except Exception:
            pass

    # 4. macOS Metal
    elif system == "Darwin":
        return "Apple Silicon (Metal)", True

    return None, False


def detect_host_hardware() -> Dict[str, Any]:
    """Gathers all hardware metrics into a structured dictionary."""
    cpu_name, cpu_threads = detect_cpu()
    ram_gb = detect_ram_gb()
    gpu_name, has_accel = detect_gpu()

    parts = [f"{cpu_name} ({cpu_threads} threads)", f"{ram_gb} Go RAM"]
    if gpu_name:
        parts.append(f"GPU: {gpu_name}")
    else:
        parts.append("GPU: Aucun GPU dédié détecté (inférence CPU)")

    summary_string = " | ".join(parts)

    return {
        "os": f"{platform.system()} {platform.release()} ({platform.machine()})",
        "cpu": cpu_name,
        "cpu_threads": cpu_threads,
        "ram_gb": ram_gb,
        "gpu": gpu_name or "Aucun",
        "hardware_acceleration": has_accel,
        "summary": summary_string,
    }
