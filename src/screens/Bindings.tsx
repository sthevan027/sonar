import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import * as api from "../api";
import { Icon } from "../components/Icon";
import { IconButton } from "../components/IconButton";
import { useToast } from "../components/Toast";
import { useAppState } from "../store";
import type { Binding, BindingStatus, BindingStatusEntry } from "../types";
import { BindingEditor } from "./BindingEditor";
import {
  bindingDetail,
  bindingIcon,
  bindingIsBrass,
  bindingTitle,
  bindingsOf,
  duplicateMessage,
  sameBindings,
  validateAccelerator,
} from "./bindingsModel";
import { normalizeAccelerator, shortcutFromKeys, shortcutKeys, suggestProfileBindings } from "./settingsModel";

interface BindingsProps {
  onBack: () => void;
}

/** Editor aberto: qual binding está sendo editado (`null` = atalho novo). */
interface EditorState {
  index: number | null;
  initial?: Binding;
}

/**
 * Tela de atalhos globais (sub-tela de Configurações). Lista os bindings, deixa adicionar,
 * editar, remover, sugerir atalhos de perfil e restaurar os padrões.
 *
 * "Indisponível" é estado da linha (o Windows recusou o registro): o motivo fica na própria
 * linha e os outros bindings continuam valendo. Já a falha de execução de uma ação vira
 * Toast — são dois problemas diferentes.
 */
