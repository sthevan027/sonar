// Funções puras da seção "Atualizações": frases, datas e progresso. Sem JSX.

import type { UpdateProgress } from "../types";

/** Frase quando o Rust não mandou texto nenhum (não deveria acontecer, mas a tela segue). */
export const UPDATE_FALLBACK_ERROR = "Não foi possível verificar atualizações agora. Tente de novo.";

export const UP_TO_DATE_TEXT = "Você está na versão mais recente.";

/** Frase de erro para mostrar: a do Rust quando veio texto, senão a genérica. */
export function updateErrorText(error: unknown): string {
  return typeof error === "string" && error.trim().length > 0 ? error : UPDATE_FALLBACK_ERROR;
}

/** Data de publicação em pt-BR ("3 de novembro de 2026"), ou `null` quando não há. */
export function formatUpdateDate(unixSeconds: number | null): string | null {
  if (unixSeconds === null || !Number.isFinite(unixSeconds)) return null;
  return new Intl.DateTimeFormat("pt-BR", {
    day: "numeric",
    month: "long",
    year: "numeric",
  }).format(new Date(unixSeconds * 1000));
}

/** Porcentagem inteira do download, ou `null` quando o tamanho total não é conhecido. */
export function progressPercent(progress: UpdateProgress): number | null {
  if (progress.total === null || progress.total <= 0) return null;
  return Math.min(100, Math.round((progress.downloaded / progress.total) * 100));
}

function formatMegabytes(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  return `${new Intl.NumberFormat("pt-BR", { maximumFractionDigits: 1 }).format(mb)} MB`;
}

/** Texto do progresso: "Baixando… 12,3 MB de 40 MB". Sem progresso ainda, "Preparando…". */
export function progressText(progress: UpdateProgress | null): string {
  if (!progress) return "Preparando o download…";
  const done = formatMegabytes(progress.downloaded);
  if (progress.total === null) return `Baixando… ${done}`;
  // Download completo: o instalador assume daqui em diante.
  if (progress.downloaded >= progress.total) return "Instalando… o Sonar vai reiniciar.";
  return `Baixando… ${done} de ${formatMegabytes(progress.total)}`;
}
