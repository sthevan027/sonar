import { useEffect } from "react";
import * as api from "../api";
import { useToast } from "./Toast";

/**
 * Avisa o usuário quando uma ação de atalho global não deu certo (`action-result`).
 *
 * Vive no nível do App, dentro do `ToastProvider`: o aviso vale em qualquer tela, não só
 * na de atalhos. Quando o painel está oculto, é o Rust que o mostra ao emitir a falha.
 * Não renderiza nada.
 */
export function ActionResultToasts() {
  const notify = useToast();

  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    api
      .onActionResult((result) => {
        if (!alive || result.ok) return;
        notify(result.message ?? "Não foi possível executar a ação.");
      })
      .then(
        (stop) => {
          // A assinatura pode chegar depois do desmonte: cancela na hora.
          if (alive) unlisten = stop;
          else stop();
        },
        () => undefined,
      );
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [notify]);

  return null;
}