export function Bindings({ onBack }: BindingsProps) {
  const state = useAppState();
  const notify = useToast();
  const stored = useMemo(() => bindingsOf(state.settings), [state.settings]);
  const [list, setList] = useState<Binding[]>(() => stored ?? []);
  const [statuses, setStatuses] = useState<BindingStatusEntry[]>([]);
  const [editor, setEditor] = useState<EditorState | null>(null);
  const [editorError, setEditorError] = useState<string | null>(null);
  const [confirmRestore, setConfirmRestore] = useState(false);
  // "Restaurar padrões" busca os padrões no Rust antes de gravar: o botão fica travado
  // enquanto a busca e a gravação acontecem (Ruling 42).
  const [restoring, setRestoring] = useState(false);
  // Uma gravação por vez: cada `save_bindings` parte da lista que a tela tem agora, e duas
  // em voo partiriam da mesma lista antiga (a segunda desfaria a primeira).
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);

  // Estado de registro dos bindings: uma entrada por binding, casada pelo acelerador
  // (a ordem muda quando um binding sai do meio da lista). A falha de execução de uma ação
  // (Toast) é ouvida no App, em qualquer tela.
  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    api.getBindingStatus().then(
      (loaded) => {
        if (alive) setStatuses(loaded);
      },
      () => undefined,
    );
    api
      .onBindingsStatus((loaded) => {
        if (alive) setStatuses(loaded);
      })
      .then(
        (stop) => {
          if (alive) unlisten = stop;
          else stop();
        },
        () => undefined,
      );
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  // O store é a fonte da verdade: quando ele muda a lista, a tela acompanha.
  useEffect(() => {
    if (!stored) return;
    setList((current) => (sameBindings(current, stored) ? current : stored));
  }, [stored]);

  const statusByAccelerator = useMemo(() => {
    const map = new Map<string, BindingStatus>();
    for (const entry of statuses) map.set(normalizeAccelerator(entry.accelerator), entry.status);
    return map;
  }, [statuses]);

  function statusOf(binding: Binding): BindingStatus {
    return statusByAccelerator.get(normalizeAccelerator(binding.accelerator)) ?? { type: "active" };
  }

  /** Roda uma gravação; ignora o pedido se já há uma em voo. */
  function exclusive(run: () => Promise<unknown>): Promise<unknown> {
    if (savingRef.current) return Promise.resolve();
    savingRef.current = true;
    setSaving(true);
    return run().finally(() => {
      savingRef.current = false;
      setSaving(false);
    });
  }

  /** Grava a lista inteira. A falha não muda a lista da tela. */
  function save(next: Binding[], opts: { onSuccess?: () => void } = {}) {
    return exclusive(() =>
      api.saveBindings(next).then(
        (loaded) => {
          setList(next);
          setStatuses(loaded);
          opts.onSuccess?.();
        },
        (cause: unknown) => {
          notify(errorMessage(cause, "Não foi possível salvar a configuração."));
        },
      ),
    );
  }

  function saveBinding(binding: Binding) {
    if (!editor) return;
    const next =
      editor.index === null
        ? [...list, binding]
        : list.map((item, i) => (i === editor.index ? binding : item));
    void exclusive(() =>
      api.saveBindings(next).then(
        (loaded) => {
          setList(next);
          setStatuses(loaded);
          setEditor(null);
          setEditorError(null);
        },
        (cause: unknown) => {
          // Nada é gravado: o editor continua aberto com a frase do backend.
          setEditorError(errorMessage(cause, "Não foi possível salvar a configuração."));
        },
      ),
    );
  }

  function removeBinding(index: number) {
    save(list.filter((_, i) => i !== index));
    setEditor(null);
    setEditorError(null);
  }

  /** Troca o acelerador de um binding direto na lista. */
  function changeAccelerator(index: number, accelerator: string) {
    const others = list.filter((_, i) => i !== index);
    const problem = duplicateMessage(accelerator, others) ?? validateAccelerator(accelerator);
    if (problem) {
      notify(problem);
      return;
    }
    save(list.map((item, i) => (i === index ? { ...item, accelerator } : item)));
  }

  /** Sugere atalhos de perfil e aplica na hora (D5). */
  function suggest() {
    const suggestions = suggestProfileBindings(state.profiles, list);
    if (suggestions.length === 0) {
      notify("Nenhum perfil sem atalho.");
      return;
    }
    save([...list, ...suggestions], {
      onSuccess: () =>
        notify(
          `${suggestions.length} ${suggestions.length === 1 ? "atalho adicionado" : "atalhos adicionados"}.`,
        ),
    });
  }

  /**
   * "Restaurar padrões" substitui a lista inteira (D4). Os padrões vêm do Rust, pelo
   * comando `get_default_bindings` — não há cópia deles no TypeScript (Ruling 42).
   */
  function restoreDefaults() {
    setConfirmRestore(false);
    setRestoring(true);
    api
      .getDefaultBindings()
      .then((patterns) => save(patterns))
      .catch(() => notify("Não foi possível restaurar os atalhos padrão."))
      .finally(() => setRestoring(false));
  }

  if (editor) {
    const edited = editor.index === null ? undefined : list[editor.index];
    return (
      <BindingEditor
        initial={edited}
        others={list.filter((_, i) => i !== editor.index)}
        profiles={state.profiles}
        devices={state.devices}
        error={editorError}
        onSave={saveBinding}
        onRemove={() => editor.index !== null && removeBinding(editor.index)}
        onCancel={() => {
          setEditor(null);
          setEditorError(null);
        }}
      />
    );
  }

  return (
    <div className="ed">
      <div className="row ed-top">
        <IconButton
          label="Voltar"
          icon={<Icon name="arrow-left" size={18} />}
          size={34}
          onPress={onBack}
        />
        <b className="ed-title">Atalhos globais</b>
      </div>

      <div className="ed-sec" style={{ flex: 1, minHeight: 0 }}>
        <div className="lbl">Atalhos</div>
        <div style={{ flex: 1, minHeight: 0, overflowY: "auto", overflowX: "hidden" }}>
          {list.length === 0 ? (
            <p className="empty">
              Nenhum atalho configurado. Adicione um atalho para as ações do Sonar.
            </p>
          ) : (
            <div className="grp">
              {list.map((binding, index) => (
                <BindingRow
                  key={`${index}-${binding.accelerator}`}
                  binding={binding}
                  status={statusOf(binding)}
                  title={bindingTitle(binding.action, {
                    profiles: state.profiles,
                    devices: state.devices,
                  })}
                  detail={bindingDetail(binding.action, {
                    profiles: state.profiles,
                    devices: state.devices,
                  })}
                  icon={bindingIcon(binding.action)}
                  brass={bindingIsBrass(binding.action)}
                  onEdit={() => {
                    setEditorError(null);
                    setEditor({ index, initial: binding });
                  }}
                  onAccelerator={(accelerator) => changeAccelerator(index, accelerator)}
                />
              ))}
            </div>
          )}
        </div>
      </div>

      <button
        type="button"
        className="ed-add"
        onClick={() => {
          setEditorError(null);
          setEditor({ index: null });
        }}
      >
        <Icon name="plus" size={16} />
        Adicionar atalho
      </button>

      <div className="row" style={{ gap: 8 }}>
        <button
          type="button"
          className="btn"
          style={{ flex: 1 }}
          disabled={saving}
          onClick={suggest}
        >
          Sugerir atalhos de perfil
        </button>
        <button
          type="button"
          className="btn"
          style={{ flex: 1 }}
          disabled={restoring || saving}
          onClick={() => setConfirmRestore(true)}
        >
          Restaurar padrões
        </button>
      </div>

      {confirmRestore && (
        <div className="ed-confirm" role="group" aria-label="Confirmar restauração">
          <span>Restaurar os atalhos padrão? Os atalhos que você criou somem.</span>
          <div className="ed-confirm-actions">
            <button type="button" className="btn danger" onClick={restoreDefaults}>
              Restaurar
            </button>
            <button type="button" className="btn" onClick={() => setConfirmRestore(false)}>
              Manter
            </button>
          </div>
        </div>
      )}

      <div className="foot">
        <Icon name="check" size={15} />
        <span>As mudanças são salvas na hora.</span>
      </div>
    </div>
  );
}

