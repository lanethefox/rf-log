import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri serves this dev server; fixed port so tauri.conf.json devUrl matches.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
});
