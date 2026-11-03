import { useEffect, useState, type ReactNode } from "react";
import { useDebouncedCallback } from "../useDebouncedCallback";
import { Slider } from "./Slider";

/** Intervalo mínimo entre comandos de volume durante o arraste (Ruling 21). */
export const VOLUME_DEBOUNCE_MS = 60;

interface VolumeSliderProps {
  /** Volume do estado, de 0.0 a 1.0. */
  value: number;
  /** Recebe 0.0 a 1.0, no máximo uma vez por janela de `VOLUME_DEBOUNCE_MS`. */
  onCommit: (value: number) => Promise<void>;
  tone?: "text" | "brass";
  icon?: ReactNode;
  label: string;
}

/**
 * Barra de volume ligada ao estado. Durante o arraste mostra o valor do usuário (rascunho)
 * e manda o comando com debounce; o rascunho some quando o estado novo chega e não há envio
 * pendente. Se o comando falha, o rascunho some e a barra volta ao valor do estado.
 */
export function VolumeSlider({ value, onCommit, tone, icon, label }: VolumeSliderProps) {
  const [draft, setDraft] = useState<number | null>(null);
  const commit = useDebouncedCallback((percent: number) => {
    Promise.resolve(onCommit(percent / 100)).catch(() => setDraft(null));
  }, VOLUME_DEBOUNCE_MS);

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

