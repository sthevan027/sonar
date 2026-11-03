import { useState, type ReactNode } from "react";
import { Icon } from "../components/Icon";
import { Tabs } from "../components/Tabs";
import { useAppState } from "../store";
import type { Route } from "../routes";
import { MicTab } from "./MicTab";
import { OutputTab } from "./OutputTab";
import { ProfileBar } from "./ProfileBar";

interface PanelProps {
  /** Botão "+" da barra de perfis. Ligado ao editor na Task 12. */
  onAddProfile: () => void;
  /** Troca de tela (App). Nenhum botão do painel usa ainda; a Task 12 liga o editor por aqui. */
  onNavigate: (route: Route) => void;
}

/** Painel principal (mockups A e A2): perfis, avisos, abas Saída | Microfone. */
export function Panel({ onAddProfile }: PanelProps) {
  const state = useAppState();
  const [tab, setTab] = useState<"output" | "mic">("output");

  return (
    <>
      <ProfileBar
        profiles={state.profiles}
        lastProfile={state.last_profile}
        onAdd={onAddProfile}
      />

      {!state.routing_available && (
        <Banner tone="warn">
          Roteamento por app indisponível neste Windows. Volume e saída padrão continuam
          funcionando.
        </Banner>
      )}
      {state.save_error && (
        <Banner tone="error">
          Não foi possível gravar no disco. Mudanças podem se perder ao fechar o Sonar.
          <small>{state.save_error}</small>
        </Banner>
      )}

      <Tabs
        items={[
          { id: "output", label: "Saída", icon: <Icon name="volume-2" size={17} /> },
          { id: "mic", label: "Microfone", icon: <Icon name="mic" size={17} /> },
        ]}
        active={tab}
        onChange={(id) => setTab(id === "mic" ? "mic" : "output")}
      />

      {tab === "output" ? <OutputTab state={state} /> : <MicTab state={state} />}
    </>
  );
}

function Banner({ tone, children }: { tone: "warn" | "error"; children: ReactNode }) {
  return (
    <div className={`banner ${tone}`} role={tone === "error" ? "alert" : "status"}>
      <Icon name="info" size={16} />
      <div>{children}</div>
    </div>
  );
}
