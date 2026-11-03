// Limita a frequência de uma escrita (ex.: volume durante o arraste do slider): envia já na
// primeira chamada (leading) e, dentro da janela de `waitMs`, guarda só a última chamada, que
// sai no fim da janela (trailing). Assim o usuário ouve a mudança ao vivo e o valor final
// sempre é enviado. `flush` manda o valor guardado já; o hook faz isso ao desmontar.

import { useEffect, useRef, useState } from "react";

export interface Throttled<A extends unknown[]> {
  (...args: A): void;
  /** Manda já o valor guardado, se houver, e fecha a janela. */
  flush(): void;
  /** Descarta o valor guardado e fecha a janela. */
  cancel(): void;
  /** Há um valor guardado esperando o fim da janela? */
  pending(): boolean;
}

export function createThrottled<A extends unknown[]>(
  fn: (...args: A) => void,
  waitMs: number,
): Throttled<A> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let queued: A | null = null;

  function startWindow() {
    timer = setTimeout(endWindow, waitMs);
  }

  function endWindow() {
    timer = null;
    if (queued === null) return;
    const args = queued;
    queued = null;
    fn(...args);
    startWindow();
  }

  function clearWindow() {
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  }

  const throttled = ((...args: A) => {
    if (timer === null) {
      fn(...args);
      startWindow();
      return;
    }
    queued = args;
  }) as Throttled<A>;

  throttled.flush = () => {
    clearWindow();
    if (queued === null) return;
    const args = queued;
    queued = null;
    fn(...args);
  };
  throttled.cancel = () => {
    clearWindow();
    queued = null;
  };
  throttled.pending = () => queued !== null;
  return throttled;
}

/**
 * Versão de React: a função é a mais recente a cada render, o `Throttled` é o mesmo durante a
 * vida do componente e o valor guardado é enviado ao desmontar. `waitMs` vale só na montagem.
 */
export function useThrottledCallback<A extends unknown[]>(
  fn: (...args: A) => void,
  waitMs: number,
): Throttled<A> {
  const fnRef = useRef(fn);
  useEffect(() => {
    fnRef.current = fn;
  });

  const [throttled] = useState(() =>
    createThrottled<A>((...args) => fnRef.current(...args), waitMs),
  );

  useEffect(() => () => throttled.flush(), [throttled]);
  return throttled;
}
