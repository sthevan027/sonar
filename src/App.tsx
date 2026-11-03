import { Suspense } from "react";
import "./styles/base.css";
import { Card } from "./components/Card";
import { Chip } from "./components/Chip";
import { useAppState } from "./store";

// Esqueleto da fundação: mostra o título e a saída padrão, lidas do estado do app.
export default function App() {
  return (
    <main className="win">
      <Suspense fallback={null}>
        <Home />
      </Suspense>
    </main>
  );
}

function Home() {
  const state = useAppState();
  const output = state.devices.find((d) => d.flow === "render" && d.is_default);

  return (
    <>
      <h1 className="title">Sonar</h1>
      <Card>
        <span className="lbl">Saída padrão</span>
        <Chip kind="device" tone="brass" label={output?.display_name ?? "Nenhuma saída"} />
      </Card>
    </>
  );
}
