import sys
import os
from PyQt6.QtWidgets import (
    QApplication,
    QMainWindow,
    QVBoxLayout,
    QHBoxLayout,
    QGridLayout,
    QWidget,
    QPushButton,
    QTextEdit,
    QLabel,
    QTabWidget,
    QProgressBar,
    QGroupBox,
    QSpinBox,
    QComboBox,
    QMessageBox,
    QFrame,
    QSystemTrayIcon,
)
from PyQt6.QtCore import Qt, QUrl, QTimer
from PyQt6.QtGui import QIcon, QFont, QDesktopServices, QCursor, QCloseEvent

from ui.themes import ModernDarkTheme
from modules.system_monitor import SystemInfoThread, SystemMonitor
from modules.system_cleaner import SystemCleaner
from modules.power_manager import PowerManager
from modules.quick_actions import QuickActions
from modules.system_tray import SystemTrayManager
from modules.system_info_widget import SystemInfoWidget


def resource_path(relative_path):
    if hasattr(sys, "_MEIPASS"):
        return os.path.join(sys._MEIPASS, relative_path)
    return os.path.join(os.path.abspath("."), relative_path)


class PCToolkit(QMainWindow):

    def __init__(self):
        super().__init__()
        self.setWindowTitle("PC Toolkit Pro - Advanced System Manager")
        self.setGeometry(100, 100, 650, 480)
        self.setMinimumSize(600, 600)
        self.setMaximumSize(800, 800)
        self.setWindowIcon(QIcon(resource_path("icon.ico")))

        self.system_cleaner = SystemCleaner(self.log_message, self.update_status)
        self.power_manager = PowerManager(self)
        self.quick_actions = QuickActions(self)

        ModernDarkTheme.apply_theme(self)

        self.system_thread = SystemInfoThread()
        self.system_thread.info_updated.connect(self.update_system_info)
        self.system_thread.start()

        # Countdown timer for integrated display
        self.countdown_timer = QTimer()
        self.countdown_timer.timeout.connect(self.update_countdown_display)
        self.shutdown_time = None

        self.setup_system_tray()

        central_widget = QWidget()
        self.setCentralWidget(central_widget)
        self.init_ui(central_widget)

        if self.tray_icon.isVisible():
            self.tray_icon.show_notification(
                "PC Toolkit Pro",
                "Running in system tray. Single-click to open.",
                QSystemTrayIcon.MessageIcon.Information,
                2000,
            )

    def init_ui(self, central_widget):
        layout = QVBoxLayout(central_widget)

        self.uptime_widget = QWidget()
        uptime_layout = QHBoxLayout(self.uptime_widget)
        uptime_layout.setContentsMargins(15, 8, 15, 8)

        uptime_layout.addWidget(QLabel("PC Up Time :"))
        uptime_layout.addStretch()

        self.uptime_display = QLabel("0 hours 0 mins")
        self.uptime_display.setStyleSheet(
            """
            QLabel {
                font-family: 'Segoe UI', Arial, sans-serif;
                font-size: 13px;
                font-weight: normal;
                color: #ffffff;
            }
        """
        )
        uptime_layout.addWidget(self.uptime_display)

        self.uptime_widget.setStyleSheet(
            """
            QWidget {
                background-color: #2d2d2d;
                border: 1px solid #404040;
                border-radius: 5px;
            }
        """
        )

        layout.addWidget(self.uptime_widget)

        self.tabs = QTabWidget()
        self.tabs.setStyleSheet(ModernDarkTheme.get_tab_style())

        self.create_system_tab()
        self.create_cleaner_tab()
        self.create_power_tab()

        # Lazy loading for system info tab
        self.system_info_widget = None
        self.tabs.addTab(QWidget(), "🖥️ Information")
        self.tabs.currentChanged.connect(self.on_tab_changed)

        layout.addWidget(self.tabs)

        copyright_label = QLabel(
            "Developed by SSujitX | <a href='https://github.com/SSujitX' style='color: #4a9eff; text-decoration: none;'>github.com/SSujitX</a> | © 2025"
        )
        copyright_label.setStyleSheet(
            """
            QLabel {
                font-family: 'Segoe UI', Arial, sans-serif;
                font-size: 9px;
                color: #666666;
                background-color: transparent;
                border: none;
                padding: 8px;
                margin: 2px;
            }
        """
        )
        copyright_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        copyright_label.setOpenExternalLinks(True)
        copyright_label.setCursor(QCursor(Qt.CursorShape.PointingHandCursor))
        layout.addWidget(copyright_label)

        layout.setContentsMargins(8, 8, 8, 8)
        layout.setSpacing(8)

    def setup_system_tray(self):
        if not QSystemTrayIcon.isSystemTrayAvailable():
            QMessageBox.critical(
                self, "System Tray", "System tray is not available on this system."
            )
            return

        self.tray_icon = SystemTrayManager(self)

        self.tray_icon.show_main_window.connect(self.show_from_tray)
        self.tray_icon.quick_shutdown.connect(self.power_manager.shutdown_now)
        self.tray_icon.quick_restart.connect(self.power_manager.restart_now)
        self.tray_icon.quick_sleep.connect(self.power_manager.sleep_now)
        self.tray_icon.quick_lock.connect(self.power_manager.lock_screen)
        self.tray_icon.open_task_manager.connect(self.quick_actions.open_task_manager)
        self.tray_icon.open_system_cleaner.connect(self.open_cleaner_tab)

        self.tray_icon.show()

    def show_from_tray(self):
        self.show()
        self.raise_()
        self.activateWindow()

    def open_cleaner_tab(self):
        self.show_from_tray()
        self.tabs.setCurrentIndex(1)

    def changeEvent(self, event):
        if event.type() == event.Type.WindowStateChange:
            if self.isMinimized():
                pass
        super().changeEvent(event)

    def closeEvent(self, event: QCloseEvent):
        if self.tray_icon.isVisible():
            self.hide()
            event.ignore()
        else:
            event.accept()

    def create_system_tab(self):
        system_widget = QWidget()
        layout = QVBoxLayout()

        info_group = QGroupBox("💻 Information")
        info_layout = QGridLayout()

        self.cpu_label = QLabel("CPU: Loading...")
        self.memory_label = QLabel("Memory: Loading...")
        self.disk_label = QLabel("Disk C: Loading...")
        self.gpu_label = QLabel("GPU: Checking...")

        system_info = SystemMonitor.get_system_info()
        self.os_label = QLabel(f"OS: {system_info['os']}")

        info_layout.addWidget(self.cpu_label, 0, 0)
        info_layout.addWidget(self.memory_label, 1, 0)
        info_layout.addWidget(self.disk_label, 2, 0)
        info_layout.addWidget(self.gpu_label, 3, 0)
        info_layout.addWidget(self.os_label, 4, 0)

        self.cpu_progress = QProgressBar()
        self.cpu_progress.setStyleSheet(ModernDarkTheme.get_progress_bar_style())
        self.memory_progress = QProgressBar()
        self.memory_progress.setStyleSheet(ModernDarkTheme.get_progress_bar_style())
        self.disk_progress = QProgressBar()
        self.disk_progress.setStyleSheet(ModernDarkTheme.get_progress_bar_style())
        self.gpu_progress = QProgressBar()
        self.gpu_progress.setStyleSheet(ModernDarkTheme.get_progress_bar_style())
        self.gpu_progress.setVisible(False)

        info_layout.addWidget(self.cpu_progress, 0, 1)
        info_layout.addWidget(self.memory_progress, 1, 1)
        info_layout.addWidget(self.disk_progress, 2, 1)
        info_layout.addWidget(self.gpu_progress, 3, 1)

        info_group.setLayout(info_layout)
        layout.addWidget(info_group)

        actions_group = QGroupBox("⚡ Quick Actions")
        actions_layout = QGridLayout()
        actions_layout.setSpacing(5)

        buttons = [
            ("📊 Task Manager", self.quick_actions.open_task_manager),
            ("🔧 Device Manager", self.quick_actions.open_device_manager),
            ("⚙️ Control Panel", self.quick_actions.open_control_panel),
            ("💾 Disk Management", self.quick_actions.open_disk_management),
            ("💻 CMD (Admin)", self.quick_actions.open_command_prompt),
            ("🔷 PowerShell (Admin)", self.quick_actions.open_powershell),
            ("📋 System Info", self.quick_actions.open_system_info),
            ("📝 Registry Editor", self.quick_actions.open_registry_editor),
            ("⚙️ Settings", self.quick_actions.open_settings),
            ("🔧 Services", self.quick_actions.open_services),
            ("📁 File Explorer", self.quick_actions.open_file_explorer),
            ("🌐 Network Connections", self.quick_actions.open_network_connections),
            ("✂️ Snipping Tool", self.quick_actions.open_snipping_tool),
            ("📄 Notepad", self.quick_actions.open_notepad),
            ("🔊 Volume Mixer", self.quick_actions.open_volume_mixer),
        ]

        for i, (text, action) in enumerate(buttons):
            btn = QPushButton(text)
            btn.setStyleSheet(
                ModernDarkTheme.get_button_style("default")
                + """
                QPushButton {
                    font-size: 12px;
                    font-weight: 500;
                    padding: 6px 12px;
                    text-align: center;
                }
            """
            )
            btn.setMaximumHeight(50)
            btn.setMinimumHeight(50)
            btn.setMinimumWidth(140)
            if action:
                btn.clicked.connect(action)
            row = i // 3
            col = i % 3
            actions_layout.addWidget(btn, row, col)

        actions_group.setLayout(actions_layout)
        layout.addWidget(actions_group)

        layout.addStretch()
        system_widget.setLayout(layout)
        self.tabs.addTab(system_widget, "📊 System")

    def create_cleaner_tab(self):
        cleaner_widget = QWidget()
        layout = QVBoxLayout()

        self.status = QLabel("Ready")
        self.status.setStyleSheet(ModernDarkTheme.get_status_label_style("default"))
        layout.addWidget(self.status)

        self.log = QTextEdit()
        self.log.setReadOnly(True)
        self.log.setMaximumHeight(120)
        self.log.setStyleSheet(ModernDarkTheme.get_text_edit_style())
        layout.addWidget(self.log)

        clean_group = QGroupBox("🧹 Cleaning Options")
        clean_layout = QVBoxLayout()

        btn1 = QPushButton("🧹 Clean Temp, Prefetch, Recycle Bin")
        btn1.clicked.connect(self.system_cleaner.clean_temp_files)
        btn1.setStyleSheet(ModernDarkTheme.get_button_style("success"))

        btn2 = QPushButton("🧼 Full Disk Cleanup (Drive C)")
        btn2.clicked.connect(self.system_cleaner.run_disk_cleanup)
        btn2.setStyleSheet(ModernDarkTheme.get_button_style("primary"))

        btn3 = QPushButton("🗑️ Empty Recycle Bin Only")
        btn3.clicked.connect(self.system_cleaner.empty_recycle_bin_only)
        btn3.setStyleSheet(ModernDarkTheme.get_button_style("special"))

        btn4 = QPushButton("🧠 Free Up Memory/RAM")
        btn4.clicked.connect(self.system_cleaner.optimize_memory)
        btn4.setStyleSheet(ModernDarkTheme.get_button_style("warning"))

        clean_layout.addWidget(btn1)
        clean_layout.addWidget(btn2)
        clean_layout.addWidget(btn3)
        clean_layout.addWidget(btn4)

        clean_group.setLayout(clean_layout)
        layout.addWidget(clean_group)

        layout.addStretch()
        cleaner_widget.setLayout(layout)
        self.tabs.addTab(cleaner_widget, "🧹 Cleaner")

    def create_power_tab(self):
        power_widget = QWidget()
        layout = QVBoxLayout()

        power_group = QGroupBox("⚡ Power Management")
        power_layout = QGridLayout()

        shutdown_btn = QPushButton("🔴 Shutdown Now")
        shutdown_btn.clicked.connect(self.power_manager.shutdown_now)
        shutdown_btn.setStyleSheet(ModernDarkTheme.get_button_style("danger"))

        restart_btn = QPushButton("🔄 Restart Now")
        restart_btn.clicked.connect(self.power_manager.restart_now)
        restart_btn.setStyleSheet(ModernDarkTheme.get_button_style("warning"))

        sleep_btn = QPushButton("😴 Sleep")
        sleep_btn.clicked.connect(self.power_manager.sleep_now)
        sleep_btn.setStyleSheet(ModernDarkTheme.get_button_style("special"))

        hibernate_btn = QPushButton("🛌 Hibernate")
        hibernate_btn.clicked.connect(self.power_manager.hibernate_now)
        hibernate_btn.setStyleSheet(ModernDarkTheme.get_button_style("success"))

        lock_btn = QPushButton("🔒 Lock Screen")
        lock_btn.clicked.connect(self.power_manager.lock_screen)
        lock_btn.setStyleSheet(ModernDarkTheme.get_button_style("default"))

        signout_btn = QPushButton("👤 Sign Out")
        signout_btn.clicked.connect(self.power_manager.sign_out)
        signout_btn.setStyleSheet(ModernDarkTheme.get_button_style("primary"))

        power_layout.addWidget(shutdown_btn, 0, 0)
        power_layout.addWidget(restart_btn, 0, 1)
        power_layout.addWidget(sleep_btn, 1, 0)
        power_layout.addWidget(hibernate_btn, 1, 1)
        power_layout.addWidget(lock_btn, 2, 0)
        power_layout.addWidget(signout_btn, 2, 1)

        power_group.setLayout(power_layout)
        layout.addWidget(power_group)

        scheduled_group = QGroupBox("⏰ Scheduled Actions")
        scheduled_layout = QVBoxLayout()

        # Enhanced countdown widget (initially hidden)
        self.countdown_widget = QWidget()
        self.countdown_widget.setStyleSheet(
            """
            QWidget {
                background: qlineargradient(x1:0, y1:0, x2:1, y2:1,
                    stop:0 rgba(239, 68, 68, 0.15),
                    stop:1 rgba(220, 38, 38, 0.1));
                border: 1px solid #ef4444;
                border-radius: 8px;
                margin: 2px 0;
            }
        """
        )

        # Add a subtle glow effect
        from PyQt6.QtWidgets import QGraphicsDropShadowEffect
        from PyQt6.QtGui import QColor

        shadow_effect = QGraphicsDropShadowEffect()
        shadow_effect.setBlurRadius(6)
        shadow_effect.setColor(QColor(239, 68, 68, 40))
        shadow_effect.setOffset(0, 0)
        self.countdown_widget.setGraphicsEffect(shadow_effect)

        countdown_layout = QHBoxLayout()
        countdown_layout.setSpacing(6)
        countdown_layout.setContentsMargins(6, 4, 6, 4)

        # Time display only
        self.countdown_time = QLabel("00:00")
        self.countdown_time.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.countdown_time.setStyleSheet(
            """
            QLabel {
                color: #ffffff;
                font-size: 11pt;
                font-weight: 700;
                font-family: 'Consolas', 'Monaco', 'Courier New', monospace;
                background: qlineargradient(x1:0, y1:0, x2:1, y2:0,
                    stop:0 #ef4444,
                    stop:1 #dc2626);
                border: 1px solid rgba(255, 255, 255, 0.2);
                border-radius: 4px;
                padding: 3px 6px;
                min-width: 50px;
                min-height: 20px;
            }
        """
        )
        countdown_layout.addWidget(self.countdown_time)

        # Quick cancel button with icon and text
        cancel_quick_btn = QPushButton("🚫 Cancel")
        cancel_quick_btn.clicked.connect(self.cancel_scheduled_shutdown)
        cancel_quick_btn.setStyleSheet(
            """
            QPushButton {
                background: rgba(239, 68, 68, 0.2);
                color: #ef4444;
                border: 1px solid rgba(239, 68, 68, 0.4);
                border-radius: 3px;
                padding: 2px 4px;
                font-size: 8pt;
                font-weight: 700;
                max-width: 60px;
                max-height: 20px;
                min-width: 60px;
                min-height: 20px;
            }
            QPushButton:hover {
                background: rgba(239, 68, 68, 0.3);
                border: 1px solid rgba(239, 68, 68, 0.6);
            }
            QPushButton:pressed {
                background: rgba(239, 68, 68, 0.4);
            }
        """
        )
        countdown_layout.addWidget(cancel_quick_btn)

        self.countdown_widget.setLayout(countdown_layout)
        self.countdown_widget.setVisible(False)
        scheduled_layout.addWidget(self.countdown_widget)

        # Timer input section
        timer_container = QWidget()
        timer_container.setStyleSheet(
            """
            QWidget {
                background: rgba(30, 41, 59, 0.6);
                border: 1px solid rgba(71, 85, 105, 0.4);
                border-radius: 8px;
                padding: 0;
                margin: 4px 0;
            }
        """
        )

        timer_layout = QHBoxLayout()
        timer_layout.setSpacing(8)
        timer_layout.setContentsMargins(12, 8, 12, 8)

        # Time label with icon
        time_label_layout = QHBoxLayout()
        time_label_layout.setSpacing(4)

        time_icon = QLabel("⏱️")
        time_icon.setStyleSheet(
            """
            QLabel {
                font-size: 10pt;
                padding: 0;
                margin: 0;
            }
        """
        )
        time_label_layout.addWidget(time_icon)

        time_label = QLabel("Duration:")
        time_label.setStyleSheet(
            """
            QLabel {
                color: #e2e8f0;
                font-size: 9pt;
                font-weight: 600;
                padding: 0;
                margin: 0;
            }
        """
        )
        time_label_layout.addWidget(time_label)
        time_label_layout.addStretch()

        timer_layout.addLayout(time_label_layout)

        # Input controls
        input_layout = QHBoxLayout()
        input_layout.setSpacing(6)

        self.timer_spinbox = QSpinBox()
        self.timer_spinbox.setRange(1, 1440)  # 1 minute to 24 hours in minutes
        self.timer_spinbox.setValue(5)
        self.timer_spinbox.setStyleSheet(
            """
            QSpinBox {
                background: rgba(15, 23, 42, 0.8);
                color: #e2e8f0;
                border: 1px solid rgba(71, 85, 105, 0.6);
                border-radius: 6px;
                padding: 4px 8px;
                font-size: 10pt;
                font-weight: 600;
                font-family: 'Consolas', 'Monaco', monospace;
                min-width: 60px;
                max-height: 28px;
            }
            QSpinBox:focus {
                border: 1px solid #3b82f6;
                background: rgba(15, 23, 42, 1.0);
            }
            QSpinBox::up-button, QSpinBox::down-button {
                background: rgba(71, 85, 105, 0.6);
                border: none;
                border-radius: 3px;
                width: 16px;
            }
            QSpinBox::up-button:hover, QSpinBox::down-button:hover {
                background: rgba(71, 85, 105, 0.8);
            }
            QSpinBox::up-arrow, QSpinBox::down-arrow {
                color: #e2e8f0;
                width: 6px;
                height: 6px;
            }
        """
        )
        input_layout.addWidget(self.timer_spinbox)

        self.time_unit_combo = QComboBox()
        self.time_unit_combo.addItems(["Minutes", "Hours"])
        self.time_unit_combo.setStyleSheet(
            """
            QComboBox {
                background: rgba(15, 23, 42, 0.8);
                color: #e2e8f0;
                border: 1px solid rgba(71, 85, 105, 0.6);
                border-radius: 6px;
                padding: 4px 8px;
                font-size: 9pt;
                font-weight: 500;
                min-width: 70px;
                max-height: 28px;
            }
            QComboBox:focus {
                border: 1px solid #3b82f6;
                background: rgba(15, 23, 42, 1.0);
            }
            QComboBox::drop-down {
                border: none;
                background: rgba(71, 85, 105, 0.6);
                border-radius: 3px;
                width: 16px;
            }
            QComboBox::down-arrow {
                color: #e2e8f0;
                width: 6px;
                height: 6px;
            }
            QComboBox QAbstractItemView {
                background: rgba(15, 23, 42, 0.95);
                color: #e2e8f0;
                border: 1px solid rgba(71, 85, 105, 0.6);
                border-radius: 6px;
                selection-background-color: rgba(59, 130, 246, 0.3);
            }
        """
        )
        self.time_unit_combo.currentTextChanged.connect(self.update_timer_range)
        input_layout.addWidget(self.time_unit_combo)

        timer_layout.addLayout(input_layout)

        timer_container.setLayout(timer_layout)
        scheduled_layout.addWidget(timer_container)

        action_layout = QHBoxLayout()
        action_layout.setSpacing(8)

        scheduled_shutdown_btn = QPushButton("⏰ Schedule Shutdown")
        scheduled_shutdown_btn.clicked.connect(self.schedule_shutdown)
        scheduled_shutdown_btn.setStyleSheet(
            """
            QPushButton {
                background: qlineargradient(x1:0, y1:0, x2:1, y2:0,
                    stop:0 #ef4444,
                    stop:1 #dc2626);
                color: white;
                border: none;
                border-radius: 8px;
                padding: 8px 14px;
                font-size: 11pt;
                font-weight: 600;
                min-height: 40px;
            }
            QPushButton:hover {
                background: qlineargradient(x1:0, y1:0, x2:1, y2:0,
                    stop:0 #dc2626,
                    stop:1 #b91c1c);
            }
            QPushButton:pressed {
                background: qlineargradient(x1:0, y1:0, x2:1, y2:0,
                    stop:0 #b91c1c,
                    stop:1 #991b1b);
            }
        """
        )

        cancel_scheduled_btn = QPushButton("🛑 Cancel Scheduled")
        cancel_scheduled_btn.clicked.connect(self.cancel_scheduled_shutdown)
        cancel_scheduled_btn.setStyleSheet(
            """
            QPushButton {
                background: qlineargradient(x1:0, y1:0, x2:1, y2:0,
                    stop:0 #64748b,
                    stop:1 #475569);
                color: white;
                border: none;
                border-radius: 8px;
                padding: 8px 14px;
                font-size: 11pt;
                font-weight: 600;
                min-height: 40px;
            }
            QPushButton:hover {
                background: qlineargradient(x1:0, y1:0, x2:1, y2:0,
                    stop:0 #475569,
                    stop:1 #334155);
            }
            QPushButton:pressed {
                background: qlineargradient(x1:0, y1:0, x2:1, y2:0,
                    stop:0 #334155,
                    stop:1 #1e293b);
            }
        """
        )

        action_layout.addWidget(scheduled_shutdown_btn)
        action_layout.addWidget(cancel_scheduled_btn)

        scheduled_layout.addLayout(action_layout)
        scheduled_group.setLayout(scheduled_layout)
        layout.addWidget(scheduled_group)

        warning_label = QLabel(
            "⚠️ Warning: Power actions will affect your system immediately!"
        )
        warning_label.setStyleSheet(ModernDarkTheme.get_status_label_style("warning"))
        warning_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(warning_label)

        layout.addStretch()
        power_widget.setLayout(layout)
        self.tabs.addTab(power_widget, "⚡ Power")

    def on_tab_changed(self, index):
        """Handle tab changes for lazy loading."""
        if index == 3 and self.system_info_widget is None:  # System info tab index
            self.create_system_info_tab()

    def create_system_info_tab(self):
        """Create system info tab with lazy loading."""
        if self.system_info_widget is None:
            self.system_info_widget = SystemInfoWidget()
            # Clear cache to ensure correct OS detection
            self.system_info_widget.clear_cache()
            # Replace the placeholder widget
            self.tabs.removeTab(3)
            self.tabs.insertTab(3, self.system_info_widget, "🖥️ Information")
            self.tabs.setCurrentIndex(3)  # Stay on the tab

    def update_timer_range(self, unit):
        if unit == "Minutes":
            self.timer_spinbox.setRange(1, 1440)  # 1 minute to 24 hours
            if self.timer_spinbox.value() > 1440:
                self.timer_spinbox.setValue(5)
        else:  # Hours
            self.timer_spinbox.setRange(1, 24)  # 1 hour to 24 hours
            if self.timer_spinbox.value() > 24:
                self.timer_spinbox.setValue(1)

    def schedule_shutdown(self):
        time_value = self.timer_spinbox.value()
        time_unit = self.time_unit_combo.currentText()

        # Calculate shutdown time
        if time_unit == "Minutes":
            seconds = time_value * 60
            time_text = f"{time_value} minute{'s' if time_value != 1 else ''}"
        else:  # Hours
            seconds = time_value * 3600
            time_text = f"{time_value} hour{'s' if time_value != 1 else ''}"

        # Show confirmation dialog
        from modules.power_manager import MinimalConfirmDialog

        dialog = MinimalConfirmDialog(
            self,
            "Schedule Shutdown",
            f"Schedule shutdown in {time_text}?",
            "Schedule",
            "Cancel",
        )
        dialog.exec()

        if dialog.result:
            try:
                import subprocess
                import datetime

                subprocess.run(["shutdown", "/s", "/f", "/t", str(seconds)], check=True)

                # Start integrated countdown
                self.shutdown_time = datetime.datetime.now() + datetime.timedelta(
                    seconds=seconds
                )
                self.countdown_widget.setVisible(True)
                self.countdown_timer.start(1000)  # Update every second

            except Exception as e:
                from modules.power_manager import MinimalInfoDialog

                dialog = MinimalInfoDialog(
                    self, "Error", f"Failed to schedule shutdown: {e}", "error"
                )
                dialog.exec()

    def cancel_scheduled_shutdown(self):
        """Cancel scheduled shutdown and hide countdown."""
        try:
            import subprocess

            subprocess.run(["shutdown", "/a"], check=True)

            # Hide countdown display
            self.countdown_timer.stop()
            self.countdown_widget.setVisible(False)
            self.shutdown_time = None

            from modules.power_manager import MinimalInfoDialog

            dialog = MinimalInfoDialog(
                self, "Cancelled", "Scheduled shutdown cancelled.", "success"
            )
            dialog.exec()

        except Exception as e:
            from modules.power_manager import MinimalInfoDialog

            dialog = MinimalInfoDialog(
                self, "Info", "No scheduled shutdown to cancel.", "info"
            )
            dialog.exec()

    def update_countdown_display(self):
        """Update the integrated countdown display."""
        if not self.shutdown_time:
            return

        import datetime

        now = datetime.datetime.now()
        remaining = self.shutdown_time - now

        if remaining.total_seconds() <= 0:
            self.countdown_time.setText("Shutting down now...")
            self.countdown_timer.stop()
            return

        # Format remaining time
        total_seconds = int(remaining.total_seconds())
        hours = total_seconds // 3600
        minutes = (total_seconds % 3600) // 60
        seconds = total_seconds % 60

        if hours > 0:
            time_str = f"{hours:02d}:{minutes:02d}:{seconds:02d}"
        else:
            time_str = f"{minutes:02d}:{seconds:02d}"

        self.countdown_time.setText(time_str)

    def update_system_info(self, info):
        try:
            cpu_percent = info["cpu"]
            self.cpu_label.setText(f"CPU: {cpu_percent:.1f}%")
            self.cpu_progress.setValue(int(cpu_percent))

            memory_percent = info["memory_percent"]
            memory_used_gb = info["memory_used"] / (1024**3)
            memory_total_gb = info["memory_total"] / (1024**3)
            self.memory_label.setText(
                f"Memory: {memory_used_gb:.1f}GB/{memory_total_gb:.1f}GB ({memory_percent:.1f}%)"
            )
            self.memory_progress.setValue(int(memory_percent))

            disk_percent = info["disk_percent"]
            disk_used_gb = info["disk_used"] / (1024**3)
            disk_total_gb = info["disk_total"] / (1024**3)
            self.disk_label.setText(
                f"Disk C: {disk_used_gb:.1f}GB/{disk_total_gb:.1f}GB ({disk_percent:.1f}%)"
            )
            self.disk_progress.setValue(int(disk_percent))

            if info.get("gpu_available", False):
                gpu_utilization = info["gpu_utilization"]
                gpu_memory_used_gb = info["gpu_memory_used"] / 1024
                gpu_memory_total_gb = info["gpu_memory_total"] / 1024
                gpu_temp = info["gpu_temperature"]

                self.gpu_label.setText(
                    f"GPU: {gpu_utilization:.1f}% | {gpu_memory_used_gb:.1f}GB/{gpu_memory_total_gb:.1f}GB | {gpu_temp:.0f}°C"
                )
                self.gpu_progress.setValue(int(gpu_utilization))
                self.gpu_label.setVisible(True)
                self.gpu_progress.setVisible(True)
            else:
                self.gpu_label.setText("GPU: Not Available")
                self.gpu_label.setVisible(True)
                self.gpu_progress.setVisible(False)

            uptime_str = info["uptime"]
            self.uptime_display.setText(self.format_uptime(uptime_str))

            if hasattr(self, "tray_icon") and self.tray_icon.isVisible():
                self.tray_icon.update_system_info(info)

        except Exception as e:
            pass

    def format_uptime(self, uptime_str):
        try:
            days = 0
            hours = 0
            minutes = 0
            seconds = 0

            if "day" in uptime_str:
                day_part, time_part = uptime_str.split(", ")
                if "days" in day_part:
                    days = int(day_part.split(" ")[0])
                else:
                    days = 1
                time_str = time_part
            else:
                time_str = uptime_str

            time_parts = time_str.split(":")
            if len(time_parts) == 3:
                hours, minutes, seconds = map(int, time_parts)

            result_parts = []

            if days > 0:
                if days == 1:
                    result_parts.append(f"{days} day")
                else:
                    result_parts.append(f"{days} days")

            if hours > 0:
                if hours == 1:
                    result_parts.append(f"{hours} hour")
                else:
                    result_parts.append(f"{hours} hours")

            if minutes > 0:
                if minutes == 1:
                    result_parts.append(f"{minutes} min")
                else:
                    result_parts.append(f"{minutes} mins")

            if seconds > 0:
                if seconds == 1:
                    result_parts.append(f"{seconds} sec")
                else:
                    result_parts.append(f"{seconds} secs")

            if not result_parts:
                return "Just started"

            return " ".join(result_parts)
        except Exception as e:
            return uptime_str

    def log_message(self, msg):
        self.log.append(msg)
        self.log.verticalScrollBar().setValue(self.log.verticalScrollBar().maximum())

    def update_status(self, msg):
        self.status.setText(msg)


if __name__ == "__main__":
    app = QApplication(sys.argv)

    app.setApplicationName("PC Toolkit Pro")
    app.setApplicationVersion("2.9")
    app.setOrganizationName("PC Toolkit Suite")

    app.setQuitOnLastWindowClosed(False)
    if not QSystemTrayIcon.isSystemTrayAvailable():
        QMessageBox.critical(
            None,
            "System Tray",
            "System tray is not available on this system. The application will run normally.",
        )

    win = PCToolkit()
    win.show()

    sys.exit(app.exec())
