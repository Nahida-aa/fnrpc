import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getRouter } from "./router.tsx";
import { RouterProvider } from "@tanstack/react-router";
import { QueryClientProvider } from "@tanstack/react-query";
import { getQueryClient } from "#/integrations/tanstack-query/provider.ts";

const router = getRouter();

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <QueryClientProvider client={getQueryClient()}>
    <RouterProvider router={router} />
    </QueryClientProvider>
  </StrictMode>,
);
