import { useEffect, useRef, useState } from "react";
import * as api from "../api";
import { Icon } from "../components/Icon";
import type { UpdateInfo, UpdateProgress } from "../types";
import {
  formatUpdateDate,
  progressPercent,
  progressText,
  UP_TO_DATE_TEXT,
  updateErrorText,
} from "./updateModel";

type Phase = "idle" | "checking" | "upToDate" | "available" | "installing";

/**
 * Seção "Atualizações" das configurações. A checagem só acontece quando o usuário pede (ou
 * quando a automática já achou algo, que aparece sem nova consulta). Instalar exige o botão.
 */
export function UpdateSection() {
  const [phase, setPhase] = useState<Phase>("idle");
  const [info, setInfo] = useState<UpdateInfo | null>(null);
  const [progress, setProgress] = useState<UpdateProgress | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Trava síncrona: dois cliques na mesma rodada não disparam duas instalações.
  const installing = useRef(false);

  useEffect(() => {
    let alive = true;
    const stops: Array<() => void> = [];
    const keep = (stop: () => void) => {
      if (alive) stops.push(stop);
      else stop();
    };

    api.pendingUpdate().then(
      (found) => {
        if (alive && found) {
          setInfo(found);
          setPhase("available");
        }
      },
      () => undefined,
    );
    api.onUpdateAvailable((found) => {
      if (!alive || installing.current) return;
      setInfo(found);
      setPhase("available");
    }).then(keep, () => undefined);
    api.onUpdateProgress((next) => {
      if (alive) setProgress(next);
    }).then(keep, () => undefined);

    return () => {
      alive = false;
      stops.forEach((stop) => stop());
    };
  }, []);

  async function check() {
    setError(null);
    setPhase("checking");
    try {
      const found = await api.checkForUpdate();
      if (found) {
        setInfo(found);
        setPhase("available");
      } else {
        setInfo(null);
        setPhase("upToDate");
      }
    } catch (err) {
      setError(updateErrorText(err));
      setPhase((current) => (current === "checking" ? "idle" : current));
    }
  }

  async function install() {
    if (installing.current) return;
    installing.current = true;
    setError(null);
    setProgress(null);
    setPhase("installing");
    try {
      // No Windows o app fecha aqui e reabre na versão nova. Se a promessa voltar, a tela
      // continua em "instalando" até o processo sair.
      await api.installUpdate();
    } catch (err) {
      installing.current = false;
      setPhase("available");
      setError(updateErrorText(err));
    }
  }

  const busy = phase === "checking" || phase === "installing";
  const date = formatUpdateDate(info?.date ?? null);
  const percent = progress ? progressPercent(progress) : null;

  return (
    <div className="ed-sec">
      <div className="lbl">Atualizações</div>
      <div className="grp">
        <div className="it">
          <div className="stile brass">
            <Icon name="rocket" size={20} />
          </div>
          <div className="tx">
            <b>Atualizar o Sonar</b>
            <span role="status" aria-live="polite">
              {statusLine(phase)}
            </span>
          </div>
          <button
            type="button"
            className="upd-btn"
            disabled={busy}
            onClick={() => void check()}
          >
            <Icon name="rotate-ccw" size={14} />
            Verificar atualizações
          </button>
        </div>

        {phase === "available" && info && (
          <div className="it upd-detail">
            <div className="tx">
              <b>Versão {info.version} disponível</b>
              {date && <span>Publicada em {date}</span>}
              {info.notes && <pre className="upd-notes">{info.notes}</pre>}
            </div>
            <button type="button" className="upd-primary" onClick={() => void install()}>
              Atualizar e reiniciar
            </button>
          </div>
        )}

        {phase === "installing" && (
          <div className="it upd-detail">
            <div className="tx">
              <b>Instalando {info ? `a versão ${info.version}` : "a atualização"}</b>
              <span>{progressText(progress)}</span>
              <div
                className="upd-bar"
                role="progressbar"
                aria-label="Progresso da atualização"
                aria-busy={percent === null}
                aria-valuemin={0}
                aria-valuemax={100}
                {...(percent === null ? {} : { "aria-valuenow": percent })}
              >
                <span style={{ width: percent === null ? "30%" : `${percent}%` }} />
              </div>
            </div>
          </div>
        )}

        {error && (
          <div className="upd-error" role="alert">
            {error}
          </div>
        )}
      </div>
    </div>
  );
}

function statusLine(phase: Phase): string {
  switch (phase) {
    case "checking":
      return "Verificando…";
    case "upToDate":
      return UP_TO_DATE_TEXT;
    case "available":
      return "Há uma versão nova para instalar.";
    case "installing":
      return "Instalando…";
    case "idle":
      return "O Sonar procura versões novas sozinho. Você escolhe quando instalar.";
  }
}
