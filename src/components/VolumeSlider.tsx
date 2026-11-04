import { useEffect, useRef, useState, type ReactNode } from "react";
import { useThrottledCallback } from "../useThrottledCallback";
import { useToast } from "./Toast";
import { Slider } from "./Slider";

/** Intervalo mínimo entre comandos de volume durante o arraste (Ruling 23). */
export const VOLUME_THROTTLE_MS = 60;

/** Falhas de volume aparecem no máximo uma vez por este intervalo. */
export const VOLUME_TOAST_MS = 2000;

/** Depois do último movimento, o rascunho segura este tempo contra eco de estado antigo. */
export const VOLUME_SETTLE_MS = 400;

interface VolumeSliderProps {
  /** Volume do estado, de 0.0 a 1.0. */
  value: number;
  /** Recebe 0.0 a 1.0. Envia já no primeiro movimento e no fim de cada janela de 60 ms. */
  onCommit: (value: number) => Promise<void>;
  tone?: "text" | "brass";
  icon?: ReactNode;
  label: string;
}

/**
 * Barra de volume ligada ao estado. Durante o arraste mostra o valor do usuário (rascunho).
 * O rascunho só some quando (a) chega um valor do estado igual a ele e não há envio pendente,
 * ou (b) passam `VOLUME_SETTLE_MS` desde o último movimento. Assim um eco do estado com o
 * volume antigo não puxa a barra de volta durante o arraste ou logo depois dele. Se o comando
 * falha, o rascunho some e aparece um aviso (no máximo um a cada 2 s).
 */
export function VolumeSlider({ value, onCommit, tone, icon, label }: VolumeSliderProps) {
  const [draft, setDraft] = useState<number | null>(null);
  const notify = useToast();
  const settleTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const commit = useThrottledCallback((percent: number) => {
    Promise.resolve(onCommit(percent / 100)).catch(() => {
      clearSettle();
      setDraft(null);
      notify("Não foi possível mudar o volume.", { key: "volume", minIntervalMs: VOLUME_TOAST_MS });
    });
  }, VOLUME_THROTTLE_MS);

  function clearSettle() {
    if (settleTimer.current !== null) {
      clearTimeout(settleTimer.current);
      settleTimer.current = null;
    }
  }

  // Estado chegou igual ao rascunho: o rascunho já cumpriu o papel.
  useEffect(() => {
    if (draft !== null && Math.round(value * 100) === draft && !commit.pending()) {
      clearSettle();
      setDraft(null);
    }
  }, [value, draft, commit]);

  useEffect(() => clearSettle, []);

  function change(percent: number) {
    setDraft(percent);
    commit(percent);
    clearSettle();
    settleTimer.current = setTimeout(() => {
      settleTimer.current = null;
      if (!commit.pending()) setDraft(null);
    }, VOLUME_SETTLE_MS);
  }

  return (
    <Slider
      value={draft ?? Math.round(value * 100)}
      onChange={change}
      tone={tone}
      icon={icon}
      label={label}
    />
  );
}
