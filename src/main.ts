import { createApp } from 'vue';
import { createPinia } from 'pinia';
import App from './App.vue';
import { i18n } from './i18n';
import { ApplicationWindowService } from './lib/services/application-window-service';
import { useAppStore } from './stores/app-store';
import { useMemoryCleanerStore } from './stores/memory-cleaner-store';
import { setupTray } from './lib/services/tray-service';
import './assets/main.css';

document.documentElement.dataset.skin = 'pctoolkit';

const app = createApp(App);
const pinia = createPinia();
app.use(pinia);
app.use(i18n);

async function start() {
  useAppStore(pinia).loadSettings();
  app.mount('#app');
  await ApplicationWindowService.showAfterMount();
  const memory = useMemoryCleanerStore(pinia);
  await memory.loadSettings();
  memory.startAutoClean();
  await setupTray();
}

void start();
