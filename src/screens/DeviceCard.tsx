import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import * as api from "../api";
import { Card } from "../components/Card";
import { Icon } from "../components/Icon";
import { IconButton } from "../components/IconButton";
import { useToast } from "../components/Toast";
import type { DeviceView } from "../types";
import { DEVICE_ICONS, iconOf, statusLine, tileToneOf } from "./devicesModel";
import { NAME_MAX, nameError, nameLength } from "./editorModel";

/** O que `update_device_meta` recebe: o cartão inteiro, não só o campo que mudou. */
interface Meta {
  /** Vazio restaura o nome do Windows. */
  name: string;
  icon: string | null;
  hidden: boolean;
}

function metaOf(device: DeviceView): Meta {
  return { name: device.name ?? "", icon: device.icon, hidden: device.hidden };
}

function sameMeta(a: Meta, b: Meta): boolean {
  return a.name === b.name && a.icon === b.icon && a.hidden === b.hidden;
}

/**
 * Fila global de gravações: a ordem de envio é a ordem de chegada no Rust. Quando uma
 * gravação falha, `onFail` roda antes da próxima da fila, então a próxima já parte do valor
 * de volta.
 */
let metaQueue: Promise<void> = Promise.resolve();

function enqueue(call: () => Promise<void>, onFail: () => void, onSettled: () => void) {
  metaQueue = metaQueue.then(() => call().catch(onFail).finally(onSettled));
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
 *
 * Toda gravação parte do último estado enviado por este cartão (`sent`), mesclado com o que
 * mudou. Assim o olho, o ícone e o nome nunca se atropelam, e gravação idêntica à anterior
 * não sai.
 */
export function DeviceCard({ device, editing, onToggleEdit, footer }: DeviceCardProps) {
  const notify = useToast();
  const name = device.display_name;
  const sent = useRef<Meta>(metaOf(device));
  const pending = useRef(0);
  const latest = useRef(device);
  const [draft, setDraft] = useState(device.name ?? "");

  useEffect(() => {
    latest.current = device;
  });

  // Estado vindo do Rust só vira a base quando nada está em voo (senão atropela o que enviamos).
  useEffect(() => {
    if (pending.current === 0) sent.current = metaOf(device);
  }, [device.name, device.icon, device.hidden]); // eslint-disable-line react-hooks/exhaustive-deps

  /** Nome para a gravação: o rascunho se a edição está aberta e o nome é válido; senão, o último enviado. */
  function writableName(): string {
    return editing && !nameError(draft) ? draft.trim() : sent.current.name;
  }

  function save(patch: Partial<Meta>) {
    const next: Meta = { ...sent.current, ...patch };
    if (sameMeta(next, sent.current)) return;
    sent.current = next;
    pending.current += 1;
    enqueue(
      () => api.updateDeviceMeta(device.id, next.name, next.icon, next.hidden),
      () => {
        notify("Não foi possível salvar o dispositivo.");
        sent.current = metaOf(latest.current);
      },
      () => {
        pending.current -= 1;
      },
    );
  }

  function commitName() {
    if (!editing || nameError(draft)) return;
    save({ name: draft.trim() });
  }

  function restore() {
    setDraft("");
    save({ name: "" });
  }

  function toggleEdit() {
    setDraft(sent.current.name);
    onToggleEdit();
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
            onPress={() => save({ name: writableName(), hidden: !sent.current.hidden })}
          />
          <IconButton
            label={editing ? `Fechar edição de ${name}` : `Editar ${name}`}
            icon={<Icon name="pencil" size={16} />}
            size={32}
            selected={editing}
            onPress={toggleEdit}
          />
        </div>
      </div>
      {editing && (
        <DeviceEditor
          device={device}
          draft={draft}
          onDraft={setDraft}
          onCommit={commitName}
          onRestore={restore}
          onIcon={(icon) => save({ name: writableName(), icon })}
        />
      )}
      {footer}
    </Card>
  );
}

interface DeviceEditorProps {
  device: DeviceView;
  draft: string;
  onDraft: (value: string) => void;
  onCommit: () => void;
  onRestore: () => void;
  onIcon: (icon: string) => void;
}

/** Campos da edição: nome com contador, grade de ícones e restaurar. */
function DeviceEditor({ device, draft, onDraft, onCommit, onRestore, onIcon }: DeviceEditorProps) {
  const error = nameError(draft);
  const count = nameLength(draft);
  const inputId = `dev-name-${device.id}`;
  const errorId = `dev-name-err-${device.id}`;

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") {
      event.preventDefault();
      onCommit();
    }
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
            onChange={(event) => onDraft(event.target.value)}
            onBlur={onCommit}
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
              onPress={() => onIcon(option.name)}
            />
          ))}
        </div>
      </div>

      <button type="button" className="rst" onClick={onRestore}>
        <Icon name="rotate-ccw" size={14} />
        Restaurar nome original do Windows
      </button>
    </>
  );
}
