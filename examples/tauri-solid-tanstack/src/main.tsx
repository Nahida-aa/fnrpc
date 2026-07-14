import { QueryClient, QueryClientProvider } from '@tanstack/solid-query';
import { createRouter, RouterProvider } from '@tanstack/solid-router';
import { render } from 'solid-js/web';
import { getRouter } from '#/router.tsx';
import { getQueryClient } from './integrations/tanstack-query/provider';
// import './styles.css';

// Create a new router instance
const router = getRouter();

const rootElement = document.getElementById('root')!;

if (!rootElement?.innerHTML) {
	render(
		() => (
			<QueryClientProvider client={getQueryClient()}>
				<RouterProvider router={router} />
			</QueryClientProvider>
		),
		rootElement,
	);
}
