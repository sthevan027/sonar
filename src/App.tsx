import { Suspense, useState } from "react";
import "./styles/base.css";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { Panel } from "./screens/Panel";
import { retryAppState } from "./store";
import type { Route } from "./routes";

const noop = () => undefined;

export default function App() {
  const [route, setRoute] = useState<Route>({ name: "panel" });
  return (
    <main className="win">
      <ErrorBoundary onReset={retryAppState}>
        <Suspense fallback={null}>
          <Screen route={route} navigate={setRoute} />
        </Suspense>
      </ErrorBoundary>
    </main>
  );
}

function Screen({ route, navigate }: { route: Route; navigate: (route: Route) => void }) {
  switch (route.name) {
    case "panel":
      // "+" do perfil ainda não abre nada: a Task 12 liga o editor a `navigate`.
      return <Panel onAddProfile={noop} onNavigate={navigate} />;
    default:
      return null;
  }
}
