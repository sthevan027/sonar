import type { PointerEvent, ReactNode } from "react";

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

  // Clicar, segurar e arrastar: o ponteiro fica "capturado" pelo trilho até soltar.
  function percentAt(event: PointerEvent<HTMLDivElement>): number {
    const rect = event.currentTarget.getBoundingClientRect();
    if (rect.width <= 0) return v;
    return Math.round(clampPercent(((event.clientX - rect.left) / rect.width) * 100));
  }
  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    event.currentTarget.setPointerCapture?.(event.pointerId);
    onChange(percentAt(event));
  }
  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) onChange(percentAt(event));
  }

  return (
    <div className="row slider-row">
      {icon}
      <div
        className={tone === "brass" ? "track br" : "track"}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
      >
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