/** A frase do backend quando ela vem como texto; a genérica quando vem outra coisa. */
function errorMessage(cause: unknown, fallback: string): string {
  return typeof cause === "string" && cause.length > 0 ? cause : fallback;
}

/** Estilo da caixinha de tecla de um binding indisponível. */
const BAD_KEY = { color: "var(--danger)", borderColor: "var(--danger-line)" };

interface BindingRowProps {
  binding: Binding;
  status: BindingStatus;
  title: string;
  detail: string;
  icon: Parameters<typeof Icon>[0]["name"];
  brass: boolean;
  onEdit: () => void;
  onAccelerator: (accelerator: string) => void;
}

/**
 * Uma linha da lista: alvo, detalhe (ou o motivo da falha) e o atalho. As caixinhas
 * capturam a combinação na hora; o lápis abre o editor.
 */
function BindingRow({
  binding,
  status,
  title,
  detail,
  icon,
  brass,
  onEdit,
  onAccelerator,
}: BindingRowProps) {
  const [capturing, setCapturing] = useState(false);
  const [hint, setHint] = useState<string | null>(null);
  const keysRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (capturing) keysRef.current?.focus();
  }, [capturing]);

  const unavailable = status.type === "unavailable";
  const keys = shortcutKeys(binding.accelerator);

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
    onAccelerator(outcome.shortcut);
  }

  return (
    <div className="it">
      <div className={brass ? "stile brass" : "stile"}>
        <Icon name={icon} size={20} />
      </div>
      <div className="tx">
        <b>{title}</b>
        <span className={unavailable ? "bad" : undefined}>
          {unavailable ? `Indisponível: ${status.message}` : (hint ?? detail)}
        </span>
      </div>
      <div className="act">
        <button
          ref={keysRef}
          type="button"
          className="keys-btn"
          disabled={unavailable}
          aria-label={
            unavailable
              ? `Atalho indisponível: ${keys.join(" ")}`
              : capturing
                ? "Digite a combinação do atalho"
                : `Atalho: ${keys.join(" ")}. Toque para trocar.`
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
          ) : (
            keys.map((key) => (
              <span key={key} className="k" style={unavailable ? BAD_KEY : undefined}>
                {key}
              </span>
            ))
          )}
        </button>
        <IconButton
          label="Editar atalho"
          icon={<Icon name="pencil" size={16} />}
          size={32}
          onPress={onEdit}
        />
      </div>
    </div>
  );
}
