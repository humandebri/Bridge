import path from "node:path"
import { defineConfig, type Plugin, type PreviewServer, type ViteDevServer } from "vite"
import baseConfig from "./vite.config"

const pocketIcGateway = process.env.KINIC_POCKET_IC_GATEWAY_URL
if (!pocketIcGateway) throw new Error("KINIC_POCKET_IC_GATEWAY_URL is required for the real E2E server")

const rejectUnsupportedPocketIcV4: Plugin = {
  name: "reject-unsupported-pocket-ic-v4",
  configureServer(server) {
    installV4RejectMiddleware(server)
  },
  configurePreviewServer(server) {
    installV4RejectMiddleware(server)
  },
}

function installV4RejectMiddleware(server: ViteDevServer | PreviewServer) {
  server.middlewares.use("/api/v4", (_request, response) => {
    response.statusCode = 404
    response.end("PocketIC test gateway does not support API v4")
  })
}

export default defineConfig({
  ...baseConfig,
  plugins: [...(baseConfig.plugins ?? []), rejectUnsupportedPocketIcV4],
  server: {
    proxy: {
      "/api": {
        target: pocketIcGateway,
        changeOrigin: true,
        rewrite: (requestPath) => requestPath.replace(/^\/api\/v3\//, "/api/v2/"),
      },
    },
  },
  preview: {
    proxy: {
      "/api": {
        target: pocketIcGateway,
        changeOrigin: true,
        rewrite: (requestPath) => requestPath.replace(/^\/api\/v3\//, "/api/v2/"),
      },
    },
  },
  resolve: {
    alias: [
      { find: /^@\/config\/profile$/, replacement: path.resolve(import.meta.dirname, ".e2e-runtime/profile.ts") },
      { find: /^@\/features\/wallet\/ic-wallet-provider$/, replacement: path.resolve(import.meta.dirname, "e2e-real/ic-wallet-provider.tsx") },
      { find: "@", replacement: path.resolve(import.meta.dirname, "src") },
    ],
  },
})
