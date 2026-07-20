import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { tanstackRouter } from "@tanstack/router-plugin/vite";

export default defineConfig({
  plugins: [react(), tanstackRouter({ target: "react" })],
  server: {
    port: 5173,
    proxy: {
      "/fnrpc": "http://localhost:3000",
    },
  },
});
