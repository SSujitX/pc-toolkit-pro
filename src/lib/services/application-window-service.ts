import { invoke } from '@tauri-apps/api/core';

export class ApplicationWindowService {
  static async showAfterMount(): Promise<void> {
    try {
      await invoke('show_main_window');
    } catch {
      // Browser preview has no Tauri bridge.
    }
  }

  static minimize(): Promise<void> {
    return invoke('minimize_main_window');
  }

  static toggleMaximize(): Promise<void> {
    return invoke('toggle_maximize_main_window');
  }

  static closeOrHide(): Promise<void> {
    return invoke('close_main_window');
  }

  static hide(): Promise<void> {
    return invoke('hide_main_window');
  }
}
