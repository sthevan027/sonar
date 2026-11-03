import type { ReactNode } from "react";
import { Icon } from "./Icon";

interface ChipProps {
  kind: "device" | "mic";
  tone: "brass" | "sage";
  label: string;
  /** Com `onPress` o chip vira botão (com chevron, de menu). Sem ele é só um rótulo. */
  onPress?: () => void;
  /** Substitui o ícone padrão do `kind` (alto-falante para dispositivo, microfone para mic). */
  icon?: ReactNode;
}

export function Chip({ kind, tone, label, onPress, icon }: ChipProps) {
  const className = `chip ${tone === "brass" ? "hs" : "sp"}`;
  const body = (
    <>
      {icon ?? <Icon name={kind === "mic" ? "mic" : "speaker"} size={15} />}
      {label}
      {onPress && <Icon name="chevron-down" size={15} />}
    </>
  );

  if (!onPress) {
    return (
      <span className={className} data-kind={kind}>
        {body}
      </span>
    );
  }
  return (
    <button type="button" className={className} data-kind={kind} onClick={onPress}>
      {body}
    </button>
  );
}
