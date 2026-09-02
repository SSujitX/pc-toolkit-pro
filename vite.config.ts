import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import path from "node:path";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [vue(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
      "@assets": path.resolve(__dirname, "./src/assets"),
    },
  },
  clearScreen: false,
  server: {
    // 1420 falls in a Windows Hyper-V excluded range (1326-1425) → EACCES.
    port: 5173,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 5174,
        }
      : {
          protocol: "ws",
          host: "127.0.0.1",
          port: 5174,
        },
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
