// Debounce de chamadas de escrita (ex.: volume durante o arraste do slider). Cada chamada
// reinicia o prazo; quando o prazo acaba, roda uma vez com os argumentos da última chamada.
// O hook dá `flush` ao desmontar: um valor pendente não se perde ao fechar a tela.

import { useEffect, useRef, useState } from "react";

export interface Debounced<A extends unknown[]> {
  (...args: A): void;
  /** Roda já a chamada pendente, se houver. */
  flush(): void;
  /** Descarta a chamada pendente. */
  cancel(): void;
  /** Há uma chamada esperando o prazo? */
  pending(): boolean;
}

export function createDebounced<A extends unknown[]>(
  fn: (...args: A) => void,
  waitMs: number,
): Debounced<A> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let lastArgs: A | null = null;

  function clearTimer() {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  }

  function flush() {
    clearTimer();
    if (lastArgs === null) return;
    const args = lastArgs;
    lastArgs = null;
    fn(...args);
  }

  const debounced = ((...args: A) => {
    lastArgs = args;
    clearTimer();
    timer = setTimeout(flush, waitMs);
  }) as Debounced<A>;

  debounced.flush = flush;
  debounced.cancel = () => {
    clearTimer();
    lastArgs = null;
  };
  debounced.pending = () => lastArgs !== null;
  return debounced;
}

/**
 * Versão de React: a função é a mais recente a cada render, o `Debounced` é o mesmo durante
 * a vida do componente e o valor pendente é rodado ao desmontar. `waitMs` vale só na montagem.
 */
export function useDebouncedCallback<A extends unknown[]>(
  fn: (...args: A) => void,
  waitMs: number,
): Debounced<A> {
  const fnRef = useRef(fn);
  useEffect(() => {
    fnRef.current = fn;
  });

  const [debounced] = useState(() =>
    createDebounced<A>((...args) => fnRef.current(...args), waitMs),
  );

  useEffect(() => () => debounced.flush(), [debounced]);
  return debounced;
}
