import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Icon } from "../components/Icon";
import { IconButton } from "../components/IconButton";
import { Slider } from "../components/Slider";
import type { Action, Binding, DeviceView, Direction, Profile } from "../types";
import { AppPicker } from "./AppPicker";
import {
  groupedChoices,
  actionOfKind,
  bindingDetail,
  bindingTitle,
  duplicateMessage,
  hasParameters,
  missingParameter,
  validateAccelerator,
  withDelta,
  withDevice,
  withDirection,
  withExe,
  withMuteState,
  muteState,
  muteStateLabel,
  muteStateOptions,
  familyKind,
  withProfile,
  withValue,
} from "./bindingsModel";
import { appName } from "./model";
import { shortcutFromKeys, shortcutKeys } from "./settingsModel";

interface BindingEditorProps {
  /** Binding editado. Ausente = atalho novo. */
  initial?: Binding;
  /** Os outros bindings da lista (checagem de duplicata). */
  others: Binding[];
  profiles: Profile[];
  devices: DeviceView[];
  /** Frase do backend quando a gravação foi recusada. */
  error: string | null;
  onSave: (binding: Binding) => void;
  onRemove: () => void;
  onCancel: () => void;
  /** O usuário mudou a ação ou o atalho: o erro do backend deixou de valer. */
  onEdit?: () => void;
}

/** Qual seletor de parâmetro está aberto. */
type PickerKind = "profile" | "app" | "device" | "direction" | "muted" | "delta";

/**
 * Editor de atalho (tela inteira, como o editor de perfil). A ação vem do seletor, o
 * acelerador da captura de teclas e os parâmetros da ação, logo abaixo. O botão Salvar
 * fica travado enquanto faltar parâmetro ou houver erro de validação.
 */
