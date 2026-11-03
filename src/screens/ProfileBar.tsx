import * as api from "../api";
import { IconButton } from "../components/IconButton";
import { useToast } from "../components/Toast";
import { Icon } from "../components/Icon";
import { isIconName } from "../icons";
import type { Profile } from "../types";
import { attempt } from "./model";

interface ProfileBarProps {
  profiles: Profile[];
  /** Último perfil ativado nesta execução (o que aparece marcado). */
  lastProfile: string | null;
  /** Botão "+": placeholder até a Task 12 ligar o editor de perfil. */
  onAdd: () => void;
}

/** Linha de perfis no topo do painel (mockup A). Ativar um perfil é um clique. */
export function ProfileBar({ profiles, lastProfile, onAdd }: ProfileBarProps) {
  const notify = useToast();
  const current = profiles.find((p) => p.id === lastProfile);

  return (
    <div className="prof">
      {profiles.map((profile) => (
        <IconButton
          key={profile.id}
          label={profileName(profile)}
          icon={<Icon name={isIconName(profile.icon) ? profile.icon : "layers"} size={20} />}
          size={44}
          selected={profile.id === lastProfile}
          onPress={() =>
            // Perfil que não existe mais volta como erro do Rust: aparece como aviso.
            attempt(api.activateProfile(profile.id), () =>
              notify(`Não foi possível ativar o perfil ${profileName(profile)}.`),
            )
          }
        />
      ))}
      <span className="pslot add">
        <IconButton label="Novo perfil" icon={<Icon name="plus" size={20} />} size={44} onPress={onAdd} />
      </span>
      <div className="pname">
        {current ? (
          <>
            <b>{profileName(current)}</b>
            <span>{summary(current)}</span>
          </>
        ) : profiles.length > 0 ? (
          <>
            <b>Nenhum perfil ativo</b>
            <span>toque num perfil para ativar</span>
          </>
        ) : (
          <>
            <b>Sem perfis</b>
            <span>crie um para mudar vários apps de uma vez</span>
          </>
        )}
      </div>
    </div>
  );
}

function profileName(profile: Profile): string {
  return profile.name?.trim() || "Perfil sem nome";
}

/** Resumo do que o perfil mexe: "saída", "microfone" ou os dois. */
function summary(profile: Profile): string {
  const parts: string[] = [];
  if (profile.items.some((item) => item.output !== undefined)) parts.push("saída");
  if (profile.items.some((item) => item.input !== undefined)) parts.push("microfone");
  if (parts.length > 0) return parts.join(" + ");
  const count = profile.items.length;
  return `${count} ${count === 1 ? "app" : "apps"}`;
}
