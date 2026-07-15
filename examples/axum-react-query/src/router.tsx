import { createRouter, RouterProvider } from "@tanstack/react-router";
import { getQueryClient } from "./integrations/tanstack-query/provider.ts";
import { routeTree } from "./routeTree.gen";

export function getRouter() {
  const queryClient = getQueryClient();
  const router = createRouter({
    routeTree,
    context: { queryClient },
    scrollRestoration: true,
  });
  return router;
}
