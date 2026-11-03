import { Suspense, useState } from "react";
import "./styles/base.css";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { ToastProvider } from "./components/Toast";
import type { Route } from "./routes";
import { Panel } from "./screens/Panel";
import { ProfileEditor } from "./screens/ProfileEditor";
import { retryAppState } from "./store";

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
      return (
        <Panel
          onAddProfile={() => navigate({ name: "profileEditor", returnTab: route.tab })}
          onEditProfile={(profileId) =>
            navigate({ name: "profileEditor", profileId, returnTab: route.tab })
          }
          tab={route.tab}
          onTabChange={(tab) => navigate({ name: "panel", tab })}
        />
      );
    case "profileEditor":
      return (
        <ProfileEditor
          profileId={route.profileId}
          onDone={() => navigate({ name: "panel", tab: route.returnTab })}
        />
      );
  }
}
