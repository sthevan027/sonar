import type { ReactNode } from "react";
import { Icon } from "../components/Icon";
import { Tabs } from "../components/Tabs";
import type { PanelTab } from "../routes";
import { useAppState } from "../store";
import { MicTab } from "./MicTab";
import { OutputTab } from "./OutputTab";
import { ProfileBar } from "./ProfileBar";

interface PanelProps {
  /** Botão "+" da barra de perfis. Ligado ao editor na Task 12. */
  onAddProfile: () => void;
  /** Aba escolhida. Vem da rota, para sobreviver a uma troca de tela. */
  tab: PanelTab;
  onTabChange: (tab: PanelTab) => void;
}

/** Painel principal (mockups A e A2): perfis, avisos, abas Saída | Microfone. */
export function Panel({ onAddProfile, tab, onTabChange }: PanelProps) {
  const state = useAppState();

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
        onChange={(id) => onTabChange(id === "mic" ? "mic" : "output")}
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
