import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react()],
  build: {
    chunkSizeWarningLimit: 1200,
    rollupOptions: {
      output: {
        manualChunks(id) {
          const has = (value: string) => id.indexOf(value) >= 0;

          if (!has("node_modules")) {
            return undefined;
          }

          if (
            has("/antd/") ||
            has("\\antd\\") ||
            has("/@ant-design/") ||
            has("\\@ant-design\\") ||
            has("/@rc-component/") ||
            has("\\@rc-component\\") ||
            has("/rc-") ||
            has("\\rc-")
          ) {
            return "antd-vendor";
          }

          if (has("/@tanstack/") || has("\\@tanstack\\")) {
            return "query-vendor";
          }

          if (has("/lucide-react/") || has("\\lucide-react\\")) {
            return "icon-vendor";
          }

          return undefined;
        }
      }
    }
  },
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8080",
        changeOrigin: true
      }
    }
  }
});
