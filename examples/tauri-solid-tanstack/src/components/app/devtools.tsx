
import { lazy } from "solid-js";
import { isServer } from "solid-js/web";

export const Devtools = !isServer
  ? lazy(() =>
      Promise.all([
        import("@tanstack/solid-devtools"),
        import("@tanstack/solid-form-devtools"),
        import("@tanstack/solid-query-devtools"),
        import("@tanstack/solid-router-devtools"),
      ]).then(([
        { TanStackDevtools },
        { formDevtoolsPlugin },
        { SolidQueryDevtoolsPanel },
        { TanStackRouterDevtoolsPanel }
      ]) => ({
        default: () => (
          <TanStackDevtools
            plugins={[
              {
                name: "Tanstack Router",
                render: <TanStackRouterDevtoolsPanel />,
              },
              {
                name: "Tanstack Query",
                render: <SolidQueryDevtoolsPanel />,
              },
              formDevtoolsPlugin(),
            ]}
          />
        ),
      }))
    )
  : () => null;
