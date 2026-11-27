import path from "node:path";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const webDir = path.dirname(fileURLToPath(import.meta.url));
const packagesDir = path.resolve(webDir, "../../packages");

export default defineConfig({
  plugins: [react()],
  resolve: {
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
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test-setup.ts"],
    maxWorkers: 2,
  },
});
