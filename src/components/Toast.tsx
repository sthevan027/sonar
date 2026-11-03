import { createContext, useCallback, useContext, useEffect, useRef, useState, type ReactNode } from "react";

/** Quanto tempo uma mensagem fica na tela. */
export const TOAST_MS = 4000;

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

/** Mostra avisos curtos num quadro com `role="status"`; some sozinho depois de `TOAST_MS`. */
export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<Item[]>([]);
  const lastShown = useRef(new Map<string, number>());
  const timers = useRef(new Set<ReturnType<typeof setTimeout>>());
  const nextId = useRef(0);

  const notify = useCallback<Notify>((message, options = {}) => {
    const now = Date.now();
    if (options.key !== undefined) {
      const last = lastShown.current.get(options.key);
      if (last !== undefined && now - last < (options.minIntervalMs ?? 0)) return;
      lastShown.current.set(options.key, now);
    }
    nextId.current += 1;
    const id = nextId.current;
    setItems((list) => [...list, { id, message }]);
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
