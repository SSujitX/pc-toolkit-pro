"""System Information Widget for detailed hardware display."""

import psutil
import platform
import subprocess
import datetime
from concurrent.futures import ThreadPoolExecutor
from PyQt6.QtWidgets import (
    QWidget,
    QVBoxLayout,
    QHBoxLayout,
    QLabel,
    QProgressBar,
    QFrame,
    QScrollArea,
    QGridLayout,
    QGroupBox,
    QPushButton,
    QApplication,
)
from PyQt6.QtCore import Qt, QTimer, QThread, pyqtSignal
from PyQt6.QtGui import QFont, QPixmap


class SystemInfoLoader(QThread):
    """Background thread for loading system information."""

    # Signals for different types of data
    uptime_updated = pyqtSignal(str)
    cpu_info_updated = pyqtSignal(dict)
    memory_info_updated = pyqtSignal(dict)
    disk_info_updated = pyqtSignal(dict)
    storage_overview_updated = pyqtSignal(dict)
    gpu_info_updated = pyqtSignal(dict)
    monitor_info_updated = pyqtSignal(dict)
    motherboard_info_updated = pyqtSignal(dict)
    os_info_updated = pyqtSignal(dict)

    def __init__(self):
        super().__init__()
        self._cpu_name_cache = None
        self._device_name_cache = None
        self._os_info_cache = {}
        self._cpu_info_cache = None
        self._motherboard_cache = None
        self._monitor_cache = None
        self._gpu_cache = None
        self._last_gpu_check = 0
        self.running = True

    def stop(self):
        """Stop the background thread."""
        self.running = False
        self.quit()
        self.wait()

    def run(self):
        """Main thread loop for updating system information."""
        # Initial load of static information
        self.load_static_info()

        # Continuous updates for dynamic information
        while self.running:
            try:
                self.update_dynamic_info()
                self.msleep(8000)  # Update every 8 seconds for better performance
            except Exception as e:
                print(f"Error in system info loader: {e}")
                self.msleep(5000)  # Wait longer on error

    def load_static_info(self):
        """Load static system information in parallel for better performance."""
        try:
            # Use ThreadPoolExecutor to load static info in parallel
            with ThreadPoolExecutor(max_workers=4) as executor:
                # Submit all tasks
                futures = {
                    executor.submit(self.get_cpu_info): "cpu",
                    executor.submit(self.get_monitor_info): "monitor",
                    executor.submit(self.get_motherboard_info): "motherboard",
                    executor.submit(self.get_os_info_all): "os",
                }

                # Process results as they complete
                for future in futures:
                    try:
                        result = future.result(timeout=10)  # 10 second timeout per task
                        info_type = futures[future]

                        # Emit appropriate signal based on type
                        if info_type == "cpu":
                            self.cpu_info_updated.emit(result)
                        elif info_type == "monitor":
                            self.monitor_info_updated.emit(result)
                        elif info_type == "motherboard":
                            self.motherboard_info_updated.emit(result)
                        elif info_type == "os":
                            self.os_info_updated.emit(result)

                    except Exception as e:
                        print(f"Error loading {futures[future]} info: {e}")
                        # Emit error data for failed loads
                        if futures[future] == "cpu":
                            self.cpu_info_updated.emit(
                                {"name": "Error", "cores": "Error"}
                            )
                        elif futures[future] == "monitor":
                            self.monitor_info_updated.emit(
                                {"monitors": ["Error detecting monitors"], "count": 0}
                            )
                        elif futures[future] == "motherboard":
                            self.motherboard_info_updated.emit(
                                {"product": "Error", "manufacturer": "Error"}
                            )
                        elif futures[future] == "os":
                            self.os_info_updated.emit(
                                {"device_name": "Error", "edition": "Error"}
                            )

        except Exception as e:
            print(f"Error in parallel static info loading: {e}")
            # Fallback to sequential loading if parallel fails
            self._load_static_info_sequential()

    def _load_static_info_sequential(self):
        """Fallback sequential loading method."""
        try:
            # Load CPU info
            cpu_info = self.get_cpu_info()
            self.cpu_info_updated.emit(cpu_info)

            # Load monitor info
            monitor_info = self.get_monitor_info()
            self.monitor_info_updated.emit(monitor_info)

            # Load motherboard info
            motherboard_info = self.get_motherboard_info()
            self.motherboard_info_updated.emit(motherboard_info)

            # Load OS info
            os_info = self.get_os_info_all()
            self.os_info_updated.emit(os_info)

        except Exception as e:
            print(f"Error loading static info: {e}")

    def update_dynamic_info(self):
        """Update dynamic system information."""
        try:
            # Update uptime
            uptime = self.get_uptime()
            self.uptime_updated.emit(uptime)

            # Update CPU usage
            cpu_usage = self.get_cpu_usage()
            self.cpu_info_updated.emit(cpu_usage)

            # Update current CPU speed
            cpu_speed = self.get_cpu_current_speed()
            self.cpu_info_updated.emit(cpu_speed)

            # Update memory info
            memory_info = self.get_memory_info()
            self.memory_info_updated.emit(memory_info)

            # Update disk info
            disk_info = self.get_disk_info()
            self.disk_info_updated.emit(disk_info)

            # Update storage overview info
            storage_overview_info = self.get_storage_overview_info()
            self.storage_overview_updated.emit(storage_overview_info)

            # Update GPU info
            gpu_info = self.get_gpu_info()
            self.gpu_info_updated.emit(gpu_info)

        except Exception as e:
            print(f"Error updating dynamic info: {e}")

    def get_uptime(self):
        """Get system uptime."""
        try:
            boot_time = datetime.datetime.fromtimestamp(psutil.boot_time())
            uptime = datetime.datetime.now() - boot_time

            days = uptime.days
            hours, remainder = divmod(uptime.seconds, 3600)
            minutes, seconds = divmod(remainder, 60)

            if days > 0:
                return f"{days} days, {hours:02d}:{minutes:02d}:{seconds:02d}"
            else:
                return f"{hours:02d}:{minutes:02d}:{seconds:02d}"
        except:
            return "Unknown"

    def get_cpu_info(self):
        """Get CPU information with caching."""
        # Return cached data if available
        if self._cpu_info_cache is not None:
            return self._cpu_info_cache

        info = {"type": "static"}

        try:
            # CPU name (cached)
            if self._cpu_name_cache is None:
                self._cpu_name_cache = self._get_cpu_name()
            info["name"] = self._cpu_name_cache

            # Cores and threads
            physical_cores = psutil.cpu_count(logical=False)
            logical_cores = psutil.cpu_count(logical=True)
            info["cores"] = f"{physical_cores} cores, {logical_cores} threads"

            # Frequency information (base and max)
            try:
                freq = psutil.cpu_freq()
                base_freq_mhz = self._get_cpu_base_frequency()

                if base_freq_mhz and freq and freq.max:
                    base_freq_ghz = base_freq_mhz / 1000.0
                    max_freq_ghz = freq.max / 1000.0
                    info["frequency"] = (
                        f"{base_freq_ghz:.2f} GHz (Max: {max_freq_ghz:.2f} GHz)"
                    )
                elif base_freq_mhz:
                    base_freq_ghz = base_freq_mhz / 1000.0
                    info["frequency"] = f"{base_freq_ghz:.2f} GHz"
                elif freq and freq.max:
                    max_freq_ghz = freq.max / 1000.0
                    info["frequency"] = f"Max: {max_freq_ghz:.2f} GHz"
                else:
                    info["frequency"] = "Unknown"
            except Exception as e:
                print(f"Error getting frequency info: {e}")
                info["frequency"] = "Unknown"

            # Cache information (combined display)
            try:
                cache_info = self._get_cpu_cache_info()
                cache_parts = []

                if cache_info["l1_cache"] != "Unknown":
                    cache_parts.append(f"L1 - {cache_info['l1_cache']}")
                if cache_info["l2_cache"] != "Unknown":
                    cache_parts.append(f"L2 - {cache_info['l2_cache']}")
                if cache_info["l3_cache"] != "Unknown":
                    cache_parts.append(f"L3 - {cache_info['l3_cache']}")

                if cache_parts:
                    info["cache_display"] = " | ".join(cache_parts)
                else:
                    info["cache_display"] = "Unknown"

            except Exception as e:
                print(f"Error getting cache info: {e}")
                info["cache_display"] = "Unknown"

            # Socket information
            try:
                socket_info = self._get_cpu_socket_info()
                info["sockets"] = socket_info
            except Exception as e:
                print(f"Error getting socket info: {e}")
                info["sockets"] = "Unknown"

        except Exception as e:
            print(f"Error getting CPU info: {e}")
            info.update(
                {
                    "name": "Unknown",
                    "cores": "Unknown",
                    "frequency": "Unknown",
                    "cache_display": "Unknown",
                    "sockets": "Unknown",
                }
            )

        # Cache the result
        self._cpu_info_cache = info
        return info

    def get_cpu_current_speed(self):
        """Get current CPU speed for dynamic updates."""
        try:
            freq = psutil.cpu_freq()
            if freq and freq.current:
                current_freq_ghz = freq.current / 1000.0
                return {"type": "current_speed", "speed": f"{current_freq_ghz:.2f} GHz"}
            else:
                return {"type": "current_speed", "speed": "Unknown"}
        except Exception as e:
            print(f"Error getting current CPU speed: {e}")
            return {"type": "current_speed", "speed": "Unknown"}

    def get_cpu_usage(self):
        """Get CPU usage information."""
        try:
            cpu_percent = psutil.cpu_percent(interval=None)
            return {"type": "usage", "usage": cpu_percent}
        except:
            return {"type": "usage", "usage": 0}

    def _get_cpu_name(self):
        """Get CPU name using multiple methods."""
        cpu_name = "Unknown Processor"

        try:
            # Method 1: Try wmic
            try:
                result = subprocess.run(
                    ["wmic", "cpu", "get", "name", "/value"],
                    capture_output=True,
                    text=True,
                    timeout=5,
                    creationflags=subprocess.CREATE_NO_WINDOW,
                )

                if result.returncode == 0 and result.stdout:
                    for line in result.stdout.split("\n"):
                        line = line.strip()
                        if line.startswith("Name=") and len(line) > 5:
                            cpu_name = line.split("=", 1)[1].strip()
                            if cpu_name and cpu_name != "Unknown Processor":
                                break
            except:
                pass

            # Method 2: Try PowerShell
            if cpu_name == "Unknown Processor":
                try:
                    result = subprocess.run(
                        [
                            "powershell",
                            "-Command",
                            "Get-WmiObject -Class Win32_Processor | Select-Object -ExpandProperty Name",
                        ],
                        capture_output=True,
                        text=True,
                        timeout=5,
                        creationflags=subprocess.CREATE_NO_WINDOW,
                    )

                    if result.returncode == 0 and result.stdout.strip():
                        cpu_name = result.stdout.strip()
                except:
                    pass

            # Method 3: Try registry
            if cpu_name == "Unknown Processor":
                try:
                    import winreg

                    key = winreg.OpenKey(
                        winreg.HKEY_LOCAL_MACHINE,
                        r"HARDWARE\DESCRIPTION\System\CentralProcessor\0",
                    )
                    cpu_name = winreg.QueryValueEx(key, "ProcessorNameString")[
                        0
                    ].strip()
                    winreg.CloseKey(key)
                except:
                    pass

            # Method 4: Fallback
            if cpu_name == "Unknown Processor" or not cpu_name:
                cpu_name = platform.processor() or "Unknown Processor"

        except Exception as e:
            print(f"Error getting CPU name: {e}")
            cpu_name = platform.processor() or "Unknown Processor"

        return cpu_name

    def _get_cpu_base_frequency(self):
        """Get CPU base frequency using WMI commands."""
        base_freq = None

        try:
            # Method 1: Try wmic to get MaxClockSpeed (base frequency)
            try:
                result = subprocess.run(
                    ["wmic", "cpu", "get", "MaxClockSpeed", "/value"],
                    capture_output=True,
                    text=True,
                    timeout=5,
                    creationflags=subprocess.CREATE_NO_WINDOW,
                )

                if result.returncode == 0 and result.stdout:
                    for line in result.stdout.split("\n"):
                        line = line.strip()
                        if line.startswith("MaxClockSpeed=") and len(line) > 14:
                            freq_mhz = line.split("=", 1)[1].strip()
                            if freq_mhz and freq_mhz.isdigit():
                                base_freq = int(freq_mhz)
                                break
            except:
                pass

            # Method 2: Try PowerShell if wmic failed
            if base_freq is None:
                try:
                    result = subprocess.run(
                        [
                            "powershell",
                            "-Command",
                            "Get-WmiObject -Class Win32_Processor | Select-Object -ExpandProperty MaxClockSpeed",
                        ],
                        capture_output=True,
                        text=True,
                        timeout=5,
                        creationflags=subprocess.CREATE_NO_WINDOW,
                    )

                    if result.returncode == 0 and result.stdout.strip():
                        freq_str = result.stdout.strip()
                        if freq_str.isdigit():
                            base_freq = int(freq_str)
                except:
                    pass

        except Exception as e:
            print(f"Error getting base frequency: {e}")

        return base_freq

    def _get_cpu_cache_info(self):
        """Get CPU cache information using WMI commands."""
        cache_info = {
            "l1_cache": "Unknown",
            "l2_cache": "Unknown",
            "l3_cache": "Unknown",
        }

        try:
            # Try PowerShell to get cache information
            result = subprocess.run(
                [
                    "powershell",
                    "-Command",
                    "Get-WmiObject -Class Win32_CacheMemory | Select-Object Level, MaxCacheSize | ConvertTo-Json",
                ],
                capture_output=True,
                text=True,
                timeout=7,
                creationflags=subprocess.CREATE_NO_WINDOW,
            )

            if result.returncode == 0 and result.stdout.strip():
                import json

                cache_data = json.loads(result.stdout.strip())

                # Handle both single object and array
                if isinstance(cache_data, dict):
                    cache_data = [cache_data]

                for cache in cache_data:
                    level = cache.get("Level", 0)
                    size_kb = cache.get("MaxCacheSize", 0)

                    if size_kb and size_kb > 0:
                        if size_kb >= 1024:
                            size_str = f"{size_kb / 1024:.1f} MB"
                        else:
                            size_str = f"{size_kb} KB"

                        if level == 3:
                            cache_info["l1_cache"] = size_str
                        elif level == 4:
                            cache_info["l2_cache"] = size_str
                        elif level == 5:
                            cache_info["l3_cache"] = size_str

        except Exception as e:
            print(f"Error getting cache info: {e}")

        return cache_info

    def _get_cpu_socket_info(self):
        """Get CPU socket information using WMI commands."""
        try:
            # Try PowerShell to get socket information
            result = subprocess.run(
                [
                    "powershell",
                    "-Command",
                    "Get-WmiObject -Class Win32_Processor | Select-Object -ExpandProperty SocketDesignation",
                ],
                capture_output=True,
                text=True,
                timeout=5,
                creationflags=subprocess.CREATE_NO_WINDOW,
            )

            if result.returncode == 0 and result.stdout.strip():
                socket_info = result.stdout.strip()
                return socket_info if socket_info else "Unknown"

        except Exception as e:
            print(f"Error getting socket info: {e}")

        return "Unknown"

    def _get_ram_details(self):
        """Get detailed RAM information using WMI."""
        ram_info = {
            "ram_name": "Unknown",
            "ram_speed": "Unknown",
            "ram_type": "Unknown",
            "ram_slots": "Unknown",
        }

        try:
            # Get RAM details using PowerShell WMI query with SMBIOSMemoryType for better DDR detection
            cmd = [
                "powershell",
                "-Command",
                "Get-WmiObject -Class Win32_PhysicalMemory | Select-Object Manufacturer, PartNumber, Speed, MemoryType, SMBIOSMemoryType, Capacity, DeviceLocator | ConvertTo-Json",
            ]

            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=10,
                creationflags=subprocess.CREATE_NO_WINDOW,
            )

            if result.returncode == 0 and result.stdout.strip():
                import json

                ram_data = json.loads(result.stdout.strip())

                # Handle single or multiple RAM modules
                if isinstance(ram_data, dict):
                    ram_data = [ram_data]

                if ram_data:
                    # Get first module info
                    first_module = ram_data[0]

                    # RAM Name - Clean up and prioritize part number
                    manufacturer = first_module.get("Manufacturer", "").strip()
                    part_number = first_module.get("PartNumber", "").strip()

                    # Clean up manufacturer name - remove null chars and extra spaces
                    if manufacturer:
                        manufacturer = (
                            manufacturer.replace("\x00", "").replace("\0", "").strip()
                        )
                        manufacturer = " ".join(manufacturer.split())

                    # Clean up part number - remove null chars and extra spaces
                    if part_number:
                        part_number = (
                            part_number.replace("\x00", "").replace("\0", "").strip()
                        )
                        part_number = " ".join(part_number.split())

                    # Create readable RAM name - prefer part number as it's more descriptive
                    if part_number and part_number not in ["Unknown", "", "N/A"]:
                        # If manufacturer is Unknown or empty, just use part number
                        if not manufacturer or manufacturer in ["Unknown", "", "N/A"]:
                            ram_info["ram_name"] = part_number
                        # If part number contains manufacturer, use it as is
                        elif manufacturer.upper() in part_number.upper():
                            ram_info["ram_name"] = part_number
                        else:
                            # Combine manufacturer and part number
                            ram_info["ram_name"] = f"{manufacturer} {part_number}"
                    elif manufacturer and manufacturer not in ["Unknown", "", "N/A"]:
                        ram_info["ram_name"] = manufacturer

                    # RAM Speed
                    speed = first_module.get("Speed")
                    if speed and speed > 0:
                        ram_info["ram_speed"] = f"{speed} MHz"

                    # RAM Type - Use SMBIOSMemoryType for better detection, fallback to MemoryType
                    smbios_type = first_module.get("SMBIOSMemoryType")
                    memory_type = first_module.get("MemoryType")

                    # SMBIOS Memory Type mapping (more accurate for modern RAM)
                    smbios_type_map = {
                        20: "DDR",
                        21: "DDR2",
                        22: "DDR2 FB-DIMM",
                        24: "DDR3",
                        26: "DDR4",
                        34: "DDR5",
                    }

                    # Legacy MemoryType mapping
                    legacy_type_map = {
                        20: "DDR",
                        21: "DDR2",
                        22: "DDR2 FB-DIMM",
                        24: "DDR3",
                        26: "DDR4",
                        34: "DDR5",
                    }

                    # Try SMBIOS type first, then legacy type
                    if smbios_type and smbios_type in smbios_type_map:
                        ram_info["ram_type"] = smbios_type_map[smbios_type]
                    elif memory_type and memory_type in legacy_type_map:
                        ram_info["ram_type"] = legacy_type_map[memory_type]
                    else:
                        # Try to guess from part number or speed
                        if part_number:
                            part_upper = part_number.upper()
                            if "DDR5" in part_upper:
                                ram_info["ram_type"] = "DDR5"
                            elif "DDR4" in part_upper:
                                ram_info["ram_type"] = "DDR4"
                            elif "DDR3" in part_upper:
                                ram_info["ram_type"] = "DDR3"
                            elif "DDR2" in part_upper:
                                ram_info["ram_type"] = "DDR2"

                        # If still unknown, guess based on speed (rough estimation)
                        if ram_info["ram_type"] == "Unknown" and speed:
                            if speed >= 4800:
                                ram_info["ram_type"] = "DDR5"
                            elif speed >= 2133:
                                ram_info["ram_type"] = "DDR4"
                            elif speed >= 800:
                                ram_info["ram_type"] = "DDR3"
                            elif speed >= 400:
                                ram_info["ram_type"] = "DDR2"

                    # Number of slots used
                    slot_count = len(ram_data)
                    ram_info["ram_slots"] = f"{slot_count} slot(s) used"

        except Exception as e:
            print(f"Error getting RAM details: {e}")

        return ram_info

    def _get_storage_details(self):
        """Get detailed storage device information for C: drive only."""
        storage_info = {"storage_name": "Unknown", "storage_type": "Unknown"}

        try:
            # First, find which physical drive contains the C: partition
            cmd_find_drive = [
                "powershell",
                "-Command",
                "$partition = Get-WmiObject -Class Win32_LogicalDiskToPartition | Where-Object {$_.Dependent -like '*C:*'} | ForEach-Object { $_.Antecedent }; $partition -replace '.*DeviceID=\"([^\"]+)\".*', '$1'",
            ]

            result_partition = subprocess.run(
                cmd_find_drive,
                capture_output=True,
                text=True,
                timeout=10,
                creationflags=subprocess.CREATE_NO_WINDOW,
            )

            if result_partition.returncode == 0 and result_partition.stdout.strip():
                partition_info = result_partition.stdout.strip()
                # Extract disk number from "Disk #X, Partition #Y"
                import re

                disk_match = re.search(r"Disk #(\d+)", partition_info)

                if disk_match:
                    disk_number = disk_match.group(1)

                    # Now get the actual physical drive information
                    cmd_drive_info = [
                        "powershell",
                        "-Command",
                        f"Get-WmiObject -Class Win32_DiskDrive | Where-Object {{$_.DeviceID -eq '\\\\.\\PHYSICALDRIVE{disk_number}'}} | Select-Object Model, MediaType, InterfaceType | ConvertTo-Json",
                    ]

                    result = subprocess.run(
                        cmd_drive_info,
                        capture_output=True,
                        text=True,
                        timeout=7,
                        creationflags=subprocess.CREATE_NO_WINDOW,
                    )

                    if result.returncode == 0 and result.stdout.strip():
                        import json

                        storage_data = json.loads(result.stdout.strip())

                        if storage_data:
                            # Use the actual model name from the drive
                            model = storage_data.get("Model", "").strip()
                            media_type = storage_data.get("MediaType", "").strip()
                            interface_type = storage_data.get(
                                "InterfaceType", ""
                            ).strip()

                            if model:
                                storage_info["storage_name"] = model

                            # Determine storage type based on model name and interface
                            if model and any(
                                indicator in model.upper()
                                for indicator in [
                                    "SSD",
                                    "NVME",
                                    "M.2",
                                    "SOLID STATE",
                                    "PRO",
                                ]
                            ):
                                storage_info["storage_type"] = (
                                    "NVMe SSD"
                                    if "NVME" in model.upper()
                                    or interface_type == "SCSI"
                                    else "SSD"
                                )
                            elif (
                                "SSD" in media_type.upper()
                                or "Solid State" in media_type
                            ):
                                storage_info["storage_type"] = "SSD"
                            elif (
                                "Fixed hard disk" in media_type
                                or "HDD" in media_type.upper()
                            ):
                                storage_info["storage_type"] = "HDD"
                            else:
                                # For modern drives, especially with SCSI interface, likely NVMe
                                if interface_type == "SCSI" and model:
                                    storage_info["storage_type"] = "NVMe SSD"
                                else:
                                    storage_info["storage_type"] = "SSD"

        except Exception as e:
            print(f"Error getting storage details: {e}")

        return storage_info

    def get_memory_info(self):
        """Get detailed memory information including RAM specifications."""
        try:
            memory = psutil.virtual_memory()

            # Get detailed RAM information using WMI
            ram_details = self._get_ram_details()

            result = {
                "total_gb": memory.total / (1024**3),
                "percent": memory.percent,
                "used_gb": memory.used / (1024**3),
                "available_gb": memory.available / (1024**3),
            }

            # Add detailed RAM specifications
            result.update(ram_details)

            return result
        except Exception as e:
            print(f"Error getting memory info: {e}")
            return {
                "total_gb": 0,
                "percent": 0,
                "used_gb": 0,
                "available_gb": 0,
                "ram_name": "Unknown",
                "ram_speed": "Unknown",
                "ram_type": "Unknown",
                "ram_slots": "Unknown",
            }

    def get_disk_info(self):
        """Get C: drive specific information."""
        try:
            # C: Drive specific information
            disk = psutil.disk_usage("C:\\")

            # Get detailed storage device information for C: drive
            storage_details = self._get_storage_details()

            result = {
                "c_total_gb": disk.total / (1024**3),
                "c_usage_percent": (disk.used / disk.total) * 100,
                "c_used_gb": disk.used / (1024**3),
                "c_free_gb": disk.free / (1024**3),
                "storage_name": storage_details.get("storage_name", "Unknown"),
                "storage_type": storage_details.get("storage_type", "Unknown"),
            }

            return result
        except Exception as e:
            print(f"Error getting disk info: {e}")
            return {
                "c_total_gb": 0,
                "c_usage_percent": 0,
                "c_used_gb": 0,
                "c_free_gb": 0,
                "storage_name": "Unknown",
                "storage_type": "Unknown",
            }

    def get_storage_overview_info(self):
        """Get physical disk information with usage data."""
        try:
            # Get physical disk information using PowerShell
            drives_info = []

            try:
                # Get physical disk information
                cmd = [
                    "powershell",
                    "-Command",
                    "Get-WmiObject -Class Win32_DiskDrive | Select-Object Model, Size, MediaType, InterfaceType, Index | ConvertTo-Json",
                ]
                result = subprocess.run(
                    cmd,
                    capture_output=True,
                    text=True,
                    timeout=10,
                    creationflags=subprocess.CREATE_NO_WINDOW,
                )

                if result.returncode == 0 and result.stdout.strip():
                    import json

                    disk_data = json.loads(result.stdout.strip())

                    # Handle both single disk and multiple disks
                    if isinstance(disk_data, dict):
                        disk_data = [disk_data]

                    # Get partition information for usage calculation
                    partitions = psutil.disk_partitions()

                    disk_index = 0
                    for disk in disk_data:
                        model = disk.get("Model", "Unknown Disk").strip()
                        size_bytes = disk.get("Size", 0)
                        media_type = disk.get("MediaType", "").strip()
                        interface_type = disk.get("InterfaceType", "").strip()

                        # Convert size to GB
                        size_gb = int(size_bytes) / (1024**3) if size_bytes else 0

                        # Determine disk type
                        if any(
                            indicator in model.upper()
                            for indicator in ["SSD", "NVME", "M.2", "SOLID STATE"]
                        ):
                            disk_type = (
                                "NVMe SSD"
                                if "NVME" in model.upper() or interface_type == "SCSI"
                                else "SSD"
                            )
                        elif "Fixed hard disk" in media_type:
                            disk_type = "HDD"
                        else:
                            disk_type = "SSD" if interface_type == "SCSI" else "HDD"

                        drives_info.append(
                            {
                                "letter": f"Storage {disk_index + 1}",
                                "name": model,
                                "total_gb": size_gb,
                                "type": disk_type,
                            }
                        )

                        disk_index += 1

            except Exception as e:
                print(f"Error getting physical disk info: {e}")

            result = {
                "drives_info": drives_info,
                "total_storage_gb": sum(drive["total_gb"] for drive in drives_info),
            }

            return result
        except Exception as e:
            print(f"Error getting storage overview info: {e}")
            return {
                "drives_info": [],
                "total_storage_gb": 0,
            }

    def get_gpu_info(self):
        """Get GPU information with caching to reduce nvidia-smi calls."""
        import time
        current_time = time.time()
        
        # Cache GPU info for 10 seconds to reduce nvidia-smi overhead
        if (self._gpu_cache is not None and 
            current_time - self._last_gpu_check < 10):
            return self._gpu_cache
            
        try:
            # Try to get NVIDIA GPU info
            try:
                result = subprocess.run(
                    [
                        "nvidia-smi",
                        "--query-gpu=name,utilization.gpu,memory.used,memory.total,temperature.gpu",
                        "--format=csv,noheader,nounits",
                    ],
                    capture_output=True,
                    text=True,
                    timeout=3,  # Reduced timeout from 5s to 3s
                    creationflags=subprocess.CREATE_NO_WINDOW,
                )

                if result.returncode == 0 and result.stdout.strip():
                    gpu_data = result.stdout.strip().split(", ")
                    if len(gpu_data) >= 5:
                        gpu_info = {
                            "available": True,
                            "name": gpu_data[0],
                            "usage": float(gpu_data[1]),
                            "memory_used_gb": float(gpu_data[2]) / 1024,
                            "memory_total_gb": float(gpu_data[3]) / 1024,
                            "temperature": float(gpu_data[4]),
                        }
                        # Cache the result
                        self._gpu_cache = gpu_info
                        self._last_gpu_check = current_time
                        return gpu_info
            except (subprocess.TimeoutExpired, FileNotFoundError, ValueError):
                pass

            # No NVIDIA GPU found - cache this result too
            gpu_info = {
                "available": False,
                "name": "No NVIDIA GPU detected",
                "usage": 0,
                "memory_used_gb": 0,
                "memory_total_gb": 0,
                "temperature": 0,
            }
            self._gpu_cache = gpu_info
            self._last_gpu_check = current_time
            return gpu_info

        except Exception as e:
            print(f"Error getting GPU info: {e}")
            gpu_info = {
                "available": False,
                "name": "Error",
                "usage": 0,
                "memory_used_gb": 0,
                "memory_total_gb": 0,
                "temperature": 0,
            }
            self._gpu_cache = gpu_info
            self._last_gpu_check = current_time
            return gpu_info

    def get_monitor_info(self):
        """Get comprehensive monitor information with optimized single PowerShell command."""
        # Return cached data if available
        if self._monitor_info_cache is not None:
            return self._monitor_info_cache

        try:
            # Combined PowerShell command for all monitor information
            combined_cmd = """
            # Get monitor details
            $monitors = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorID
            Write-Output "MONITOR_DETAILS_START"
            foreach ($monitor in $monitors) {
                $name = [System.Text.Encoding]::ASCII.GetString($monitor.UserFriendlyName).TrimEnd([char]0)
                $manufacturer = [System.Text.Encoding]::ASCII.GetString($monitor.ManufacturerName).TrimEnd([char]0)
                $product = [System.Text.Encoding]::ASCII.GetString($monitor.ProductCodeID).TrimEnd([char]0)
                Write-Output "$manufacturer|$product|$name"
            }
            
            # Get screen resolution and primary info
            Write-Output "SCREEN_INFO_START"
            Add-Type -AssemblyName System.Windows.Forms
            $screens = [System.Windows.Forms.Screen]::AllScreens
            for ($i = 0; $i -lt $screens.Count; $i++) {
                $screen = $screens[$i]
                $isPrimary = if ($screen.Primary) { "Primary" } else { "Secondary" }
                Write-Output "$($screen.Bounds.Width)x$($screen.Bounds.Height)|$isPrimary"
            }
            
            # Get refresh rates
            Write-Output "REFRESH_RATES_START"
            Get-CimInstance -ClassName Win32_VideoController | Where-Object { $_.CurrentRefreshRate -ne $null } | ForEach-Object { Write-Output $_.CurrentRefreshRate }
            """

            # Execute combined command
            result = subprocess.run(
                ["powershell", "-Command", combined_cmd],
                capture_output=True,
                text=True,
                timeout=15,
                creationflags=subprocess.CREATE_NO_WINDOW,
            )

            monitors = []
            monitor_details = []
            screen_info = []
            refresh_rates = []

            # Parse combined output
            if result.returncode == 0 and result.stdout.strip():
                lines = result.stdout.strip().split("\n")
                current_section = None

                for line in lines:
                    line = line.strip()
                    if not line:
                        continue

                    # Check for section markers
                    if line == "MONITOR_DETAILS_START":
                        current_section = "monitor_details"
                        continue
                    elif line == "SCREEN_INFO_START":
                        current_section = "screen_info"
                        continue
                    elif line == "REFRESH_RATES_START":
                        current_section = "refresh_rates"
                        continue

                    # Parse data based on current section
                    if current_section == "monitor_details" and "|" in line:
                        parts = line.split("|")
                        if len(parts) >= 3:
                            manufacturer = parts[0].strip()
                            model = parts[1].strip()
                            name = parts[2].strip()
                            monitor_details.append(
                                {
                                    "manufacturer": manufacturer,
                                    "model": model,
                                    "name": name,
                                }
                            )
                    elif current_section == "screen_info" and "|" in line:
                        parts = line.split("|")
                        if len(parts) >= 2:
                            resolution = parts[0].strip()
                            is_primary = parts[1].strip()
                            screen_info.append(
                                {"resolution": resolution, "is_primary": is_primary}
                            )
                    elif current_section == "refresh_rates" and line.isdigit():
                        refresh_rates.append(int(line))

            # Combine information
            monitor_count = max(len(monitor_details), len(screen_info))

            for i in range(monitor_count):
                # Get monitor details
                if i < len(monitor_details):
                    detail = monitor_details[i]
                    manufacturer = detail["manufacturer"]
                    model = detail["model"]
                    name = detail["name"]
                else:
                    manufacturer = "Unknown"
                    model = "Unknown"
                    name = "Unknown Monitor"

                # Get screen info
                if i < len(screen_info):
                    screen = screen_info[i]
                    resolution = screen["resolution"]
                    is_primary = screen["is_primary"]
                else:
                    resolution = "Unknown"
                    is_primary = "Secondary"

                # Get refresh rate
                refresh_rate = refresh_rates[i] if i < len(refresh_rates) else "Unknown"

                # Format monitor information
                primary_text = " (Primary)" if is_primary == "Primary" else ""
                refresh_text = (
                    f" @ {refresh_rate}Hz" if refresh_rate != "Unknown" else ""
                )

                # Create consistent format: ManufacturerModel | Name | Resolution
                # Remove manufacturer name from the display name if it's already included
                clean_name = name
                if manufacturer.lower() in name.lower():
                    # Remove manufacturer from name to avoid duplication
                    clean_name = name.replace(manufacturer, "").strip()

                monitor_info = f"{manufacturer}{model} | {clean_name} | {resolution}{refresh_text}{primary_text}"
                monitors.append(monitor_info)

            if not monitors:
                monitors = ["No monitors detected"]

            result = {
                "monitors": monitors,
                "count": len(monitors) if monitors != ["No monitors detected"] else 0,
            }
            # Cache the result
            self._monitor_info_cache = result
            return result
        except Exception as e:
            print(f"Error getting monitor info: {e}")
            error_result = {"monitors": ["Error detecting monitors"], "count": 0}
            self._monitor_info_cache = error_result
            return error_result

    def get_motherboard_info(self):
        """Get motherboard information with caching."""
        # Return cached data if available
        if self._motherboard_cache is not None:
            return self._motherboard_cache
            
        try:
            result = subprocess.run(
                [
                    "powershell",
                    "-Command",
                    '$mb = Get-WmiObject -Class Win32_BaseBoard; $bios = Get-WmiObject -Class Win32_BIOS; $system = Get-WmiObject -Class Win32_ComputerSystem; $cpu = Get-WmiObject -Class Win32_Processor; $memArray = Get-WmiObject -Class Win32_PhysicalMemoryArray; $memModules = Get-WmiObject -Class Win32_PhysicalMemory; $chipset = "Unknown"; if ($mb.Product -like "*B650*") { $chipset = "AMD B650" } elseif ($mb.Product -like "*X670*") { $chipset = "AMD X670" } elseif ($mb.Product -like "*B550*") { $chipset = "AMD B550" } elseif ($mb.Product -like "*X570*") { $chipset = "AMD X570" } elseif ($mb.Product -like "*B450*") { $chipset = "AMD B450" } elseif ($mb.Product -like "*X470*") { $chipset = "AMD X470" } elseif ($mb.Product -like "*Z790*") { $chipset = "Intel Z790" } elseif ($mb.Product -like "*Z690*") { $chipset = "Intel Z690" } elseif ($mb.Product -like "*B760*") { $chipset = "Intel B760" } elseif ($mb.Product -like "*B660*") { $chipset = "Intel B660" } elseif ($cpu.Name -like "*AMD*" -and $cpu.Name -like "*7000*") { $chipset = "AMD 600 Series (Estimated)" } elseif ($cpu.Name -like "*AMD*" -and $cpu.Name -like "*5000*") { $chipset = "AMD 500 Series (Estimated)" } elseif ($cpu.Name -like "*AMD*") { $chipset = "AMD (Unknown Series)" } elseif ($cpu.Name -like "*Intel*") { $chipset = "Intel (Unknown Series)" }; Write-Host "Motherboard: $($mb.Product)"; Write-Host "Manufacturer: $($mb.Manufacturer)"; Write-Host "Version: $($mb.Version)"; Write-Host "Chipset: $chipset"; Write-Host "BIOS Version: $($bios.SMBIOSBIOSVersion)"; Write-Host "BIOS Manufacturer: $($bios.Manufacturer)"; Write-Host "BIOS Date: $([DateTime]::ParseExact($bios.ReleaseDate.Substring(0,8), \'yyyyMMdd\', $null).ToString(\'MM/dd/yyyy\'))"; Write-Host "System Model: $($system.Model)"; Write-Host "Memory Slots: $($memArray.MemoryDevices)"; Write-Host "Max Memory Capacity: $([math]::Round($memArray.MaxCapacity/1024/1024)) GB"; Write-Host "Memory Slots Used: $($memModules.Count)"',
                ],
                capture_output=True,
                text=True,
                timeout=10,
                creationflags=subprocess.CREATE_NO_WINDOW,
            )

            motherboard_info = {}
            if result.returncode == 0 and result.stdout.strip():
                lines = result.stdout.strip().split("\n")
                for line in lines:
                    if ":" in line:
                        key, value = line.split(":", 1)
                        motherboard_info[key.strip().lower().replace(" ", "_")] = (
                            value.strip()
                        )

            result = {
                "product": motherboard_info.get("motherboard", "Unknown"),
                "manufacturer": motherboard_info.get("manufacturer", "Unknown"),
                "version": motherboard_info.get("version", "Unknown"),
                "chipset": motherboard_info.get("chipset", "Unknown"),
                "bios_version": motherboard_info.get("bios_version", "Unknown"),
                "bios_manufacturer": motherboard_info.get(
                    "bios_manufacturer", "Unknown"
                ),
                "bios_date": motherboard_info.get("bios_date", "Unknown"),
                "system_model": motherboard_info.get("system_model", "Unknown"),
                "memory_slots": motherboard_info.get("memory_slots", "Unknown"),
                "max_memory_capacity": motherboard_info.get(
                    "max_memory_capacity", "Unknown"
                ),
                "memory_slots_used": motherboard_info.get(
                    "memory_slots_used", "Unknown"
                ),
            }
            # Cache the result
            self._motherboard_cache = result
            return result
        except Exception as e:
            print(f"Error getting motherboard info: {e}")
            error_result = {
                "product": "Error",
                "manufacturer": "Error",
                "version": "Error",
                "chipset": "Error",
                "bios_version": "Error",
                "bios_manufacturer": "Error",
                "bios_date": "Error",
                "system_model": "Error",
                "memory_slots": "Error",
                "max_memory_capacity": "Error",
                "memory_slots_used": "Error",
            }
            # Cache the error result
            self._motherboard_cache = error_result
            return error_result

    def get_os_info_all(self):
        """Get all OS information."""
        try:
            # Device name
            if self._device_name_cache is None:
                try:
                    import socket

                    self._device_name_cache = socket.gethostname()
                except:
                    self._device_name_cache = "Unknown"

            return {
                "device_name": self._device_name_cache,
                "user_name": self._get_os_info("user_name"),
                "edition": self._get_os_info("edition"),
                "version": self._get_os_info("version"),
                "build": self._get_os_info("build"),
                "install_date": self._get_os_info("install_date"),
                "experience": self._get_os_info("experience"),
                "arch": self._get_os_info("arch"),
            }
        except Exception as e:
            print(f"Error getting OS info: {e}")
            return {
                "device_name": "Unknown",
                "user_name": "Unknown",
                "edition": "Unknown",
                "version": "Unknown",
                "build": "Unknown",
                "install_date": "Unknown",
                "experience": "Unknown",
                "arch": "Unknown",
            }

    def _get_os_info(self, key):
        """Get specific OS information with caching."""
        if key in self._os_info_cache:
            return self._os_info_cache[key]

        try:
            if key == "user_name":
                try:
                    import getpass

                    value = getpass.getuser()
                except:
                    value = "Unknown"

            elif key == "edition":
                try:
                    # Get Windows edition and build number to determine correct version
                    edition_result = subprocess.run(
                        [
                            "powershell",
                            "-Command",
                            '(Get-ItemProperty "HKLM:SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion").ProductName',
                        ],
                        capture_output=True,
                        text=True,
                        timeout=5,
                        creationflags=subprocess.CREATE_NO_WINDOW,
                    )

                    build_result = subprocess.run(
                        [
                            "powershell",
                            "-Command",
                            '(Get-ItemProperty "HKLM:SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion").CurrentBuild',
                        ],
                        capture_output=True,
                        text=True,
                        timeout=5,
                        creationflags=subprocess.CREATE_NO_WINDOW,
                    )

                    if edition_result.returncode == 0 and edition_result.stdout.strip():
                        edition = edition_result.stdout.strip()

                        # Check build number to determine if it's Windows 11
                        if build_result.returncode == 0 and build_result.stdout.strip():
                            build_number = int(build_result.stdout.strip())
                            # Windows 11 starts from build 22000
                            if build_number >= 22000:
                                # Replace "Windows 10" with "Windows 11" if build indicates Windows 11
                                if "Windows 10" in edition:
                                    edition = edition.replace(
                                        "Windows 10", "Windows 11"
                                    )

                        value = edition
                    else:
                        value = f"{platform.system()} {platform.release()}"
                except:
                    value = f"{platform.system()} {platform.release()}"

            elif key == "version":
                try:
                    result = subprocess.run(
                        [
                            "powershell",
                            "-Command",
                            '(Get-ItemProperty "HKLM:SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion").DisplayVersion',
                        ],
                        capture_output=True,
                        text=True,
                        timeout=5,
                        creationflags=subprocess.CREATE_NO_WINDOW,
                    )

                    if result.returncode == 0 and result.stdout.strip():
                        value = result.stdout.strip()
                    else:
                        value = platform.release()
                except:
                    value = platform.release()

            elif key == "build":
                try:
                    result = subprocess.run(
                        [
                            "powershell",
                            "-Command",
                            '(Get-ItemProperty "HKLM:SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion").CurrentBuild + "." + (Get-ItemProperty "HKLM:SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion").UBR',
                        ],
                        capture_output=True,
                        text=True,
                        timeout=5,
                        creationflags=subprocess.CREATE_NO_WINDOW,
                    )

                    if result.returncode == 0 and result.stdout.strip():
                        value = result.stdout.strip()
                    else:
                        value = platform.version()
                except:
                    value = platform.version()

            elif key == "install_date":
                try:
                    result = subprocess.run(
                        [
                            "powershell",
                            "-Command",
                            '[DateTime]::FromFileTime((Get-ItemProperty "HKLM:SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion").InstallDate).ToString("MM/dd/yyyy")',
                        ],
                        capture_output=True,
                        text=True,
                        timeout=5,
                        creationflags=subprocess.CREATE_NO_WINDOW,
                    )

                    if result.returncode == 0 and result.stdout.strip():
                        value = result.stdout.strip()
                    else:
                        value = "Unknown"
                except:
                    value = "Unknown"

            elif key == "experience":
                try:
                    result = subprocess.run(
                        [
                            "powershell",
                            "-Command",
                            'Get-AppxPackage -Name "MicrosoftWindows.Client.WebExperience" | Select-Object -ExpandProperty Version',
                        ],
                        capture_output=True,
                        text=True,
                        timeout=5,
                        creationflags=subprocess.CREATE_NO_WINDOW,
                    )

                    if result.returncode == 0 and result.stdout.strip():
                        version = result.stdout.strip()
                        value = f"Windows Feature Experience Pack {version}"
                    else:
                        # Fallback method
                        result = subprocess.run(
                            [
                                "powershell",
                                "-Command",
                                '(Get-ItemProperty "HKLM:SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion").UBR',
                            ],
                            capture_output=True,
                            text=True,
                            timeout=5,
                            creationflags=subprocess.CREATE_NO_WINDOW,
                        )

                        if result.returncode == 0 and result.stdout.strip():
                            ubr = result.stdout.strip()
                            value = (
                                f"Windows Feature Experience Pack 1000.26100.{ubr}.0"
                            )
                        else:
                            value = "Windows Feature Experience Pack"
                except:
                    value = "Windows Feature Experience Pack"

            elif key == "arch":
                value = platform.architecture()[0]
            else:
                value = "Unknown"

            self._os_info_cache[key] = value
            return value
        except:
            self._os_info_cache[key] = "Unknown"
            return "Unknown"


