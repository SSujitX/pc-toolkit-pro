"""System monitoring functionality for PC Tool."""

import psutil
import platform
import datetime
import subprocess
import json
from PyQt6.QtCore import QThread, pyqtSignal


class SystemInfoThread(QThread):
    """Thread for continuously monitoring system information."""

    info_updated = pyqtSignal(dict)

    def __init__(self):
        super().__init__()
        self.running = True
        self.gpu_available = self._check_gpu_availability()

    def _check_gpu_availability(self):
        """Check if GPU monitoring is available."""
        try:
            result = subprocess.run(
                [
                    "nvidia-smi",
                    "--query-gpu=utilization.gpu",
                    "--format=csv,noheader,nounits",
                ],
                capture_output=True,
                text=True,
                timeout=5,
                creationflags=subprocess.CREATE_NO_WINDOW,
            )
            return result.returncode == 0
        except (subprocess.TimeoutExpired, FileNotFoundError):
            return False

    def _get_gpu_info(self):
        """Get GPU utilization information."""
        if not self.gpu_available:
            return None

        try:
            # Get GPU utilization
            result = subprocess.run(
                [
                    "nvidia-smi",
                    "--query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu",
                    "--format=csv,noheader,nounits",
                ],
                capture_output=True,
                text=True,
                timeout=5,
                creationflags=subprocess.CREATE_NO_WINDOW,
            )

            if result.returncode == 0 and result.stdout.strip():
                gpu_data = result.stdout.strip().split(", ")
                if len(gpu_data) >= 4:
                    return {
                        "utilization": float(gpu_data[0]),
                        "memory_used": float(gpu_data[1]),
                        "memory_total": float(gpu_data[2]),
                        "temperature": float(gpu_data[3]),
                    }
        except (subprocess.TimeoutExpired, ValueError, IndexError):
            pass

        return None

    def run(self):
        """Main thread loop for system monitoring."""
        while self.running:
            try:
                cpu_percent = psutil.cpu_percent(interval=1)
                memory = psutil.virtual_memory()
                disk = psutil.disk_usage("C:\\")
                boot_time = datetime.datetime.fromtimestamp(psutil.boot_time())
                uptime = datetime.datetime.now() - boot_time

                info = {
                    "cpu": cpu_percent,
                    "memory_percent": memory.percent,
                    "memory_used": memory.used,
                    "memory_total": memory.total,
                    "disk_percent": (disk.used / disk.total) * 100,
                    "disk_used": disk.used,
                    "disk_total": disk.total,
                    "uptime": str(uptime).split(".")[0],
                }

                # Add GPU information if available
                gpu_info = self._get_gpu_info()
                if gpu_info:
                    info["gpu_available"] = True
                    info["gpu_utilization"] = gpu_info["utilization"]
                    info["gpu_memory_used"] = gpu_info["memory_used"]
                    info["gpu_memory_total"] = gpu_info["memory_total"]
                    info["gpu_temperature"] = gpu_info["temperature"]
                else:
                    info["gpu_available"] = False

                self.info_updated.emit(info)

            except Exception as e:
                print(f"System monitoring error: {e}")

            self.msleep(5000)  # Reduced frequency from 2s to 5s for better performance

    def stop(self):
        """Stop the monitoring thread."""
        self.running = False
        self.quit()
        self.wait()


class SystemMonitor:
    """Handles system monitoring operations."""

    @staticmethod
    def get_system_info():
        """Get static system information."""
        return {
            "os": f"{platform.system()} {platform.release()}",
            "processor": platform.processor(),
            "architecture": platform.architecture()[0],
            "machine": platform.machine(),
            "python_version": platform.python_version(),
        }
