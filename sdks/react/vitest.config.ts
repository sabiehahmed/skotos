import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

const reactSdkDir = path.dirname(fileURLToPath(import.meta.url));
const browserSdkEntry = path.resolve(reactSdkDir, "../browser/src/index.ts");

export default defineConfig({
  resolve: {
    alias: [
      {
        find: /^@skotos\/browser-sdk$/,
        replacement: browserSdkEntry,
      },
    ],
  },
  test: {
    environment: "jsdom",
    globals: true,
    maxWorkers: 2,
  },
});
