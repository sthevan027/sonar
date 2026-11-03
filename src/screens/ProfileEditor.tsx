import { useState } from "react";
import * as api from "../api";
import { AppLogo } from "../components/AppLogo";
import { DeviceMenu } from "../components/DeviceMenu";
import { Icon } from "../components/Icon";
import { IconButton } from "../components/IconButton";
import { useToast } from "../components/Toast";
import { useAppState } from "../store";
import type { AppState, ProfileItem, RunningApp } from "../types";
import { toTarget } from "../types";
import { AppPicker } from "./AppPicker";
import {
  NAME_MAX,
  PROFILE_ICONS,
  addApp,
  buildProfile,
  choiceFromMenu,
  choiceLabel,
  choiceOf,
  deviceOfChoice,
  draftFrom,
  menuIdOf,
  nameError,
  nameLength,
  usesMic,
  type Draft,
} from "./editorModel";
import { appName, selectableDevices, toneOf } from "./model";

interface ProfileEditorProps {
  /** Perfil a editar. Ausente (ou não encontrado) = perfil novo. */
  profileId?: string;
  /** Volta para o painel, sem salvar (Cancelar) ou depois de salvar ou excluir. */
  onDone: () => void;
}

/** Id de perfil novo. `randomUUID` existe no webview do Tauri; a alternativa cobre o resto. */
function newProfileId(): string {
  const crypto = globalThis.crypto;
  if (typeof crypto?.randomUUID === "function") return crypto.randomUUID();
  return `p-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/** Editor de perfil (mockup C): ícone, nome opcional, apps e o destino de cada um. */
export function ProfileEditor({ profileId, onDone }: ProfileEditorProps) {
  const state = useAppState();
  const notify = useToast();
  const existing =
    profileId === undefined ? undefined : state.profiles.find((p) => p.id === profileId);
  const [draft, setDraft] = useState<Draft>(() => draftFrom(existing, newProfileId));
  const [picking, setPicking] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [saving, setSaving] = useState(false);

  const error = nameError(draft.name);
  const count = nameLength(draft.name);

  function changeItem(index: number, patch: Partial<ProfileItem>) {
    setDraft((d) => ({
      ...d,
      items: d.items.map((item, i) => (i === index ? { ...item, ...patch } : item)),
    }));
  }

  function removeItem(index: number) {
    setDraft((d) => ({ ...d, items: d.items.filter((_, i) => i !== index) }));
  }

  function addFromPicker(app: RunningApp) {
    setDraft((d) => ({ ...d, items: addApp(d.items, app.exe) }));
    setPicking(false);
  }

  function save() {
    if (error || saving) return;
    setSaving(true);
    api.saveProfile(buildProfile(draft)).then(onDone, (cause: unknown) => {
      setSaving(false);
      notify(typeof cause === "string" ? cause : "Não foi possível salvar o perfil.");
    });
  }

  function remove() {
    api.deleteProfile(draft.id).then(onDone, () => {
      setConfirming(false);
      notify("Não foi possível excluir o perfil.");
    });
  }

  return (
    <div className="ed">
      <div className="row ed-top">
        <IconButton
          label="Voltar"
          icon={<Icon name="arrow-left" size={18} />}
          size={34}
          onPress={onDone}
        />
        <b className="ed-title">{existing ? "Editar perfil" : "Novo perfil"}</b>
      </div>

      <div className="ed-scroll">
        <div className="row ed-head">
          <div className="ed-big" aria-hidden="true">
            <Icon name={draft.icon} size={32} />
          </div>
          <div className="ed-namebox">
            <label className="lbl" htmlFor="ed-name">
              Nome (opcional)
            </label>
            <div className="ed-field">
              <input
                id="ed-name"
                className={error ? "ed-input over" : "ed-input"}
                value={draft.name}
                autoComplete="off"
                spellCheck={false}
                aria-invalid={error !== null}
                aria-describedby={error ? "ed-name-err" : undefined}
                onChange={(event) => setDraft((d) => ({ ...d, name: event.target.value }))}
              />
              <em className={count > NAME_MAX ? "ed-count over" : "ed-count"}>
                {`${count} / ${NAME_MAX}`}
              </em>
            </div>
            {error && (
              <small id="ed-name-err" className="ed-err" role="alert">
                {error}
              </small>
            )}
          </div>
        </div>

        <div className="ed-sec">
          <div className="lbl">Ícone</div>
          <div className="ed-icons" role="group" aria-label="Ícone do perfil">
            {PROFILE_ICONS.map((icon) => (
              <IconButton
                key={icon.name}
                label={icon.label}
                icon={<Icon name={icon.name} size={18} />}
                size={38}
                selected={draft.icon === icon.name}
                onPress={() => setDraft((d) => ({ ...d, icon: icon.name }))}
              />
            ))}
          </div>
        </div>

        <div className="ed-sec">
          <div className="lbl">Apps neste perfil</div>
          <div className="ed-list">
            {draft.items.map((item, index) => (
              <ProfileItemRow
                key={`${index}-${item.exe}`}
                item={item}
                state={state}
                onChange={(patch) => changeItem(index, patch)}
                onRemove={() => removeItem(index)}
              />
            ))}
          </div>
          {picking ? (
            <AppPicker
              exclude={draft.items.map((item) => item.exe)}
              onPick={addFromPicker}
              onCancel={() => setPicking(false)}
            />
          ) : (
            <button type="button" className="ed-add" onClick={() => setPicking(true)}>
              <Icon name="plus" size={16} />
              Adicionar app
            </button>
          )}
        </div>

        <div className="row ed-note">
          <Icon name="info" size={15} />
          <span>
            Apps fora do perfil ficam como estavam. Só apps que usam microfone mostram a escolha
            de mic.
          </span>
        </div>

        {existing && !confirming && (
          <button type="button" className="ed-danger" onClick={() => setConfirming(true)}>
            Excluir perfil
          </button>
        )}
        {existing && confirming && (
          <div className="ed-confirm" role="group" aria-label="Confirmar exclusão">
            <span>Excluir este perfil? Os apps mantêm as rotas que ele já aplicou.</span>
            <div className="ed-confirm-actions">
              <button type="button" className="btn danger" onClick={remove}>
                Excluir
              </button>
              <button type="button" className="btn" onClick={() => setConfirming(false)}>
                Manter
              </button>
            </div>
          </div>
        )}
      </div>

      <div className="ed-foot">
        <button type="button" className="btn sec" onClick={onDone}>
          Cancelar
        </button>
        <button
          type="button"
          className="btn pri"
          disabled={error !== null || saving}
          onClick={save}
        >
          Salvar perfil
        </button>
      </div>
    </div>
  );
}

interface ProfileItemRowProps {
  item: ProfileItem;
  state: AppState;
  onChange: (patch: Partial<ProfileItem>) => void;
  onRemove: () => void;
}

/** Um app do perfil: chip de saída e, quando o app usa mic, chip de mic. */
function ProfileItemRow({ item, state, onChange, onRemove }: ProfileItemRowProps) {
  const name = appName(item.exe);
  const output = choiceOf(item.output);
  const input = choiceOf(item.input);

  return (
    <div className="card ed-pr">
      <div className="row ed-app">
        <AppLogo exe={item.exe} />
        <div className="ed-appname">{name}</div>
        <IconButton
          label={`Remover ${name}`}
          icon={<Icon name="x" size={16} />}
          size={32}
          onPress={onRemove}
        />
      </div>
      <div className="ed-chips">
        <DeviceMenu
          kind="device"
          tone={toneOf(deviceOfChoice(output, state.devices, "render"))}
          label={choiceLabel(output, state.devices, "render")}
          menuLabel={`Saída do ${name}`}
          devices={selectableDevices(state.devices, "render", menuIdOf(output))}
          selectedId={menuIdOf(output)}
          keepLabel="Manter atual"
          onSelect={(id) => onChange({ output: toTarget(choiceFromMenu(id)) })}
        />
        {usesMic(state, item) && (
          <DeviceMenu
            kind="mic"
            tone={toneOf(deviceOfChoice(input, state.devices, "capture"))}
            label={choiceLabel(input, state.devices, "capture")}
            menuLabel={`Microfone do ${name}`}
            devices={selectableDevices(state.devices, "capture", menuIdOf(input))}
            selectedId={menuIdOf(input)}
            keepLabel="Manter atual"
            onSelect={(id) => onChange({ input: toTarget(choiceFromMenu(id)) })}
          />
        )}
      </div>
    </div>
  );
}
