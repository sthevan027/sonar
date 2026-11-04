import { Icon } from "./Icon";

interface MuteButtonProps {
  muted: boolean;
  /** Nome do que será silenciado, para o leitor de tela ("Spotify", "Saída padrão"). */
  name: string;
  onToggle: () => void;
}

/** Ícone de som ao lado da barra de volume. Clicar silencia ou devolve o som. */
export function MuteButton({ muted, name, onToggle }: MuteButtonProps) {
  return (
    <button
      type="button"
      className="mute-btn"
      aria-pressed={muted}
      aria-label={muted ? `Ativar o som de ${name}` : `Silenciar ${name}`}
      title={muted ? "Ativar o som" : "Silenciar"}
      onClick={onToggle}
    >
      <Icon name={muted ? "volume-x" : "volume-2"} size={18} />
    </button>
  );
}
