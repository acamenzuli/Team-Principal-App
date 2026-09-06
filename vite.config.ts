import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri drives this dev server; the fixed port and failure-on-conflict are
// deliberate, because a silently relocated dev server means the app window
// loads nothing and the reason is not obvious.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  build: {
    target: "chrome110",
    sourcemap: true,
  },
});
