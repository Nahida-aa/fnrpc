import { HeadContent, Outlet, Scripts, createRootRoute, useParams, useRouteContext, useRouter, useRouterState } from '@tanstack/solid-router';
import { ThemeProvider, themeScript } from '#/components/theme/index.tsx';
import type { JSX } from 'solid-js';
import styleCss from '../styles.css?url'
import { QueryClient, QueryClientProvider } from '@tanstack/solid-query';
import { Devtools } from '#/components/app/devtools.tsx';
// import * as deviceApi from '../feat/env/device';
// import { getGroupList } from '#/cmd/tasks.ts';
interface MyRouterContext {
	queryClient: QueryClient;
}

export const Route = createRootRoute<MyRouterContext>({
  head: () => ({
    title: 'LocalDub',
    meta: [{
      name: 'viewport',
      content: 'width=device-width, initial-scale=1',
    }],
    links: [{ rel: 'stylesheet', href: styleCss }],
    scripts: [{ children: themeScript }],
	}),
  beforeLoad: async () => {

  },
  shellComponent: RootComponent,
});
function RootComponent() {
  return (
    <RootDocument >
			<Outlet />
    </RootDocument>
  )
}
function RootDocument({ children }: { children: JSX.Element }) {
  return <>
  <HeadContent />
  
    <ThemeProvider>
        <main class="min-w-0 flex-1 h-screen grid grid-rows-[auto_1fr]">
          {children}
        </main>
    </ThemeProvider>
  <Devtools />
  <Scripts />
  </>
}