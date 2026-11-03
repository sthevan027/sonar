import { Suspense, useState } from "react";
import "./styles/base.css";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ToastProvider } from "./components/Toast";
import type { Route } from "./routes";
import { Panel } from "./screens/Panel";
import { retryAppState } from "./store";

const noop = () => undefined;

export default function App() {
  const [route, setRoute] = useState<Route>({ name: "panel", tab: "output" });
  return (
    <main className="win">
      <ToastProvider>
        <ErrorBoundary onReset={retryAppState}>
          <Suspense fallback={null}>
            <Screen route={route} navigate={setRoute} />
          </Suspense>
        </ErrorBoundary>
      </ToastProvider>
    </main>
  );
}

function Screen({ route, navigate }: { route: Route; navigate: (route: Route) => void }) {
  switch (route.name) {
    case "panel":
      // "+" do perfil ainda não abre nada: a Task 12 liga o editor a `navigate`.
      return (
        <Panel
          onAddProfile={noop}
          tab={route.tab}
          onTabChange={(tab) => navigate({ name: "panel", tab })}
        />
      );
    default:
      return null;
  }
}
