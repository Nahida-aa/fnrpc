import { defineConfig } from "rolldown"

export default defineConfig({
  input: ["src/index.ts"],
  output: {
    dir: "dist",
    format: "esm",
    preserveModules: true,
    preserveModulesRoot: "src",
    entryFileNames: "[name].js",
  },
  platform: "neutral",
  external: [/@fnrpc\/client/, /@tanstack\//],
})
