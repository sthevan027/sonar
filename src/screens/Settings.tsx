import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import * as api from "../api";
import { Icon } from "../components/Icon";
import { IconButton } from "../components/IconButton";
import { useToast } from "../components/Toast";
import { Toggle } from "../components/Toggle";
import { useAppState } from "../store";
import type { Settings as AppSettings } from "../types";
import {
  APP_NAME,
  APP_VERSION,
  DEFAULT_SHORTCUT,
  shortcutFromKeys,
  shortcutKeys,
} from "./settingsModel";

interface SettingsProps {
  onBack: () => void;
  onOpenDevices: () => void;
  onOpenProfiles: () => void;
}

/**
 * Fila global de `save_settings`: a ordem de envio é a de chegada no Rust. O corpo de cada
 * gravação é montado na hora de rodar (com o que está pedido naquele momento), e a falha de
 * um campo roda antes da próxima gravação da fila.
 */
let settingsQueue: Promise<void> = Promise.resolve();

function enqueueSettings(call: () => Promise<void>, onFail: (error: unknown) => void) {
  settingsQueue = settingsQueue.then(() => call().catch(onFail));
}

/**
 * Configurações (mockup E). O estado vem do store (`state.settings`); `get_settings` só entra
 * como reserva se o estado não trouxer configurações. Mudança aparece na hora (sobreposição
 * local) e vai para `save_settings` em fila. Se um campo falha, só ele volta ao valor
 * confirmado. Atalho e "Iniciar com o Windows" são aplicados pelo Rust ao gravar; se recusarem,
 * a frase de motivo aparece no aviso.
 */
