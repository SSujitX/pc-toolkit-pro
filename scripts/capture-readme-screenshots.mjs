import puppeteer from 'puppeteer-core';
import { mkdir } from 'node:fs/promises';
import path from 'node:path';

const chromePath =
  process.env.CHROME_PATH || 'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe';
const base = 'http://127.0.0.1:5173/';
const outDir = path.resolve('images');
await mkdir(outDir, { recursive: true });

const browser = await puppeteer.launch({
  executablePath: chromePath,
  headless: true,
  defaultViewport: { width: 1280, height: 800, deviceScaleFactor: 2 },
  args: ['--hide-scrollbars', '--force-device-scale-factor=2'],
});

const page = await browser.newPage();
await page.goto(base, { waitUntil: 'networkidle0', timeout: 60_000 });
await page.waitForSelector('.nav-item', { timeout: 30_000 });

await page.evaluate(() => {
  document.documentElement.classList.add('dark');
  document.documentElement.dataset.theme = 'dark';
  document.documentElement.style.colorScheme = 'dark';

  const app = document.querySelector('#app');
  const vueApp = app && /** @type {any} */ (app).__vue_app__;
  const pinia = vueApp?.config?.globalProperties?.$pinia;
  if (!pinia) throw new Error('Pinia not found on #app');

  const GB = 1024 ** 3;
  const demoMonitor = {
    cpu: 28.4,
    memoryPercent: 47.2,
    memoryUsed: 29.6 * GB,
    memoryTotal: 63.1 * GB,
    diskPercent: 84.5,
    diskUsed: 786.8 * GB,
    diskTotal: 930.8 * GB,
    uptimeSeconds: 7 * 3600 + 47 * 60 + 39,
    osLabel: 'Windows 11',
    gpuAvailable: true,
    gpuUtilization: 23,
    gpuMemoryUsed: 2.8 * GB,
    gpuMemoryTotal: 16 * GB,
    gpuTemperature: 61,
  };
  const demoMemoryStats = {
    physicalTotal: 63.1 * GB,
    physicalAvail: 33.2 * GB,
    physicalUsed: 29.9 * GB,
    physicalLoadPercent: 47,
    virtualTotal: 72.4 * GB,
    virtualAvail: 38.1 * GB,
    virtualUsed: 34.3 * GB,
    virtualLoadPercent: 47,
  };
  const demoSystemInfo = {
    uptime: '7h 47m 39s',
    cpuName: 'AMD Ryzen 9 7950X 16-Core Processor',
    cpuCores: 16,
    cpuThreads: 32,
    cpuUsage: 24.9,
    cpuFrequency: '4501 MHz (Max 4501 MHz)',
    cpuCache: 'L3 64 MB',
    cpuSocket: 'AM5',
    memoryTotal: 63.1 * GB,
    memoryUsed: 28.5 * GB,
    memoryAvailable: 34.6 * GB,
    memoryPercent: 45.2,
    ramName: 'DDR5 Dual Channel',
    ramType: 'DDR5',
    ramSpeed: '6000 MT/s',
    ramSlotsUsed: '2 of 4',
    diskTotal: 930.8 * GB,
    diskUsed: 786.8 * GB,
    diskFree: 144 * GB,
    diskPercent: 84.5,
    diskDevice: 'NVMe SSD (C:)',
    diskType: 'NVMe SSD',
    gpuName: 'NVIDIA GeForce RTX 4080',
    gpuUsage: 23,
    gpuMemoryUsed: 2.8 * GB,
    gpuMemoryTotal: 16 * GB,
    gpuTemperature: 61,
    motherboardProduct: 'X670E Gaming',
    motherboardManufacturer: 'ASUS',
    motherboardVersion: 'Rev 1.0',
    motherboard: 'ASUS X670E Gaming',
    chipset: 'AMD X670E',
    bios: 'AMI',
    biosVersion: '2403',
    biosManufacturer: 'American Megatrends',
    biosDate: '2025-06-12',
    systemModel: 'Custom Build',
    memorySlotsTotal: '4',
    maxMemoryCapacity: '192 GB',
    osEdition: 'Windows 11 Pro',
    osVersion: '24H2',
    osBuild: '26100',
    osExperience: 'Desktop',
    hostname: 'DESKTOP-PCTOOL',
    username: 'User',
    monitors: ['Dell U2723QE 3840x2160', 'LG UltraGear 2560x1440'],
    storageDevices: ['Samsung 990 PRO 1TB', 'WD Black SN850X 2TB'],
    powerSupplyName: 'Corsair RM850x 850W',
    powerPlan: 'Balanced',
    powerSupplies: ['Corsair RM850x 850W'],
    batteries: [],
    acLineStatus: 'Online',
    copyText: 'PC Toolkit Pro system report',
  };

  const monitor = pinia._s.get('monitor');
  if (monitor) {
    if (monitor.timer != null) {
      clearInterval(monitor.timer);
      monitor.timer = null;
    }
    monitor.refresh = async () => {
      monitor.snapshot = { ...demoMonitor };
      monitor.loading = false;
    };
    monitor.startPolling = () => {
      monitor.snapshot = { ...demoMonitor };
      monitor.loading = false;
    };
    monitor.snapshot = { ...demoMonitor };
    monitor.loading = false;
  }

  const memory = pinia._s.get('memoryCleaner');
  if (memory) {
    if (memory.statsTimer != null) {
      clearInterval(memory.statsTimer);
      memory.statsTimer = null;
    }
    if (memory.autoTimer != null) {
      clearInterval(memory.autoTimer);
      memory.autoTimer = null;
    }
    memory.refreshStats = async () => {
      memory.stats = { ...demoMemoryStats };
    };
    memory.stats = { ...demoMemoryStats };
    memory.settings = {
      areas: {
        workingSet: true,
        systemFileCache: true,
        modifiedPageList: true,
        standbyList: true,
        standbyListLowPriority: false,
        combinedPageList: true,
        registryCache: true,
        modifiedFileCache: true,
      },
      autoIntervalMinutes: 30,
      autoFreeBelowPercent: 15,
    };
    memory.settingsLoaded = true;
  }

  const systemInfo = pinia._s.get('systemInfo');
  if (systemInfo) {
    systemInfo.load = async () => {
      systemInfo.info = { ...demoSystemInfo };
      systemInfo.loading = false;
      systemInfo.error = null;
    };
    systemInfo.info = { ...demoSystemInfo };
    systemInfo.loading = false;
    systemInfo.error = null;
  }

  const appStore = pinia._s.get('app');
  if (appStore?.setTheme) appStore.setTheme('dark');

  /** @type {any} */
  window.__pctDemo = { demoMonitor, demoMemoryStats, demoSystemInfo };
});

