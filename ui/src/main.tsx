import React from "react"
import ReactDOM from "react-dom/client"
import { QueryClient, QueryClientProvider } from "@tanstack/react-query"
import { createRouter, RouterProvider } from "@tanstack/react-router"
import { WagmiProvider } from "wagmi"
import { Toaster } from "sonner"
import { routeTree } from "./routeTree.gen"
import { wagmiConfig } from "@/lib/evm/client"
import { IcWalletProviderRoot } from "@/features/wallet/ic-wallet-provider"
import "./styles.css"

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 15_000,
      retry: false,
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
    },
  },
})
const router = createRouter({ routeTree, defaultPreload: "intent" })

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router
  }
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <WagmiProvider config={wagmiConfig}>
      <QueryClientProvider client={queryClient}>
        <IcWalletProviderRoot>
          <RouterProvider router={router} />
          <Toaster richColors position="bottom-right" />
        </IcWalletProviderRoot>
      </QueryClientProvider>
    </WagmiProvider>
  </React.StrictMode>,
)
