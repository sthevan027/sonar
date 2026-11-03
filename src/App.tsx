import { Suspense, useState } from "react";
import "./styles/base.css";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { Panel } from "./screens/Panel";
import { retryAppState } from "./store";

/**
 * Telas do app. Cada tela nova entra como outro nome aqui e um `case` em `Screen`:
 * a Task 12 acrescenta o editor de perfil; as próximas, dispositivos e configurações.
 */
export type Route = { name: "panel" };

const noop = () => undefined;

export default function App() {
  // Sem setter por enquanto: só existe o painel. Quem trocar de tela ganha o `setRoute` aqui.
  const [route] = useState<Route>({ name: "panel" });
  return (
    <main className="win">
      <ErrorBoundary onReset={retryAppState}>
        <Suspense fallback={null}>
          <Screen route={route} />
        </Suspense>
      </ErrorBoundary>
    </main>
  );
}

function Screen({ route }: { route: Route }) {
  switch (route.name) {
    case "panel":
      // "+" do perfil ainda não abre nada: a Task 12 liga o editor.
      return <Panel onAddProfile={noop} />;
    default:
      return null;
  }
}
