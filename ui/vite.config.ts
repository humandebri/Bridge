import path from "node:path"
import tailwindcss from "@tailwindcss/vite"
import { tanstackRouter } from "@tanstack/router-plugin/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

export default defineConfig({
  plugins: [tanstackRouter({ target: "react", autoCodeSplitting: true }), react(), tailwindcss()],
  resolve: { alias: { "@": path.resolve(import.meta.dirname, "./src") } },
  define: process.env.KINIC_GENERIC_PRODUCTION_UI_BUILD === "1"
    ? { "import.meta.env.VITE_DEPLOYMENT_PROFILE_JSON": "undefined" }
    : undefined,
})