class SystemInfoWidget(QWidget):
    """Widget for displaying detailed system information."""

    def __init__(self):
        super().__init__()
        self.init_ui()
        self.setup_async_loader()
        self.show_loading_states()

    def setup_async_loader(self):
        """Setup the background thread for loading system information."""
        self.loader = SystemInfoLoader()

        # Initialize static info loading counter
        self.static_info_loaded_count = 0
        self.static_info_total = 4  # CPU, Monitor, Motherboard, OS

        # Connect signals
        self.loader.uptime_updated.connect(self.update_uptime_display)
        self.loader.cpu_info_updated.connect(self.update_cpu_display)
        self.loader.memory_info_updated.connect(self.update_memory_display)
        self.loader.disk_info_updated.connect(self.update_disk_display)
        self.loader.storage_overview_updated.connect(
            self.update_storage_overview_display
        )
        self.loader.gpu_info_updated.connect(self.update_gpu_display)
        self.loader.monitor_info_updated.connect(self.update_monitor_display)
        self.loader.motherboard_info_updated.connect(self.update_motherboard_display)
        self.loader.os_info_updated.connect(self.update_os_display)

        # Start the loader
        self.loader.start()

    def show_loading_states(self):
        """Show loading states for all information."""
        # Set all labels to show "Loading..." initially
        self.uptime_label.setText("Loading...")

    def clear_cache(self):
        """Clear all cached system information to force refresh."""
        if hasattr(self, "loader"):
            self.loader._cpu_name_cache = None
            self.loader._device_name_cache = None
            self.loader._os_info_cache = {}
            # Clear static info caches
            self.loader._cpu_info_cache = None
            self.loader._monitor_info_cache = None
            self.loader._motherboard_info_cache = None
            self.loader._os_info_full_cache = None

    def closeEvent(self, event):
        """Handle widget close event."""
        if hasattr(self, "loader"):
            self.loader.stop()
        super().closeEvent(event)

    def init_ui(self):
        """Initialize the user interface."""
        layout = QVBoxLayout(self)
        layout.setSpacing(15)
        layout.setContentsMargins(20, 20, 20, 20)

        # Create scroll area for better handling of content
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.Shape.NoFrame)

        # Main content widget
        content_widget = QWidget()
        content_layout = QVBoxLayout(content_widget)
        content_layout.setSpacing(20)

        # Add system information sections in requested order
        content_layout.addWidget(self.create_uptime_section())  # Uptime first
        content_layout.addWidget(self.create_copy_info_section())  # Copy Info button
        content_layout.addWidget(self.create_cpu_section())  # Processor
        content_layout.addWidget(self.create_disk_section())  # Local Disk
        content_layout.addWidget(self.create_memory_section())  # Memory
        content_layout.addWidget(self.create_storage_overview_section())  # Storage
        content_layout.addWidget(self.create_gpu_section())  # Graphics
        content_layout.addWidget(self.create_monitor_section())  # Monitor
        content_layout.addWidget(self.create_motherboard_section())  # Motherboard
        content_layout.addWidget(self.create_os_section())  # OS last

        content_layout.addStretch()

        scroll.setWidget(content_widget)
        layout.addWidget(scroll)

    def create_section_frame(self, title, icon=""):
        """Create a styled frame for each section."""
        frame = QGroupBox(f"{icon} {title}")
        frame.setStyleSheet(
            """
            QGroupBox {
                font-size: 14px;
                font-weight: bold;
                color: #e2e8f0;
                border: 2px solid #4a5568;
                border-radius: 8px;
                margin-top: 10px;
                padding-top: 10px;
                background: #2d3748;
            }
            QGroupBox::title {
                subcontrol-origin: margin;
                left: 10px;
                padding: 0 8px 0 8px;
                color: #63b3ed;
                background: #2d3748;
            }
        """
        )
        return frame

    def create_info_row(self, label_text, value_text="", progress_value=None):
        """Create a row with label and value/progress bar."""
        row_widget = QWidget()
        layout = QHBoxLayout(row_widget)
        layout.setContentsMargins(0, 5, 0, 5)

        # Label
        label = QLabel(label_text)
        label.setStyleSheet("color: #cbd5e0; font-size: 12px; font-weight: normal;")
        label.setMinimumWidth(120)
        layout.addWidget(label)

        if progress_value is not None:
            # Progress bar
            progress = QProgressBar()
            progress.setValue(int(progress_value))
            progress.setStyleSheet(
                """
                QProgressBar {
                    border: 1px solid #4a5568;
                    border-radius: 4px;
                    text-align: center;
                    background: #1a202c;
                    color: #e2e8f0;
                    font-size: 11px;
                }
                QProgressBar::chunk {
                    background: qlineargradient(x1:0, y1:0, x2:1, y2:0,
                        stop:0 #38a169, stop:0.5 #48bb78, stop:1 #68d391);
                    border-radius: 3px;
                }
            """
            )
            progress.setMaximumHeight(20)
            layout.addWidget(progress, 2)

            # Value label
            value_label = QLabel(value_text)
            value_label.setStyleSheet(
                "color: #e2e8f0; font-size: 12px; font-weight: bold;"
            )
            value_label.setMinimumWidth(80)
            value_label.setAlignment(Qt.AlignmentFlag.AlignRight)
            layout.addWidget(value_label)
        else:
            # Just value text
            value_label = QLabel(value_text)
            value_label.setStyleSheet("color: #e2e8f0; font-size: 12px;")
            value_label.setAlignment(Qt.AlignmentFlag.AlignRight)
            layout.addWidget(value_label, 1)

        return row_widget

    def create_uptime_section(self):
        """Create uptime information section."""
        frame = self.create_section_frame("System Uptime", "⏱️")
        layout = QVBoxLayout(frame)

        self.uptime_label = QLabel("Loading...")
        self.uptime_label.setStyleSheet(
            """
            color: #63b3ed; 
            font-size: 16px; 
            font-weight: bold; 
            padding: 10px;
            background: #1a202c;
            border-radius: 6px;
            border: 1px solid #4a5568;
        """
        )
        self.uptime_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(self.uptime_label)

        return frame

    def create_cpu_section(self):
        """Create CPU information section."""
        frame = self.create_section_frame("Processor Information", "🔥")
        layout = QVBoxLayout(frame)

        self.cpu_name_row = self.create_info_row("Processor:", "Loading...")
        self.cpu_cores_row = self.create_info_row("Cores/Threads:", "Loading...")
        self.cpu_freq_row = self.create_info_row("Frequency:", "Loading...")
        self.cpu_cache_row = self.create_info_row("Cache:", "Loading...")
        self.cpu_sockets_row = self.create_info_row("Sockets:", "Loading...")
        self.cpu_usage_row = self.create_info_row("Usage:", "0%", 0)

        layout.addWidget(self.cpu_name_row)
        layout.addWidget(self.cpu_cores_row)
        layout.addWidget(self.cpu_freq_row)
        layout.addWidget(self.cpu_cache_row)
        layout.addWidget(self.cpu_sockets_row)
        layout.addWidget(self.cpu_usage_row)

        return frame

    def create_memory_section(self):
        """Create memory information section."""
        frame = self.create_section_frame("Memory Information", "💾")
        layout = QVBoxLayout(frame)

        self.memory_total_row = self.create_info_row("Total Memory:", "Loading...")
        self.memory_usage_row = self.create_info_row("Usage:", "0%", 0)
        self.memory_used_row = self.create_info_row("Used:", "Loading...")
        self.memory_available_row = self.create_info_row("Available:", "Loading...")

        # Add detailed RAM specifications
        self.ram_name_row = self.create_info_row("RAM Name:", "Loading...")
        self.ram_type_row = self.create_info_row("RAM Type:", "Loading...")
        self.ram_speed_row = self.create_info_row("RAM Speed:", "Loading...")
        self.ram_slots_row = self.create_info_row("RAM Slots:", "Loading...")

        layout.addWidget(self.memory_total_row)
        layout.addWidget(self.memory_usage_row)
        layout.addWidget(self.memory_used_row)
        layout.addWidget(self.memory_available_row)
        layout.addWidget(self.ram_name_row)
        layout.addWidget(self.ram_type_row)
        layout.addWidget(self.ram_speed_row)
        layout.addWidget(self.ram_slots_row)

        return frame

    def create_disk_section(self):
        """Create local disk information section."""
        frame = self.create_section_frame("Local Disk (C:)", "💿")
        layout = QVBoxLayout(frame)

        # Storage device info for C: drive
        self.storage_name_row = self.create_info_row("Storage Device:", "Loading...")
        self.storage_type_row = self.create_info_row("Storage Type:", "Loading...")
        self.disk_total_row = self.create_info_row("Local Disk:", "Loading...")
        self.disk_usage_row = self.create_info_row("C: Usage:", "0%", 0)
        self.disk_free_row = self.create_info_row("C: Free:", "Loading...")

        layout.addWidget(self.storage_name_row)
        layout.addWidget(self.storage_type_row)
        layout.addWidget(self.disk_total_row)
        layout.addWidget(self.disk_usage_row)
        layout.addWidget(self.disk_free_row)

        return frame

    def create_storage_overview_section(self):
        """Create storage overview section for all drives."""
        frame = self.create_section_frame("Storage Information", "🗄️")
        layout = QVBoxLayout(frame)

        # Storage count
        self.storage_count_row = self.create_info_row(
            "Total Storage Count:", "Loading..."
        )
        layout.addWidget(self.storage_count_row)

        # Store layout and frame for dynamic row creation
        self.storage_layout = layout
        self.storage_frame = frame
        self.storage_rows = []  # Will store dynamically created storage rows

        return frame

    def create_gpu_section(self):
        """Create GPU information section."""
        frame = self.create_section_frame("Graphics Information", "🎮")
        layout = QVBoxLayout(frame)

        self.gpu_name_row = self.create_info_row("Graphics Card:", "Loading...")
        self.gpu_usage_row = self.create_info_row("Usage:", "0%", 0)
        self.gpu_memory_row = self.create_info_row("Memory:", "Loading...")
        self.gpu_temp_row = self.create_info_row("Temperature:", "Loading...")

        layout.addWidget(self.gpu_name_row)
        layout.addWidget(self.gpu_usage_row)
        layout.addWidget(self.gpu_memory_row)
        layout.addWidget(self.gpu_temp_row)

        return frame

    def create_os_section(self):
        """Create operating system information section."""
        frame = self.create_section_frame("Operating System", "🖥️")
        layout = QVBoxLayout(frame)

        self.device_name_row = self.create_info_row("Device Name:", "Loading...")
        self.user_name_row = self.create_info_row("User:", "Loading...")
        self.os_edition_row = self.create_info_row("Edition:", "Loading...")
        self.os_version_row = self.create_info_row("Version:", "Loading...")
        self.os_build_row = self.create_info_row("OS Build:", "Loading...")
        self.os_install_date_row = self.create_info_row("Installed on:", "Loading...")
        self.os_experience_row = self.create_info_row("Experience:", "Loading...")
        self.os_arch_row = self.create_info_row("Architecture:", "Loading...")

        layout.addWidget(self.device_name_row)
        layout.addWidget(self.user_name_row)
        layout.addWidget(self.os_edition_row)
        layout.addWidget(self.os_version_row)
        layout.addWidget(self.os_build_row)
        # layout.addWidget(self.os_install_date_row)
        layout.addWidget(self.os_experience_row)
        # layout.addWidget(self.os_arch_row)

        return frame

    def create_monitor_section(self):
        """Create monitor information section."""
        frame = self.create_section_frame("Monitor Information", "🖥️")
        layout = QVBoxLayout(frame)

        self.monitor_count_row = self.create_info_row("Monitor Count:", "Loading...")
        layout.addWidget(self.monitor_count_row)

        # Store layout and frame for dynamic row creation
        self.monitor_layout = layout
        self.monitor_frame = frame
        self.monitor_rows = []  # Will store dynamically created monitor rows

        return frame

    def create_motherboard_section(self):
        """Create motherboard information section."""
        frame = self.create_section_frame("Motherboard Information", "🔧")
        layout = QVBoxLayout(frame)

        self.motherboard_product_row = self.create_info_row("Product:", "Loading...")
        self.motherboard_manufacturer_row = self.create_info_row(
            "Manufacturer:", "Loading..."
        )
        self.motherboard_version_row = self.create_info_row("Version:", "Loading...")
        self.motherboard_chipset_row = self.create_info_row("Chipset:", "Loading...")
        self.motherboard_bios_version_row = self.create_info_row(
            "BIOS Version:", "Loading..."
        )
        self.motherboard_bios_manufacturer_row = self.create_info_row(
            "BIOS Manufacturer:", "Loading..."
        )
        self.motherboard_bios_date_row = self.create_info_row(
            "BIOS Date:", "Loading..."
        )
        self.motherboard_system_model_row = self.create_info_row(
            "System Model:", "Loading..."
        )

        # Memory slot information
        self.motherboard_memory_slots_row = self.create_info_row(
            "Total Memory Slots:", "Loading..."
        )
        self.motherboard_max_capacity_row = self.create_info_row(
            "Max Memory Capacity:", "Loading..."
        )
        self.motherboard_slots_used_row = self.create_info_row(
            "Memory Slots Used:", "Loading..."
        )

        layout.addWidget(self.motherboard_product_row)
        layout.addWidget(self.motherboard_manufacturer_row)
        layout.addWidget(self.motherboard_version_row)
        layout.addWidget(self.motherboard_chipset_row)
        layout.addWidget(self.motherboard_bios_version_row)
        layout.addWidget(self.motherboard_bios_manufacturer_row)
        layout.addWidget(self.motherboard_bios_date_row)
        layout.addWidget(self.motherboard_system_model_row)
        layout.addWidget(self.motherboard_memory_slots_row)
        layout.addWidget(self.motherboard_max_capacity_row)
        layout.addWidget(self.motherboard_slots_used_row)

        return frame

    def create_copy_info_section(self):
        """Create copy info button section."""
        frame = QGroupBox()
        frame.setStyleSheet(
            """
            QGroupBox {
                border: none;
                background: transparent;
                margin: 5px;
            }
            """
        )
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(
            10, 0, 10, 0
        )  # Remove top space, minimal bottom space

        self.copy_info_button = QPushButton("📋 Copy System Info")
        self.copy_info_button.setStyleSheet(
            """
            QPushButton {
                background: #4a90e2;
                color: white;
                border: none;
                border-radius: 8px;
                padding: 12px 24px;
                font-size: 14px;
                font-weight: bold;
                min-height: 20px;
            }
            QPushButton:hover {
                background: #357abd;
            }
            QPushButton:pressed {
                background: #2968a3;
            }
            QPushButton:disabled {
                background: #6c757d;
                color: #adb5bd;
            }
            """
        )
        self.copy_info_button.clicked.connect(self.copy_system_info)
        self.copy_info_button.setEnabled(False)  # Initially disabled

        layout.addWidget(self.copy_info_button)
        return frame

    def copy_system_info(self):
        """Copy formatted system information to clipboard."""
        try:
            # Get current CPU info
            cpu_name = self.cpu_name_row.findChildren(QLabel)[1].text()
            cpu_cores = self.cpu_cores_row.findChildren(QLabel)[1].text()
            cpu_freq = self.cpu_freq_row.findChildren(QLabel)[1].text()
            cpu_cache = self.cpu_cache_row.findChildren(QLabel)[1].text()
            cpu_sockets = self.cpu_sockets_row.findChildren(QLabel)[1].text()

            # Get current memory info
            memory_total = self.memory_total_row.findChildren(QLabel)[1].text()
            memory_used = self.memory_used_row.findChildren(QLabel)[1].text()
            memory_available = self.memory_available_row.findChildren(QLabel)[1].text()

            # Get disk info
            disk_total = self.disk_total_row.findChildren(QLabel)[1].text()
            disk_free = self.disk_free_row.findChildren(QLabel)[1].text()
            disk_usage = self.disk_usage_row.findChildren(QLabel)[1].text()

            # Get storage overview info
            storage_count = self.storage_count_row.findChildren(QLabel)[1].text()

            # Get GPU info
            gpu_name = self.gpu_name_row.findChildren(QLabel)[1].text()
            gpu_memory = self.gpu_memory_row.findChildren(QLabel)[1].text()

            # Get RAM info
            ram_name = self.ram_name_row.findChildren(QLabel)[1].text()
            ram_type = self.ram_type_row.findChildren(QLabel)[1].text()
            ram_speed = self.ram_speed_row.findChildren(QLabel)[1].text()
            ram_slots = self.ram_slots_row.findChildren(QLabel)[1].text()

            # Get storage info
            storage_name = self.storage_name_row.findChildren(QLabel)[1].text()
            storage_type = self.storage_type_row.findChildren(QLabel)[1].text()

            # Get GPU temperature
            gpu_temp = self.gpu_temp_row.findChildren(QLabel)[1].text()

            # Get OS info
            device_name = self.device_name_row.findChildren(QLabel)[1].text()
            user_name = self.user_name_row.findChildren(QLabel)[1].text()
            os_edition = self.os_edition_row.findChildren(QLabel)[1].text()
            os_version = self.os_version_row.findChildren(QLabel)[1].text()
            os_build = self.os_build_row.findChildren(QLabel)[1].text()
            os_experience = self.os_experience_row.findChildren(QLabel)[1].text()

            # Get motherboard info
            motherboard_product = self.motherboard_product_row.findChildren(QLabel)[
                1
            ].text()
            motherboard_manufacturer = self.motherboard_manufacturer_row.findChildren(
                QLabel
            )[1].text()
            motherboard_version = self.motherboard_version_row.findChildren(QLabel)[
                1
            ].text()
            motherboard_chipset = self.motherboard_chipset_row.findChildren(QLabel)[
                1
            ].text()
            motherboard_bios_version = self.motherboard_bios_version_row.findChildren(
                QLabel
            )[1].text()
            motherboard_bios_manufacturer = (
                self.motherboard_bios_manufacturer_row.findChildren(QLabel)[1].text()
            )
            motherboard_bios_date = self.motherboard_bios_date_row.findChildren(QLabel)[
                1
            ].text()
            motherboard_system_model = self.motherboard_system_model_row.findChildren(
                QLabel
            )[1].text()
            motherboard_memory_slots = self.motherboard_memory_slots_row.findChildren(
                QLabel
            )[1].text()
            motherboard_max_capacity = self.motherboard_max_capacity_row.findChildren(
                QLabel
            )[1].text()
            motherboard_slots_used = self.motherboard_slots_used_row.findChildren(
                QLabel
            )[1].text()

            # Get monitor info
            monitor_count = self.monitor_count_row.findChildren(QLabel)[1].text()

            # Get dynamic monitor information
            monitor_details = []
            for i, monitor_row in enumerate(self.monitor_rows):
                monitor_label = (
                    monitor_row.findChildren(QLabel)[0].text().replace(":", "")
                )
                monitor_value = monitor_row.findChildren(QLabel)[1].text()
                monitor_details.append(f"{monitor_label}: {monitor_value}")

            # Get dynamic storage information
            storage_details = []
            for i, storage_row in enumerate(self.storage_rows):
                storage_label = (
                    storage_row.findChildren(QLabel)[0].text().replace(":", "")
                )
                storage_value = storage_row.findChildren(QLabel)[1].text()
                storage_details.append(f"{storage_label}: {storage_value}")

            # Format the comprehensive information
            formatted_info = f"""Processor Information:
----------------------
Processor: {cpu_name}
Cores/Threads: {cpu_cores}
Frequency: {cpu_freq}
Cache: {cpu_cache}
Sockets: {cpu_sockets}

Local Disk (C:) Information:
----------------------
Storage Device: {storage_name}
Storage Type: {storage_type}
Local Disk (C:) Total: {disk_total}
Local Disk (C:) Free: {disk_free}
Local Disk (C:) Usage: {disk_usage}

Memory Information:
----------------------
Ram Total: {memory_total}
Ram Used: {memory_used}
Ram Available: {memory_available}
RAM Name: {ram_name}
RAM Type: {ram_type}
RAM Speed: {ram_speed}
RAM Slots: {ram_slots}

Storage Information:
----------------------
Total Storage Devices: {storage_count}"""

            # Add dynamic storage details
            if storage_details:
                for detail in storage_details:
                    formatted_info += f"\n{detail}"

            formatted_info += f"""

Graphics Information:
----------------------
GPU: {gpu_name}
GPU Memory: {gpu_memory}
GPU Temperature: {gpu_temp}

Monitor Information:
----------------------
Monitor Count: {monitor_count}"""

            # Add dynamic monitor details
            if monitor_details:
                for detail in monitor_details:
                    formatted_info += f"\n{detail}"

            formatted_info += f"""

Motherboard Information:
----------------------
Product: {motherboard_product}
Manufacturer: {motherboard_manufacturer}
Version: {motherboard_version}
Chipset: {motherboard_chipset}
BIOS Version: {motherboard_bios_version}
BIOS Manufacturer: {motherboard_bios_manufacturer}
BIOS Date: {motherboard_bios_date}
System Model: {motherboard_system_model}
Total Memory Slots: {motherboard_memory_slots}
Max Memory Capacity: {motherboard_max_capacity}
Memory Slots Used: {motherboard_slots_used}

Operating System Information:
----------------------
Device Name: {device_name}
User: {user_name}
Operating System: {os_edition}
OS Version: {os_version}
OS Build: {os_build}
OS Experience: {os_experience}"""

            # Copy to clipboard
            clipboard = QApplication.clipboard()
            clipboard.setText(formatted_info)

            # Temporarily change button text to show success
            original_text = self.copy_info_button.text()
            self.copy_info_button.setText("✅ Copied!")
            QTimer.singleShot(
                2000, lambda: self.copy_info_button.setText(original_text)
            )

        except Exception as e:
            print(f"Error copying system info: {e}")
            # Show error on button
            original_text = self.copy_info_button.text()
            self.copy_info_button.setText("❌ Error")
            QTimer.singleShot(
                2000, lambda: self.copy_info_button.setText(original_text)
            )

    def check_static_info_loaded(self):
        """Check if all static information is loaded and enable copy button after delay."""
        self.static_info_loaded_count += 1
        if self.static_info_loaded_count >= self.static_info_total:
            # Add 2-second delay before enabling the copy button
            QTimer.singleShot(2000, lambda: self.copy_info_button.setEnabled(True))

    # New async display update methods
    def update_uptime_display(self, uptime_str):
        """Update uptime display from background thread."""
        self.uptime_label.setText(uptime_str)

    def update_cpu_display(self, cpu_info):
        """Update CPU display from background thread."""
        try:
            if cpu_info.get("type") == "static":
                # Static CPU information
                self.cpu_name_row.findChildren(QLabel)[1].setText(
                    cpu_info.get("name", "Unknown")
                )
                self.cpu_cores_row.findChildren(QLabel)[1].setText(
                    cpu_info.get("cores", "Unknown")
                )

                # Frequency information (base and current)
                self.cpu_freq_row.findChildren(QLabel)[1].setText(
                    cpu_info.get("frequency", "Unknown")
                )

                # Combined cache information
                cache_display = cpu_info.get("cache_display", "Unknown")
                self.cpu_cache_row.findChildren(QLabel)[1].setText(cache_display)

                # Socket information
                self.cpu_sockets_row.findChildren(QLabel)[1].setText(
                    cpu_info.get("sockets", "Unknown")
                )

                # Check if all static info is loaded
                self.check_static_info_loaded()
            elif cpu_info.get("type") == "usage":
                # CPU usage information
                usage = cpu_info.get("usage", 0)
                self.cpu_usage_row.findChildren(QProgressBar)[0].setValue(int(usage))
                self.cpu_usage_row.findChildren(QLabel)[1].setText(f"{usage:.1f}%")
            elif cpu_info.get("type") == "current_speed":
                # Update current speed in frequency display
                current_speed = cpu_info.get("current_speed", "Unknown")
                # Update the frequency row with current speed info
                if hasattr(self, "cpu_freq_row"):
                    current_text = self.cpu_freq_row.findChildren(QLabel)[1].text()
                    if "(Current:" not in current_text and current_speed != "Unknown":
                        base_text = current_text.split(" (Current:")[0]
                        updated_text = f"{base_text} (Current: {current_speed})"
                        self.cpu_freq_row.findChildren(QLabel)[1].setText(updated_text)
        except Exception as e:
            print(f"Error updating CPU display: {e}")

    def update_memory_display(self, memory_info):
        """Update memory display from background thread."""
        try:
            total_gb = memory_info.get("total_gb", 0)
            percent = memory_info.get("percent", 0)
            used_gb = memory_info.get("used_gb", 0)
            available_gb = memory_info.get("available_gb", 0)

            self.memory_total_row.findChildren(QLabel)[1].setText(f"{total_gb:.1f} GB")
            self.memory_usage_row.findChildren(QProgressBar)[0].setValue(int(percent))
            self.memory_usage_row.findChildren(QLabel)[1].setText(f"{percent:.1f}%")
            self.memory_used_row.findChildren(QLabel)[1].setText(f"{used_gb:.1f} GB")
            self.memory_available_row.findChildren(QLabel)[1].setText(
                f"{available_gb:.1f} GB"
            )

            # Update detailed RAM specifications
            self.ram_name_row.findChildren(QLabel)[1].setText(
                memory_info.get("ram_name", "Unknown")
            )
            self.ram_type_row.findChildren(QLabel)[1].setText(
                memory_info.get("ram_type", "Unknown")
            )
            self.ram_speed_row.findChildren(QLabel)[1].setText(
                memory_info.get("ram_speed", "Unknown")
            )
            self.ram_slots_row.findChildren(QLabel)[1].setText(
                memory_info.get("ram_slots", "Unknown")
            )
        except Exception as e:
            print(f"Error updating memory display: {e}")

    def update_disk_display(self, disk_info):
        """Update C: drive display from background thread."""
        try:
            # Update storage device info for C: drive
            self.storage_name_row.findChildren(QLabel)[1].setText(
                disk_info.get("storage_name", "Unknown")
            )
            self.storage_type_row.findChildren(QLabel)[1].setText(
                disk_info.get("storage_type", "Unknown")
            )

            c_total_gb = disk_info.get("c_total_gb", 0)
            c_usage_percent = disk_info.get("c_usage_percent", 0)
            c_used_gb = disk_info.get("c_used_gb", 0)
            c_free_gb = disk_info.get("c_free_gb", 0)

            self.disk_total_row.findChildren(QLabel)[1].setText(
                f"{c_used_gb:.1f} GB / {c_total_gb:.1f} GB"
            )
            self.disk_usage_row.findChildren(QProgressBar)[0].setValue(
                int(c_usage_percent)
            )
            self.disk_usage_row.findChildren(QLabel)[1].setText(
                f"{c_usage_percent:.1f}%"
            )
            self.disk_free_row.findChildren(QLabel)[1].setText(f"{c_free_gb:.1f} GB")
        except Exception as e:
            print(f"Error updating disk display: {e}")

    def update_storage_overview_display(self, storage_info):
        """Update storage overview display from background thread."""
        try:
            # Get drives info
            drives_info = storage_info.get("drives_info", [])

            # Update storage count
            self.storage_count_row.findChildren(QLabel)[1].setText(
                str(len(drives_info))
            )

            # Remove existing storage rows
            for row in self.storage_rows:
                self.storage_layout.removeWidget(row)
                row.deleteLater()
            self.storage_rows.clear()

            # Create new storage rows based on actual storage count
            for i, drive in enumerate(drives_info):
                total_gb = drive.get("total_gb", 0)
                drive_name = drive.get("name", "Unknown Disk")

                # Format: "Device Name - Total GB"
                display_text = f"{drive_name} - {total_gb:.1f} GB"

                # Create new row
                storage_row = self.create_info_row(f"Storage {i + 1}:", display_text)
                self.storage_rows.append(storage_row)
                self.storage_layout.addWidget(storage_row)

        except Exception as e:
            print(f"Error updating storage overview display: {e}")

    def update_gpu_display(self, gpu_info):
        """Update GPU display from background thread."""
        try:
            if gpu_info.get("available", False):
                name = gpu_info.get("name", "Unknown")
                usage = gpu_info.get("usage", 0)
                memory_used_gb = gpu_info.get("memory_used_gb", 0)
                memory_total_gb = gpu_info.get("memory_total_gb", 0)
                temperature = gpu_info.get("temperature", 0)

                self.gpu_name_row.findChildren(QLabel)[1].setText(name)
                self.gpu_usage_row.findChildren(QProgressBar)[0].setValue(int(usage))
                self.gpu_usage_row.findChildren(QLabel)[1].setText(f"{usage:.1f}%")

                if memory_total_gb > 0:
                    memory_percent = (memory_used_gb / memory_total_gb) * 100
                    self.gpu_memory_row.findChildren(QLabel)[1].setText(
                        f"{memory_used_gb:.1f} GB / {memory_total_gb:.1f} GB"
                    )
                else:
                    memory_percent = 0
                    self.gpu_memory_row.findChildren(QLabel)[1].setText("Unknown")

                self.gpu_temp_row.findChildren(QLabel)[1].setText(
                    f"{temperature:.0f}°C"
                )
            else:
                self.gpu_name_row.findChildren(QLabel)[1].setText(
                    gpu_info.get("name", "No NVIDIA GPU detected")
                )
                self.gpu_usage_row.findChildren(QProgressBar)[0].setValue(0)
                self.gpu_usage_row.findChildren(QLabel)[1].setText("0%")
                self.gpu_memory_row.findChildren(QLabel)[1].setText("N/A")
                self.gpu_temp_row.findChildren(QLabel)[1].setText("N/A")
        except Exception as e:
            print(f"Error updating GPU display: {e}")

    def update_os_display(self, os_info):
        """Update OS display from background thread."""
        try:
            self.device_name_row.findChildren(QLabel)[1].setText(
                os_info.get("device_name", "Unknown")
            )
            self.user_name_row.findChildren(QLabel)[1].setText(
                os_info.get("user_name", "Unknown")
            )
            self.os_edition_row.findChildren(QLabel)[1].setText(
                os_info.get("edition", "Unknown")
            )
            self.os_version_row.findChildren(QLabel)[1].setText(
                os_info.get("version", "Unknown")
            )
            self.os_build_row.findChildren(QLabel)[1].setText(
                os_info.get("build", "Unknown")
            )
            self.os_experience_row.findChildren(QLabel)[1].setText(
                os_info.get("experience", "Unknown")
            )
            # Check if all static info is loaded
            self.check_static_info_loaded()
        except Exception as e:
            print(f"Error updating OS display: {e}")

    def update_monitor_display(self, monitor_info):
        """Update monitor display from background thread."""
        try:
            monitors = monitor_info.get("monitors", [])
            count = monitor_info.get("count", 0)

            self.monitor_count_row.findChildren(QLabel)[1].setText(str(count))

            # Remove existing monitor rows
            for row in self.monitor_rows:
                self.monitor_layout.removeWidget(row)
                row.deleteLater()
            self.monitor_rows.clear()

            # Create new monitor rows based on actual monitor count
            for i, monitor in enumerate(monitors):
                if (
                    monitor != "No monitors detected"
                    and monitor != "Error detecting monitors"
                ):
                    # Clean up monitor name by removing prefixes
                    monitor_name = monitor.replace("Main Monitor: ", "").replace(
                        f"Monitor {i+1}: ", ""
                    )

                    # Create new row
                    monitor_row = self.create_info_row(
                        f"Monitor {i + 1}:", monitor_name
                    )

                    self.monitor_rows.append(monitor_row)
                    self.monitor_layout.addWidget(monitor_row)

            # Check if all static info is loaded
            self.check_static_info_loaded()

        except Exception as e:
            print(f"Error updating monitor display: {e}")

    def update_motherboard_display(self, motherboard_info):
        """Update motherboard display from background thread."""
        try:
            self.motherboard_product_row.findChildren(QLabel)[1].setText(
                motherboard_info.get("product", "Unknown")
            )
            self.motherboard_manufacturer_row.findChildren(QLabel)[1].setText(
                motherboard_info.get("manufacturer", "Unknown")
            )
            self.motherboard_version_row.findChildren(QLabel)[1].setText(
                motherboard_info.get("version", "Unknown")
            )
            self.motherboard_chipset_row.findChildren(QLabel)[1].setText(
                motherboard_info.get("chipset", "Unknown")
            )
            self.motherboard_bios_version_row.findChildren(QLabel)[1].setText(
                motherboard_info.get("bios_version", "Unknown")
            )
            self.motherboard_bios_manufacturer_row.findChildren(QLabel)[1].setText(
                motherboard_info.get("bios_manufacturer", "Unknown")
            )
            self.motherboard_bios_date_row.findChildren(QLabel)[1].setText(
                motherboard_info.get("bios_date", "Unknown")
            )
            self.motherboard_system_model_row.findChildren(QLabel)[1].setText(
                motherboard_info.get("system_model", "Unknown")
            )

            # Update memory slot information
            self.motherboard_memory_slots_row.findChildren(QLabel)[1].setText(
                str(motherboard_info.get("memory_slots", "Unknown"))
            )
            self.motherboard_max_capacity_row.findChildren(QLabel)[1].setText(
                motherboard_info.get("max_memory_capacity", "Unknown")
            )
            self.motherboard_slots_used_row.findChildren(QLabel)[1].setText(
                str(motherboard_info.get("memory_slots_used", "Unknown"))
            )
            # Check if all static info is loaded
            self.check_static_info_loaded()
        except Exception as e:
            print(f"Error updating motherboard display: {e}")
