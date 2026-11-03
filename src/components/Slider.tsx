import type { ReactNode } from "react";

/** Limita a 0..100. NaN vira 0 (não deixa a barra sair do trilho). */
export function clampPercent(value: number): number {
  if (Number.isNaN(value)) return 0;
  return Math.min(100, Math.max(0, value));
}

interface SliderProps {
  value: number;
  onChange: (value: number) => void;
  /** `brass` = trilho dourado (volume de dispositivo); `text` = claro (volume de app). */
  tone?: "text" | "brass";
  icon?: ReactNode;
  /** Nome acessível do controle (obrigatório). */
  label: string;
}

/**
 * Barra de volume. Usa um `<input type="range">` invisível por cima do trilho: teclado e
 * ponteiro vêm de graça, e a área de toque é mais alta que o trilho de 6px.
 * O valor mostrado é arredondado depois do clamp.
 */
export function Slider({ value, onChange, tone = "text", icon, label }: SliderProps) {
  const v = Math.round(clampPercent(value));
  return (
    <div className="row slider-row">
      {icon}
      <div className={tone === "brass" ? "track br" : "track"}>
        <i style={{ width: `${v}%` }} />
        <u style={{ left: `${v}%` }} />
        <input
          className="track-input"
          type="range"
          min={0}
          max={100}
          step={1}
          value={v}
          aria-label={label}
          onChange={(e) => onChange(clampPercent(Number(e.target.value)))}
        />
      </div>
      <span className="pct mono">{v}</span>
    </div>
  );
}
