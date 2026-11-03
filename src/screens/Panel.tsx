import type { ReactNode } from "react";
import { Icon } from "../components/Icon";
import { IconButton } from "../components/IconButton";
import { Tabs } from "../components/Tabs";
import type { PanelTab } from "../routes";
import { useAppState } from "../store";
import { MicTab } from "./MicTab";
import { OutputTab } from "./OutputTab";
import { ProfileBar } from "./ProfileBar";

interface PanelProps {
  /** Botão "+" da barra de perfis: abre o editor de perfil novo. */
  onAddProfile: () => void;
  /** Abre o editor de um perfil existente. */
  onEditProfile: (profileId: string) => void;
  /** Engrenagem: abre Configurações. */
  onOpenSettings: () => void;
  /** Aba escolhida. Vem da rota, para sobreviver a uma troca de tela. */
  tab: PanelTab;
  onTabChange: (tab: PanelTab) => void;
}

/** Painel principal (mockups A e A2): perfis, avisos, abas Saída | Microfone. */
export function Panel({
  onAddProfile,
  onEditProfile,
  onOpenSettings,
  tab,
  onTabChange,
}: PanelProps) {
  const state = useAppState();

  return (
    <>
      <ProfileBar
        profiles={state.profiles}
        lastProfile={state.last_profile}
        onAdd={onAddProfile}
        onEdit={onEditProfile}
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

      <div className="row">
        <div className="tabs-row">
          <Tabs
            items={[
              { id: "output", label: "Saída", icon: <Icon name="volume-2" size={17} /> },
              { id: "mic", label: "Microfone", icon: <Icon name="mic" size={17} /> },
            ]}
            active={tab}
            onChange={(id) => onTabChange(id === "mic" ? "mic" : "output")}
          />
        </div>
        <IconButton
          label="Configurações"
          icon={<Icon name="settings" size={17} />}
          size={34}
          onPress={onOpenSettings}
        />
      </div>

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
