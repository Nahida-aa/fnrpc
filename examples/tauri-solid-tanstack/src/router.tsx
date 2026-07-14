// import { ErrorCard } from '@repo/ui-solid/app/error';
// import { NotFound } from '@repo/ui-solid/app/NotFound';
import {
	createRouter,
} from '@tanstack/solid-router';
// Import the generated route tree
import { routeTree } from './routeTree.gen';
import { getQueryClient } from '#/integrations/tanstack-query/provider.ts';


export function getRouter() {
	// Create a new router instance
	const queryClient = getQueryClient();
	const router = createRouter({
		routeTree,
		context: { queryClient },
		scrollRestoration: true,
		defaultPreloadStaleTime: 0,
		// routeMasks: [...settingsMasks]
		// defaultErrorComponent: ErrorCard,
		// defaultNotFoundComponent: () => <NotFound />,
	});
	return router;
}

// Register the router instance for type safety
declare module '@tanstack/solid-router' {
	interface Register {
		router: ReturnType<typeof getRouter>;
	}
}
