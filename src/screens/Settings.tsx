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
 * Configurações (mockup E). Lê e grava por `get_settings` e `save_settings`. Iniciar com o
 * Windows e o atalho só guardam a escolha aqui: registrar de fato é trabalho da próxima tarefa.
 */
export function Settings({ onBack, onOpenDevices, onOpenProfiles }: SettingsProps) {
  const state = useAppState();
  const notify = useToast();
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [capturing, setCapturing] = useState(false);
  const [hint, setHint] = useState<string | null>(null);
  const keysRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    let alive = true;
    api.getSettings().then(
      (loaded) => {
        if (alive) setSettings(loaded);
      },
      () => notify("Não foi possível carregar as configurações."),
    );
    return () => {
      alive = false;
    };
  }, [notify]);

  useEffect(() => {
    if (capturing) keysRef.current?.focus();
  }, [capturing]);

  /** Mostra a mudança na hora; se o Rust recusar, volta ao valor anterior e avisa. */
  function save(next: AppSettings) {
    const previous = settings;
    setSettings(next);
    api.saveSettings(next).catch(() => {
      setSettings(previous);
      notify("Não foi possível salvar a configuração.");
    });
  }

  function onShortcutKey(event: KeyboardEvent<HTMLButtonElement>) {
    if (!capturing || !settings) return;
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
    save({ ...settings, shortcut: outcome.shortcut });
  }

  const keys = settings ? shortcutKeys(settings.shortcut) : [];
  const outputs = state.devices.filter((d) => d.flow === "render").length;
  const inputs = state.devices.filter((d) => d.flow === "capture").length;

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
            {settings && (
              <Toggle
                label="Iniciar com o Windows"
                on={settings.autostart}
                onChange={(on) => save({ ...settings, autostart: on })}
              />
            )}
          </div>

          <div className="it">
            <div className="stile">
              <Icon name="keyboard" size={20} />
            </div>
            <div className="tx">
              <b>Atalho global</b>
              <span className={hint ? "bad" : undefined}>
                {hint ?? "Abre o painel na hora"}
              </span>
              {settings && settings.shortcut !== DEFAULT_SHORTCUT && (
                <button
                  type="button"
                  className="rst"
                  onClick={() => save({ ...settings, shortcut: DEFAULT_SHORTCUT })}
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
              disabled={!settings}
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
              <span>Criar, editar e apagar</span>
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