export function BindingEditor({
  initial,
  others,
  profiles,
  devices,
  error,
  onSave,
  onRemove,
  onCancel,
  onEdit,
}: BindingEditorProps) {
  const [action, setAction] = useState<Action>(initial?.action ?? actionOfKind("toggle_panel"));
  const [accelerator, setAccelerator] = useState<string>(initial?.accelerator ?? "");
  const [capturing, setCapturing] = useState(false);
  const [hint, setHint] = useState<string | null>(null);
  const [touched, setTouched] = useState(false);
  const [picker, setPicker] = useState<PickerKind | null>(null);
  const [confirming, setConfirming] = useState(false);
  const keysRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (capturing) keysRef.current?.focus();
  }, [capturing]);

  // O erro que o backend devolveu era sobre o atalho/ação anteriores: trocou, saiu da tela (M12).
  const firstRun = useRef(true);
  useEffect(() => {
    if (firstRun.current) {
      firstRun.current = false;
      return;
    }
    onEdit?.();
  }, [action, accelerator]); // eslint-disable-line react-hooks/exhaustive-deps

  const duplicate = duplicateMessage(accelerator, others);
  const invalid = validateAccelerator(accelerator);
  const missing = missingParameter(action);
  // A duplicata vem primeiro (RF-16): é o erro que o usuário mais encontra.
  const localError = duplicate ?? invalid ?? missing;
  // Antes de o usuário tocar em nada, a tela não acusa erro de atalho vazio.
  const showError =
    localError !== null && (touched || accelerator.trim() !== "" || missing !== null);
  const shownError = error ?? hint ?? (showError ? localError : null);
  const canSave = localError === null && !capturing;

  const keys = accelerator ? shortcutKeys(accelerator) : [];

  function selectKind(kind: Action["type"]) {
    // Trocar de família reinicia o parâmetro; ficar na mesma família mantém a escolha.
    setAction((current) => (familyKind(current.type) === kind ? current : actionOfKind(kind)));
    setPicker(null);
  }

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    if (!capturing) return;
    if (event.key === "Tab") return;
    event.preventDefault();
    if (event.key === "Escape") {
      setCapturing(false);
      setHint(null);
      return;
    }
    const outcome = shortcutFromKeys(event);
    if (outcome.status === "wait") return;
    if (outcome.status === "invalid") {
      setHint(outcome.message);
      return;
    }
    setCapturing(false);
    setHint(null);
    setTouched(true);
    setAccelerator(outcome.shortcut);
  }

  const outputs = devices.filter((d) => d.flow === "render");

  return (
    <div className="ed">
      <div className="row ed-top">
        <IconButton
          label="Voltar"
          icon={<Icon name="arrow-left" size={18} />}
          size={34}
          onPress={onCancel}
        />
        <b className="ed-title">{initial ? "Editar atalho" : "Adicionar atalho"}</b>
      </div>

      <div className="ed-scroll">
        <div className="ed-sec">
          <div className="lbl">Ação</div>
          <div className="picker">
            {groupedChoices().map(({ group, choices }) => (
              <div key={group} className="pick-group">
                <div className="pick-group-title">{group}</div>
                <ul className="pick-list pick-list-all" aria-label={group}>
                  {choices.map((choice) => (
                    <li key={choice.kind}>
                      <button
                        type="button"
                        className={choice.kind === familyKind(action.type) ? "pick on" : "pick"}
                        onClick={() => selectKind(choice.kind)}
                      >
                        <Icon name={choice.icon} size={16} />
                        <span>{choice.label}</span>
                        {choice.kind === familyKind(action.type) ? (
                          <Icon name="check" size={15} />
                        ) : (
                          choice.hint && <em className="cnt">{choice.hint}</em>
                        )}
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            ))}
          </div>
        </div>

        <div className="ed-sec">
          <div className="lbl">Atalho</div>
          <div className="row">
            <button
              ref={keysRef}
              type="button"
              className="keys-btn"
              aria-label={
                capturing
                  ? "Digite a combinação do atalho"
                  : `Atalho: ${keys.length ? keys.join(" ") : "—"}. Toque para trocar.`
              }
              onClick={() => {
                setHint(null);
                setCapturing(true);
              }}
              onKeyDown={onKeyDown}
              onBlur={() => {
                setCapturing(false);
                setHint(null);
              }}
            >
              {capturing ? (
                <span className="keys-hint">Pressione a combinação</span>
              ) : keys.length ? (
                keys.map((key) => (
                  <span key={key} className="k">
                    {key}
                  </span>
                ))
              ) : (
                <span className="k">—</span>
              )}
            </button>
          </div>
          {shownError && (
            <span className="ed-err" role="alert">
              {shownError}
            </span>
          )}
          <span className="ed-note">
            Clique nas teclas para trocar a combinação. Esc cancela a captura.
          </span>
        </div>

        {hasParameters(action.type) && (
          <div className="ed-sec">
            <div className="lbl">Parâmetros</div>
            <div className="grp">{parametersOf(action, picker, setPicker, setAction, profiles, outputs)}</div>
            {picker === "profile" && (
              <div className="picker">
                {profiles.length === 0 ? (
                  <p className="ed-note">Nenhum perfil criado.</p>
                ) : (
                  <ul className="pick-list" aria-label="Perfis">
                    {profiles.map((profile) => (
                      <li key={profile.id}>
                        <button
                          type="button"
                          className={
                            action.type === "activate_profile" && action.profile_id === profile.id
                              ? "pick on"
                              : "pick"
                          }
                          onClick={() => {
                            setAction(withProfile(action, profile.id));
                            setPicker(null);
                          }}
                        >
                          <Icon name="layers" size={16} />
                          <span>{profile.name ?? profile.id}</span>
                        </button>
                      </li>
                    ))}
                  </ul>
                )}
                <button type="button" className="ed-link" onClick={() => setPicker(null)}>
                  Cancelar
                </button>
              </div>
            )}
            {picker === "app" && (
              <AppPicker
                exclude={[]}
                onPick={(app) => {
                  setAction(withExe(action, app.exe));
                  setPicker(null);
                }}
                onCancel={() => setPicker(null)}
              />
            )}
            {picker === "device" && (
              <div className="picker">
                <ul className="pick-list" aria-label="Saídas">
                  <li>
                    <button
                      type="button"
                      className={
                        action.type === "move_app" && action.device === null ? "pick on" : "pick"
                      }
                      onClick={() => {
                        setAction(withDevice(action, null));
                        setPicker(null);
                      }}
                    >
                      <span>Padrão</span>
                    </button>
                  </li>
                  {outputs.map((device) => (
                    <li key={device.id}>
                      <button
                        type="button"
                        className={
                          action.type === "move_app" && action.device === device.id ? "pick on" : "pick"
                        }
                        onClick={() => {
                          setAction(withDevice(action, device.id));
                          setPicker(null);
                        }}
                      >
                        <Icon name="speaker" size={16} />
                        <span>{device.display_name}</span>
                      </button>
                    </li>
                  ))}
                </ul>
                <button type="button" className="ed-link" onClick={() => setPicker(null)}>
                  Cancelar
                </button>
              </div>
            )}
            {picker === "direction" && (
              <div className="picker">
                <ul className="pick-list" aria-label="Direção">
                  {directionOptions(action).map((option) => (
                    <li key={option.direction}>
                      <button
                        type="button"
                        className={
                          (action.type === "cycle_profile" ||
                            action.type === "cycle_default_output") &&
                          action.direction === option.direction
                            ? "pick on"
                            : "pick"
                        }
                        onClick={() => {
                          setAction(withDirection(action, option.direction));
                          setPicker(null);
                        }}
                      >
                        <span>{option.label}</span>
                      </button>
                    </li>
                  ))}
                </ul>
                <button type="button" className="ed-link" onClick={() => setPicker(null)}>
                  Cancelar
                </button>
              </div>
            )}
            {picker === "muted" && (
              <div className="picker">
                <ul className="pick-list" aria-label="Estado">
                  {muteStateOptions(action).map((option) => (
                    <li key={option.state}>
                      <button
                        type="button"
                        className={muteState(action) === option.state ? "pick on" : "pick"}
                        onClick={() => {
                          setAction(withMuteState(action, option.state));
                          setPicker(null);
                        }}
                      >
                        <span>{option.label}</span>
                      </button>
                    </li>
                  ))}
                </ul>
                <button type="button" className="ed-link" onClick={() => setPicker(null)}>
                  Cancelar
                </button>
              </div>
            )}
            {picker === "delta" && (
              <div className="picker">
                <ul className="pick-list" aria-label="Passo">
                  {deltaOptions.map((option) => (
                    <li key={option.delta}>
                      <button
                        type="button"
                        className={
                          action.type === "step_output_volume" && action.delta === option.delta
                            ? "pick on"
                            : "pick"
                        }
                        onClick={() => {
                          setAction(withDelta(action, option.delta));
                          setPicker(null);
                        }}
                      >
                        <span>{option.label}</span>
                      </button>
                    </li>
                  ))}
                </ul>
                <button type="button" className="ed-link" onClick={() => setPicker(null)}>
                  Cancelar
                </button>
              </div>
            )}
            <span className="ed-note">
              {action.type === "move_app"
                ? "“Padrão” devolve o app para a saída padrão do Windows."
                : "O parâmetro é gravado junto com o atalho."}
            </span>
          </div>
        )}

        {initial && !confirming && (
          <button type="button" className="ed-danger" onClick={() => setConfirming(true)}>
            Remover atalho
          </button>
        )}
        {initial && confirming && (
          <div className="ed-confirm" role="group" aria-label="Confirmar remoção">
            <span>Remover este atalho? Os outros atalhos continuam.</span>
            <div className="ed-confirm-actions">
              <button type="button" className="btn danger" onClick={onRemove}>
                Remover
              </button>
              <button type="button" className="btn" onClick={() => setConfirming(false)}>
                Manter
              </button>
            </div>
          </div>
        )}
      </div>

      <div className="ed-foot">
        <button type="button" className="btn sec" onClick={onCancel}>
          Cancelar
        </button>
        <button
          type="button"
          className="btn pri"
          disabled={!canSave}
          onClick={() => onSave({ accelerator: accelerator.trim(), action })}
        >
          Salvar atalho
        </button>
      </div>
    </div>
  );
}

const deltaOptions = [
  { delta: 0.05, label: "Subir 5%" },
  { delta: -0.05, label: "Baixar 5%" },
];

/** As duas direções de um ciclo: o rótulo muda com a ação (perfil ou saída). */
function directionOptions(action: Action): { direction: Direction; label: string }[] {
  if (action.type === "cycle_default_output") {
    return [
      { direction: "next", label: "Próxima saída" },
      { direction: "prev", label: "Saída anterior" },
    ];
  }
  return [
    { direction: "next", label: "Próximo" },
    { direction: "prev", label: "Anterior" },
  ];
}

/** Uma linha de campo: rótulo à esquerda, valor à direita. */
function ParamField({
  label,
  value,
  placeholder,
  onClick,
}: {
  label: string;
  value: string;
  placeholder?: string;
  onClick: () => void;
}) {
  return (
    <button type="button" className="it" onClick={onClick}>
      <div className="tx">
        <span>{label}</span>
      </div>
      <span
        className="cnt"
        style={value ? { color: "var(--text)", fontSize: 13 } : undefined}
      >
        {value || placeholder || "Escolher"}
      </span>
      <Icon name="chevron-down" size={15} />
    </button>
  );
}

/** Os campos de parâmetro da ação escolhida. */
function parametersOf(
  action: Action,
  picker: PickerKind | null,
  setPicker: (kind: PickerKind | null) => void,
  setAction: (action: Action) => void,
  profiles: Profile[],
  outputs: DeviceView[],
): ReactNode {
  const open = (kind: PickerKind) => setPicker(picker === kind ? null : kind);

  switch (action.type) {
    case "activate_profile": {
      const profile = profiles.find((p) => p.id === action.profile_id);
      return (
        <ParamField
          label="Perfil"
          value={profile ? (profile.name ?? profile.id) : ""}
          placeholder={profiles.length ? "Escolher perfil" : "Nenhum perfil criado"}
          onClick={() => open("profile")}
        />
      );
    }
    case "cycle_profile":
      return (
        <ParamField
          label="Direção"
          value={action.direction === "next" ? "Próximo" : "Anterior"}
          onClick={() => open("direction")}
        />
      );
    case "cycle_default_output":
      return (
        <ParamField
          label="Direção"
          value={action.direction === "next" ? "Próxima saída" : "Saída anterior"}
          onClick={() => open("direction")}
        />
      );
    case "toggle_mic_mute":
    case "set_mic_mute":
    case "toggle_output_mute":
    case "set_output_mute":
      return (
        <ParamField
          label="Estado"
          value={muteStateLabel(action)}
          onClick={() => open("muted")}
        />
      );
    case "step_output_volume":
      return (
        <ParamField
          label="Passo"
          value={action.delta > 0 ? "Subir 5%" : action.delta < 0 ? "Baixar 5%" : "Ajustar"}
          onClick={() => open("delta")}
        />
      );
    case "set_output_volume":
      return (
        <div className="it">
          <div className="tx">
            <span>Valor</span>
          </div>
          <div style={{ flex: 1, minWidth: 0, marginLeft: "auto" }}>
            <Slider
              value={Math.round(action.value * 100)}
              onChange={(percent) => setAction(withValue(action, percent / 100))}
              label="Valor do volume"
            />
          </div>
        </div>
      );
    case "set_app_mute":
      return (
        <>
          <ParamField
            label="App"
            value={action.exe ? appName(action.exe) : ""}
            placeholder="Escolher app"
            onClick={() => open("app")}
          />
          <ParamField
            label="Estado"
            value={muteStateLabel(action)}
            onClick={() => open("muted")}
          />
        </>
      );
    case "move_app":
      return (
        <>
          <ParamField
            label="App"
            value={action.exe ? appName(action.exe) : ""}
            placeholder="Escolher app"
            onClick={() => open("app")}
          />
          <ParamField
            label="Saída"
            value={
              action.device === null
                ? "Padrão"
                : (outputs.find((d) => d.id === action.device)?.display_name ?? "")
            }
            placeholder={action.device === null ? "" : "Escolher saída"}
            onClick={() => open("device")}
          />
        </>
      );
    default:
      return null;
  }
}

/** Rótulo do alvo: usado no editor para mostrar o que o atalho vai fazer. */
export function describeBinding(binding: Binding, profiles: Profile[], devices: DeviceView[]): string {
  const ctx = { profiles, devices };
  return `${bindingTitle(binding.action, ctx)} — ${bindingDetail(binding.action, ctx)}`;
}
