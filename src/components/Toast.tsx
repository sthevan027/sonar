import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";

/** Quanto tempo uma mensagem fica na tela. Também é a janela de deduplicação. */
export const TOAST_MS = 4000;

/** No máximo tantas mensagens visíveis; a mais antiga sai primeiro. */
export const TOAST_MAX_VISIBLE = 3;

export interface NotifyOptions {
  /** Mensagens com a mesma chave respeitam `minIntervalMs` entre si. */
  key?: string;
  minIntervalMs?: number;
}

export type Notify = (message: string, options?: NotifyOptions) => void;

// Sem provedor (ex.: teste isolado), avisar não faz nada: nunca quebra a tela.
const ToastContext = createContext<Notify>(() => undefined);

export function useToast(): Notify {
  return useContext(ToastContext);
}

interface Item {
  id: number;
  message: string;
}

/**
 * Mostra avisos curtos num quadro com `role="status"`. Cada aviso some sozinho depois de
 * `TOAST_MS`. Não repete uma mensagem idêntica dentro dessa janela, e no máximo
 * `TOAST_MAX_VISIBLE` aparecem ao mesmo tempo (o mais antigo sai para dar lugar).
 */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<Item[]>([]);
  const lastKeyShown = useRef(new Map<string, number>());
  const lastMessageShown = useRef(new Map<string, number>());
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>());
  const nextId = useRef(0);

  const notify = useCallback<Notify>((message, options = {}) => {
    const now = Date.now();
    const lastMessage = lastMessageShown.current.get(message);
    if (lastMessage !== undefined && now - lastMessage < TOAST_MS) return;

    if (options.key !== undefined) {
      const last = lastKeyShown.current.get(options.key);
      if (last !== undefined && now - last < (options.minIntervalMs ?? 0)) return;
      lastKeyShown.current.set(options.key, now);
    }
    lastMessageShown.current.set(message, now);

    nextId.current += 1;
    const id = nextId.current;
    setItems((list) => [...list, { id, message }].slice(-TOAST_MAX_VISIBLE));
    const timer = setTimeout(() => {
      timers.current.delete(timer);
      setItems((list) => list.filter((item) => item.id !== id));
    }, TOAST_MS);
    timers.current.add(timer);
  }, []);

  useEffect(() => {
    const pending = timers.current;
    return () => {
      pending.forEach((timer) => clearTimeout(timer));
      pending.clear();
    };
  }, []);

  return (
    <ToastContext.Provider value={notify}>
      {children}
      <div className="toasts" role="status" aria-live="polite">
        {items.map((item) => (
          <div key={item.id} className="toast">
            {item.message}
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}