async function clickNav(label) {
  await page.evaluate((name) => {
    const buttons = [...document.querySelectorAll('button.nav-item, .nav-item')];
    const match = buttons.find((el) => new RegExp(name, 'i').test(el.textContent || ''));
    if (!match) throw new Error(`Nav item not found: ${name}`);
    match.click();
  }, label);
  await new Promise((r) => setTimeout(r, 1100));
}

async function shot(file) {
  const out = path.join(outDir, file);
  await page.screenshot({ path: out, type: 'png' });
  console.log('wrote', out);
}

await new Promise((r) => setTimeout(r, 900));
await shot('screenshot1.png'); // System

await clickNav('Memory Cleaner');
await shot('screenshot2.png');

await clickNav('^Cleaner$|Cleaner(?! )');
// Prefer exact Cleaner (not Deep / Memory)
await page.evaluate(() => {
  const buttons = [...document.querySelectorAll('button.nav-item, .nav-item')];
  const cleaner = buttons.find((el) => /^\s*Cleaner\s*$/i.test(el.textContent || ''));
  if (!cleaner) throw new Error('Cleaner nav item not found');
  cleaner.click();
});
await new Promise((r) => setTimeout(r, 1100));
await shot('screenshot3.png');

await clickNav('Information');
await page.evaluate(() => {
  const pinia = document.querySelector('#app')?.__vue_app__?.config?.globalProperties?.$pinia;
  const systemInfo = pinia?._s?.get('systemInfo');
  const demo = window.__pctDemo?.demoSystemInfo;
  if (systemInfo && demo) {
    systemInfo.info = { ...demo };
    systemInfo.loading = false;
    systemInfo.error = null;
  }
});
await new Promise((r) => setTimeout(r, 700));
await shot('screenshot4.png');

await browser.close();
