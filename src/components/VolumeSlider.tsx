import { useEffect, useState, type ReactNode } from "react";
import { useThrottledCallback } from "../useThrottledCallback";
import { useToast } from "./Toast";
import { Slider } from "./Slider";

/** Intervalo mínimo entre comandos de volume durante o arraste (Ruling 23). */
export const VOLUME_THROTTLE_MS = 60;

/** Falhas de volume aparecem no máximo uma vez por este intervalo. */
export const VOLUME_TOAST_MS = 2000;

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
 * Barra de volume ligada ao estado. Durante o arraste mostra o valor do usuário (rascunho)
 * e envia com limite de frequência; o rascunho some quando o estado novo chega e não há
 * envio pendente. Se o comando falha, o rascunho some, a barra volta ao estado e aparece um
 * aviso (no máximo um a cada 2 s).
 */
export function VolumeSlider({ value, onCommit, tone, icon, label }: VolumeSliderProps) {
  const [draft, setDraft] = useState<number | null>(null);
  const notify = useToast();
  const commit = useThrottledCallback((percent: number) => {
    Promise.resolve(onCommit(percent / 100)).catch(() => {
      setDraft(null);
      notify("Não foi possível mudar o volume.", { key: "volume", minIntervalMs: VOLUME_TOAST_MS });
    });
  }, VOLUME_THROTTLE_MS);

  useEffect(() => {
    if (!commit.pending()) setDraft(null);
  }, [value, commit]);

  function change(percent: number) {
    setDraft(percent);
    commit(percent);
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
