import path from "node:path";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const webDir = path.dirname(fileURLToPath(import.meta.url));
const packagesDir = path.resolve(webDir, "../../packages");
const apiProxyTarget = process.env.VITE_API_PROXY_TARGET ?? "http://localhost:8080";

export default defineConfig(({ command, mode }) => ({
  plugins: [react()],
  resolve:
    command === "serve" || mode === "test"
      ? {
          alias: [
            {
              find: /^@skotos\/ui\/styles\.css$/,
              replacement: path.join(packagesDir, "ui/src/styles/index.css"),
            },
            {
              find: /^@skotos\/ui\/tokens\.css$/,
              replacement: path.join(packagesDir, "ui/src/styles/tokens.css"),
            },
            {
              find: /^@skotos\/ui$/,
              replacement: path.join(packagesDir, "ui/src/index.ts"),
            },
            {
              find: /^@skotos\/api-client$/,
              replacement: path.join(packagesDir, "api-client/src/index.ts"),
            },
          ],
        }
      : undefined,
  server: {
    port: 5173,
    proxy: {
      "/api": {
        target: apiProxyTarget,
        changeOrigin: true,
      },
    },
  },
}));