export function Settings({ onBack, onOpenDevices, onOpenProfiles }: SettingsProps) {
  const state = useAppState();
  const notify = useToast();
  const storeSettings = state.settings as AppSettings | undefined;
  const [fallback, setFallback] = useState<AppSettings | null>(null);
  const confirmed = storeSettings ?? fallback;
  // Campos pedidos e ainda não confirmados pelo store. `wanted` é o espelho síncrono da fila.
  const [overlay, setOverlay] = useState<Partial<AppSettings>>({});
  const wanted = useRef<Partial<AppSettings>>({});
  const base = useRef<AppSettings | null>(confirmed);
  const [capturing, setCapturing] = useState(false);
  const [hint, setHint] = useState<string | null>(null);
  const keysRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (storeSettings) return;
    let alive = true;
    api.getSettings().then(
      (loaded) => {
        if (alive) setFallback(loaded);
      },
      () => notify("Não foi possível carregar as configurações."),
    );
    return () => {
      alive = false;
    };
  }, [storeSettings, notify]);

  // Quando o store confirma um valor pedido, a sobreposição daquele campo sai.
  useEffect(() => {
    base.current = confirmed;
    if (!confirmed) return;
    const keep: Partial<AppSettings> = { ...wanted.current };
    for (const key of Object.keys(keep) as (keyof AppSettings)[]) {
      if (keep[key] === confirmed[key]) delete keep[key];
    }
    wanted.current = keep;
    setOverlay(keep);
  }, [confirmed]);

  useEffect(() => {
    if (capturing) keysRef.current?.focus();
  }, [capturing]);

  /** Mostra a mudança na hora e a manda para a fila. Falha: só este campo volta. */
  function change<K extends keyof AppSettings>(field: K, value: AppSettings[K]) {
    wanted.current = { ...wanted.current, [field]: value };
    setOverlay(wanted.current);
    enqueueSettings(
      () => {
        const current = base.current;
        if (!current) return Promise.resolve();
        return api.saveSettings({ ...current, ...wanted.current });
      },
      (error: unknown) => {
        const rest = { ...wanted.current };
        delete rest[field];
        wanted.current = rest;
        setOverlay(rest);
        // Atalho em uso ou "Iniciar com o Windows" recusado chegam como frase em português.
        notify(
          typeof error === "string" && error.length > 0
            ? error
            : "Não foi possível salvar a configuração.",
        );
      },
    );
  }

  const shown: AppSettings | null = confirmed ? { ...confirmed, ...overlay } : null;

  function onShortcutKey(event: KeyboardEvent<HTMLButtonElement>) {
    if (!capturing || !shown) return;
    // Tab sai da captura (o foco segue o caminho normal).
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
    change("shortcut", outcome.shortcut);
  }

  const keys = shown ? shortcutKeys(shown.shortcut) : [];
  const outputs = state.devices.filter((d) => d.flow === "render").length;
  const inputs = state.devices.filter((d) => d.flow === "capture").length;
  const helpText = hint ?? (capturing ? "Pressione a combinação. Esc cancela." : "Abre o painel na hora");

  return (
    <div className="tab-body">
      <div className="row ed-top">
        <IconButton
          label="Voltar"
          icon={<Icon name="arrow-left" size={18} />}
          size={34}
          onPress={onBack}
        />
        <b className="ed-title">Configurações</b>
      </div>

      <div className="ed-sec">
        <div className="lbl">Geral</div>
        <div className="grp">
          <div className="it">
            <div className="stile brass">
              <Icon name="power" size={20} />
            </div>
            <div className="tx">
              <b>Iniciar com o Windows</b>
              <span>Abre na bandeja ao ligar o PC</span>
            </div>
            {shown && (
              <Toggle
                label="Iniciar com o Windows"
                on={shown.autostart}
                onChange={(on) => change("autostart", on)}
              />
            )}
          </div>

          <div className="it">
            <div className="stile">
              <Icon name="keyboard" size={20} />
            </div>
            <div className="tx">
              <b>Atalho global</b>
              <span className={hint ? "bad" : undefined} aria-live="polite">
                {helpText}
              </span>
              {shown && shown.shortcut !== DEFAULT_SHORTCUT && (
                <button
                  type="button"
                  className="rst"
                  onClick={() => change("shortcut", DEFAULT_SHORTCUT)}
                >
                  <Icon name="rotate-ccw" size={14} />
                  Restaurar atalho padrão (Win Alt V)
                </button>
              )}
            </div>
            <button
              ref={keysRef}
              type="button"
              className="keys-btn"
              disabled={!shown}
              aria-label={
                capturing
                  ? "Digite a combinação do atalho"
                  : `Atalho global: ${keys.join(" ")}. Toque para trocar.`
              }
              onClick={() => {
                setHint(null);
                setCapturing(true);
              }}
              onKeyDown={onShortcutKey}
              onBlur={() => {
                setCapturing(false);
                setHint(null);
              }}
            >
              {capturing ? (
                <span className="keys-hint">Pressione a combinação</span>
              ) : (
                keys.map((key) => (
                  <span key={key} className="k">
                    {key}
                  </span>
                ))
              )}
            </button>
          </div>
        </div>
      </div>

      <div className="ed-sec">
        <div className="lbl">Seu áudio</div>
        <div className="grp">
          <button type="button" className="it" onClick={onOpenDevices}>
            <div className="stile">
              <Icon name="speaker" size={20} />
            </div>
            <div className="tx">
              <b>Dispositivos de áudio</b>
              <span>Saída e entrada: nome, ícone, visível</span>
            </div>
            <span className="cnt">{`${outputs} · ${inputs}`}</span>
            <Icon name="chevron-right" size={18} />
          </button>
          <button type="button" className="it" onClick={onOpenProfiles}>
            <div className="stile brass">
              <Icon name="layers" size={20} />
            </div>
            <div className="tx">
              <b>Perfis</b>
              <span>Abre a barra de perfis no painel</span>
            </div>
            <span className="cnt">{state.profiles.length}</span>
            <Icon name="chevron-right" size={18} />
          </button>
        </div>
      </div>

      <div className="ed-sec">
        <div className="lbl">Sobre</div>
        <div className="grp">
          <div className="it">
            <div className="stile">
              <Icon name="info" size={20} />
            </div>
            <div className="tx">
              <b>{APP_NAME}</b>
              <span>Versão {APP_VERSION}</span>
            </div>
          </div>
        </div>
      </div>

      <div className="foot">
        <Icon name="check" size={15} />
        <span>As mudanças são salvas na hora.</span>
      </div>
    </div>
  );
}
