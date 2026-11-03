import { useEffect, useRef } from "react";
import * as api from "../api";
import { IconButton } from "../components/IconButton";
import { useToast } from "../components/Toast";
import { Icon } from "../components/Icon";
import { isIconName } from "../icons";
import type { Profile } from "../types";
import { attempt } from "./model";

/** Quanto tempo o botão precisa ficar pressionado para abrir a edição do perfil. */
export const LONG_PRESS_MS = 600;

interface ProfileBarProps {
  profiles: Profile[];
  /** Último perfil ativado nesta execução (o que aparece marcado). */
  lastProfile: string | null;
  /** Botão "+": abre o editor de perfil novo. */
  onAdd: () => void;
  /** Abre o editor de um perfil: toque longo, clique direito, ou o lápis do perfil ativo. */
  onEdit: (profileId: string) => void;
}

/** Linha de perfis no topo do painel (mockup A). Ativar um perfil é um clique. */
export function ProfileBar({ profiles, lastProfile, onAdd, onEdit }: ProfileBarProps) {
  const notify = useToast();
  const current = profiles.find((p) => p.id === lastProfile);
  const pressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Depois de um toque longo, o clique que chega em seguida não deve ativar o perfil.
  const skipClick = useRef(false);

  function cancelPress() {
    if (pressTimer.current !== null) clearTimeout(pressTimer.current);
    pressTimer.current = null;
  }

  function startPress(profileId: string) {
    cancelPress();
    skipClick.current = false;
    pressTimer.current = setTimeout(() => {
      pressTimer.current = null;
      skipClick.current = true;
      onEdit(profileId);
    }, LONG_PRESS_MS);
  }

  useEffect(
    () => () => {
      if (pressTimer.current !== null) clearTimeout(pressTimer.current);
    },
    [],
  );

  return (
    <div className="prof">
      {profiles.map((profile) => (
        <span
          key={profile.id}
          className="pslot"
          onPointerDown={() => startPress(profile.id)}
          onPointerUp={cancelPress}
          onPointerLeave={cancelPress}
          onPointerCancel={cancelPress}
          onContextMenu={(event) => {
            event.preventDefault();
            cancelPress();
            onEdit(profile.id);
          }}
        >
          <IconButton
            label={profileName(profile)}
            icon={<Icon name={isIconName(profile.icon) ? profile.icon : "layers"} size={20} />}
            size={44}
            selected={profile.id === lastProfile}
            onPress={() => {
              if (skipClick.current) {
                skipClick.current = false;
                return;
              }
              // Perfil que não existe mais volta como erro do Rust: aparece como aviso.
              attempt(api.activateProfile(profile.id), () =>
                notify(`Não foi possível ativar o perfil ${profileName(profile)}.`),
              );
            }}
          />
        </span>
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
      {current && (
        <IconButton
          label={`Editar ${profileName(current)}`}
          icon={<Icon name="pencil" size={16} />}
          size={32}
          onPress={() => onEdit(current.id)}
        />
      )}
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
