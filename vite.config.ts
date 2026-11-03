import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  // Tauri: não apagar a saída do Rust no terminal.
  clearScreen: false,
  server: {
    // Tauri espera esta porta fixa (devUrl em src-tauri/tauri.conf.json).
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
});
