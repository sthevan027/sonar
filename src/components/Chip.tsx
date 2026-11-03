import type { ReactNode, Ref } from "react";
import { Icon } from "./Icon";

interface ChipProps {
  kind: "device" | "mic";
  tone: "brass" | "sage";
  label: string;
  /** Com `onPress` o chip vira botão (com chevron, de menu). Sem ele é só um rótulo. */
  onPress?: () => void;
  /** Substitui o ícone padrão do `kind` (alto-falante para dispositivo, microfone para mic). */
  icon?: ReactNode;
  /** Estado do menu que o chip abre (`aria-expanded`). Só vale com `onPress`. */
  expanded?: boolean;
  /** Referência ao botão, para devolver o foco ao fechar o menu. */
  ref?: Ref<HTMLButtonElement>;
}

export function Chip({ kind, tone, label, onPress, icon, expanded, ref }: ChipProps) {
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
    <button
      ref={ref}
      type="button"
      className={className}
      data-kind={kind}
      aria-haspopup="menu"
      aria-expanded={expanded}
      onClick={onPress}
    >
      {body}
    </button>
  );
}
