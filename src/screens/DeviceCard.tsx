import { useState, type KeyboardEvent, type ReactNode } from "react";
import * as api from "../api";
import { Card } from "../components/Card";
import { Icon } from "../components/Icon";
import { IconButton } from "../components/IconButton";
import { useToast } from "../components/Toast";
import type { DeviceView } from "../types";
import { DEVICE_ICONS, iconOf, statusLine, tileToneOf } from "./devicesModel";
import { NAME_MAX, nameError, nameLength } from "./editorModel";

/** O que `update_device_meta` recebe: o cartão inteiro, não só o campo que mudou. */
interface MetaPatch {
  /** Vazio restaura o nome do Windows. */
  name: string;
  icon: string | null;
  hidden: boolean;
}

/**
 * Gravações de metadados saem em fila: cada uma leva o cartão inteiro, então a ordem de
 * chegada no Rust tem de ser a ordem de envio (ex.: salvar o nome no blur e trocar o ícone
 * no clique que vem logo depois).
 */
let metaQueue: Promise<void> = Promise.resolve();

function enqueue(call: () => Promise<void>, onFail: () => void) {
  metaQueue = metaQueue.then(call).then(undefined, onFail);
}

/** Classe da linha de estado: verde no texto só quando conectado e visível; bolinha verde quando conectado. */
function statusClass(device: DeviceView): string {
  if (!device.connected) return "st";
  return device.hidden ? "st conn" : "st conn live";
}

interface DeviceCardProps {
  device: DeviceView;
  /** Cartão em edição: aparecem nome, ícone e restaurar. */
  editing: boolean;
  onToggleEdit: () => void;
  /** Conteúdo extra no fim do cartão (medidor do mic padrão). */
  footer?: ReactNode;
}

/**
 * Cartão de um dispositivo (mockups D e D2). Olho oculta do painel; lápis abre a edição.
 * Desconectado ou oculto aparece apagado; desconectado com regras guardadas diz isso.
 */
export function DeviceCard({ device, editing, onToggleEdit, footer }: DeviceCardProps) {
  const notify = useToast();
  const name = device.display_name;

  function save(next: MetaPatch) {
    enqueue(
      () => api.updateDeviceMeta(device.id, next.name, next.icon, next.hidden),
      () => notify("Não foi possível salvar o dispositivo."),
    );
  }

  const tone = editing ? "edit" : !device.connected || device.hidden ? "off" : "default";

  return (
    <Card tone={tone}>
      <div className="row">
        <div className={`tile ${tileToneOf(device)}`}>
          <Icon name={iconOf(device)} size={20} />
        </div>
        <div className="who">
          <b>{name}</b>
          <span className="o">{device.windows_name}</span>
          <div className={statusClass(device)}>
            <i aria-hidden="true" />
            {statusLine(device)}
          </div>
        </div>
        <div className="act">
          <IconButton
            label={device.hidden ? `Mostrar ${name} no painel` : `Ocultar ${name} no painel`}
            icon={<Icon name={device.hidden ? "eye-off" : "eye"} size={16} />}
            size={32}
            onPress={() =>
              save({ name: device.name ?? "", icon: device.icon, hidden: !device.hidden })
            }
          />
          <IconButton
            label={editing ? `Fechar edição de ${name}` : `Editar ${name}`}
            icon={<Icon name="pencil" size={16} />}
            size={32}
            selected={editing}
            onPress={onToggleEdit}
          />
        </div>
      </div>
      {editing && <DeviceEditor device={device} onSave={save} />}
      {footer}
    </Card>
  );
}

/** Campos da edição. Monta ao abrir a edição, então o rascunho começa do nome salvo. */
function DeviceEditor({ device, onSave }: { device: DeviceView; onSave: (next: MetaPatch) => void }) {
  const [draft, setDraft] = useState(device.name ?? "");
  const error = nameError(draft);
  const count = nameLength(draft);
  const committedName = device.name ?? "";
  // Nome que vai junto em qualquer gravação: o rascunho se for válido; senão, o último salvo.
  const pendingName = error ? committedName : draft.trim();
  const inputId = `dev-name-${device.id}`;
  const errorId = `dev-name-err-${device.id}`;

  function commitName() {
    if (error || pendingName === committedName) return;
    onSave({ name: pendingName, icon: device.icon, hidden: device.hidden });
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") {
      event.preventDefault();
      commitName();
    }
  }

  function restore() {
    setDraft("");
    onSave({ name: "", icon: device.icon, hidden: device.hidden });
  }

  return (
    <>
      <div className="ed-sec">
        <label className="lbl" htmlFor={inputId}>
          Nome no painel
        </label>
        <div className="ed-field">
          <input
            id={inputId}
            className={error ? "ed-input over" : "ed-input"}
            value={draft}
            placeholder={device.windows_name}
            autoComplete="off"
            spellCheck={false}
            aria-invalid={error !== null}
            aria-describedby={error ? errorId : undefined}
            onChange={(event) => setDraft(event.target.value)}
            onBlur={commitName}
            onKeyDown={onKeyDown}
          />
          <em className={count > NAME_MAX ? "ed-count over" : "ed-count"}>
            {`${count} / ${NAME_MAX}`}
          </em>
        </div>
        {error && (
          <small id={errorId} className="ed-err" role="alert">
            {error}
          </small>
        )}
      </div>

      <div className="ed-sec">
        <div className="lbl">Ícone</div>
        <div className="dev-icons" role="group" aria-label="Ícone do dispositivo">
          {DEVICE_ICONS.map((option) => (
            <IconButton
              key={option.name}
              label={option.label}
              icon={<Icon name={option.name} size={18} />}
              size={38}
              selected={iconOf(device) === option.name}
              onPress={() =>
                onSave({ name: pendingName, icon: option.name, hidden: device.hidden })
              }
            />
          ))}
        </div>
      </div>

      <button type="button" className="rst" onClick={restore}>
        <Icon name="rotate-ccw" size={14} />
        Restaurar nome original do Windows
      </button>
    </>
  );
}
