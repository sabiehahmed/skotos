import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["scripts/**/*.test.{mjs,ts}"],
    environment: "node",
    maxWorkers: 2,
  },
});
