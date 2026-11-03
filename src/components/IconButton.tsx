import type { ReactNode } from "react";

interface IconButtonProps {
  /** Texto acessível: o botão não tem texto visível. */
  label: string;
  icon: ReactNode;
  onPress: () => void;
  /** Lado em px: 32 (padrão, `.ab`), 34 (voltar), 38 ou 44 (perfil). */
  size?: 32 | 34 | 38 | 44;
  selected?: boolean;
  disabled?: boolean;
}

export function IconButton({ label, icon, onPress, size = 32, selected, disabled }: IconButtonProps) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      disabled={disabled}
      className={selected ? "ibtn sel" : "ibtn"}
      style={{ width: size, height: size, borderRadius: size >= 38 ? 11 : size === 34 ? 10 : 9 }}
      onClick={onPress}
    >
      {icon}
    </button>
  );
}
