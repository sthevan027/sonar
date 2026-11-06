//! Dispatcher: o caminho único entre toda entrada (atalho, bandeja, IPC) e a execução.
//!
//! Toda ação entra numa fila limitada e nunca bloqueia quem produziu. Um worker único
//! consome a fila na ordem de chegada, executa pelo `ActionExecutor` e publica o
//! resultado. As rajadas de volume viram uma execução só: dez `StepOutputVolume` que
//! chegam juntos viram um `StepOutputVolume` com a soma dos deltas.
//!
//! O worker **nunca** segura a trava `apply` do `Engine`: ela é um `Mutex` do std, não é
//! reentrante, e cada `act_*` do `Engine` já a segura por conta própria. O dispatcher só
//! conhece o `Engine` como `Arc<dyn ActionExecutor>`.

use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::actions::{Action, ActionExecutor, ActionResult, ErrorCode};

/// Capacidade da fila de ações pendentes. Atingida, quem produz recebe `SubmitError::Full`.
pub const QUEUE_CAPACITY: usize = 64;

/// Janela de coalescência das rajadas de volume.
pub const COALESCE_WINDOW: Duration = Duration::from_millis(75);

/// Motivo da recusa de uma ação enfileirada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubmitError {
    /// A fila está cheia: a ação não entrou e nada mudou.
    Full,
    /// O worker encerrou: ninguém mais vai consumir a fila.
    ///
    /// Na prática só acontece se o `Receiver` cair — e ele cai junto com o worker.
    /// Como o pânico do executor é contido (`catch_unwind` em `run`), o worker
    /// não morre por pânico: hoje esse caminho é praticamente inalcançável. Ele
    /// fica no enum porque é o contrato honesto de um `try_send` sobre um canal
    /// desconectado, e porque nada impede o worker de encerrar no futuro
    /// (quando ele ganhar um caminho de saída próprio).
    Closed,
}

impl std::fmt::Display for SubmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubmitError::Full => write!(f, "a fila de ações está cheia"),
            SubmitError::Closed => write!(f, "o worker de ações encerrou"),
        }
    }
}

impl std::error::Error for SubmitError {}

/// Ponto de entrada de toda ação vinda de atalho, bandeja ou IPC.
///
/// É `Send + Sync` de propósito: o app guarda um no `State` e o compartilha entre a
/// bandeja, o atalho global e os comandos.
#[derive(Clone)]
pub struct Dispatcher {
    tx: SyncSender<Action>,
}

impl Dispatcher {
    /// Sobe o worker e devolve o ponto de entrada.
    ///
    /// `exec` é quem sabe falar com o Windows; `report` recebe o par ação/resultado depois
    /// de cada execução; `window` é a janela de coalescência das rajadas de volume.
    pub fn spawn(
        exec: Arc<dyn ActionExecutor>,
        report: Arc<dyn Fn(&Action, &ActionResult) + Send + Sync>,
        window: Duration,
    ) -> Dispatcher {
        let (tx, rx) = mpsc::sync_channel::<Action>(QUEUE_CAPACITY);
        // A thread é destacada: ela não segura o processo. O `Receiver` cai com ela,
        // e aí quem ainda tentar enfileirar recebe `SubmitError::Closed`.
        std::thread::spawn(move || worker(rx, exec, report, window));
        Dispatcher { tx }
    }

    /// Sobe o worker com a janela de coalescência do plano (`COALESCE_WINDOW`).
    ///
    /// É o construtor que o app deve usar: a janela é um parâmetro só para os
    /// testes, que precisam de janelas pequenas para não demorarem.
    pub fn spawn_default(
        exec: Arc<dyn ActionExecutor>,
        report: Arc<dyn Fn(&Action, &ActionResult) + Send + Sync>,
    ) -> Dispatcher {
        Dispatcher::spawn(exec, report, COALESCE_WINDOW)
    }

    /// Enfileira uma ação. Nunca bloqueia: fila cheia vira `SubmitError::Full`.
    pub fn submit(&self, action: Action) -> Result<(), SubmitError> {
        match self.tx.try_send(action) {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => Err(SubmitError::Full),
            Err(TrySendError::Disconnected(_)) => Err(SubmitError::Closed),
        }
    }
}

/// Executa uma ação e publica o resultado.
fn run(
    exec: &Arc<dyn ActionExecutor>,
    report: &Arc<dyn Fn(&Action, &ActionResult) + Send + Sync>,
    action: Action,
) {
    // O `AssertUnwindSafe` fica só em volta da chamada ao executor: `dyn ActionExecutor`
    // não promete `RefUnwindSafe`, e sem isso o `catch_unwind` nem compila. O resto do
    // worker continua com a proteção normal de pânico do Rust.
    let resultado = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        exec.execute(action.clone())
    })) {
        Ok(resultado) => resultado,
        // O pânico é um bug do executor, não uma recusa do Windows: `OsError`
        // diria que foi o sistema a recusar a operação, e não foi.
        Err(_) => ActionResult::failed(
            ErrorCode::Internal,
            "A ação falhou de forma inesperada.",
        ),
    };
    // O `report` também fica preso ao pânico: um callback defeituoso não pode matar o
    // worker e deixar as ações seguintes sem saída.
    let avisou = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        report(&action, &resultado);
    }));
    if avisou.is_err() {
        eprintln!("sonar: o aviso do resultado da ação falhou com pânico; seguindo");
    }
}

/// Loop do worker: consome a fila na ordem de chegada.
fn worker(
    rx: Receiver<Action>,
    exec: Arc<dyn ActionExecutor>,
    report: Arc<dyn Fn(&Action, &ActionResult) + Send + Sync>,
    window: Duration,
) {
    // Ação que quebrou um lote de volume: ela espera a vez dela, depois do lote.
    let mut adiada: Option<Action> = None;
    loop {
        let primeira = match adiada.take() {
            Some(acao) => acao,
            None => match rx.recv() {
                Ok(acao) => acao,
                // A fila fechou: o worker sai.
                Err(_) => break,
            },
        };

        match primeira {
            Action::StepOutputVolume { delta } => {
                // Lote de volume: junta as rajadas que chegarem dentro da janela.
                // Só `StepOutputVolume` soma: `SetOutputVolume` define um valor
                // absoluto e `ToggleOutputMute` é efeito de borda, não accumulate.
                let mut total = delta;
                let mut quebrou: Option<Action> = None;
                // O prazo do lote é FIXO: nasce com a primeira ação e não se renova
                // a cada ação que chega. Se renovasse, uma rajada com intervalo
                // menor que a janela (o auto-repeat do Windows, a 30–60 ms) nunca
                // fecharia o lote: `run()` não seria chamado, o volume não seria
                // aplicado durante a rajada e a fila encheria de volumes pendentes
                // (o atalho passaria a receber `SubmitError::Full`).
                let prazo = Instant::now() + window;
                loop {
                    // `saturating_duration_since` devolve `Duration::ZERO` quando o
                    // prazo já passou: `recv_timeout(Duration::ZERO)` é um try
                    // receive, que não bloqueia — o lote fecha na hora.
                    let restante = prazo.saturating_duration_since(Instant::now());
                    match rx.recv_timeout(restante) {
                        Ok(Action::StepOutputVolume { delta }) => total += delta,
                        // Ação de outro tipo: encerra o lote e roda depois dele,
                        // na ordem de chegada.
                        Ok(outra) => {
                            quebrou = Some(outra);
                            break;
                        }
                        Err(mpsc::RecvTimeoutError::Timeout) => break,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                run(&exec, &report, Action::StepOutputVolume { delta: total });
                adiada = quebrou;
            }
            outra => run(&exec, &report, outra),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::time::Instant;

    use super::*;

    /// Executor de mentira: registra as chamadas e sabe falhar ou entrar em pânico.
    struct FakeExecutor {
        chamadas: Mutex<Vec<Action>>,
        eventos: mpsc::Sender<Action>,
        panicar_em: Mutex<Option<usize>>,
        falhar_em: Mutex<Option<usize>>,
        porta: Mutex<Option<Porta>>,
        contador: AtomicUsize,
    }

    /// Barreira usada pelo teste de fila cheia: o executor para dentro dela.
    struct Porta {
        entrou: mpsc::Sender<()>,
        liberar: mpsc::Receiver<()>,
    }

    impl FakeExecutor {
        fn new(eventos: mpsc::Sender<Action>) -> Arc<FakeExecutor> {
            Arc::new(FakeExecutor {
                chamadas: Mutex::new(Vec::new()),
                eventos,
                panicar_em: Mutex::new(None),
                falhar_em: Mutex::new(None),
                porta: Mutex::new(None),
                contador: AtomicUsize::new(0),
            })
        }

        fn panicar_na(&self, indice: usize) {
            *self.panicar_em.lock().unwrap() = Some(indice);
        }

        fn falhar_na(&self, indice: usize) {
            *self.falhar_em.lock().unwrap() = Some(indice);
        }

        /// Trava a primeira execução até o teste mandar soltar.
        fn travar_primeira(&self, entrou: mpsc::Sender<()>, liberar: mpsc::Receiver<()>) {
            *self.porta.lock().unwrap() = Some(Porta { entrou, liberar });
        }

        fn chamadas(&self) -> Vec<Action> {
            self.chamadas.lock().unwrap().clone()
        }
    }

    impl ActionExecutor for FakeExecutor {
        fn execute(&self, action: Action) -> ActionResult {
            let indice = self.contador.fetch_add(1, Ordering::SeqCst);
            self.chamadas.lock().unwrap().push(action.clone());
            let _ = self.eventos.send(action.clone());

            let deve_panicar = *self.panicar_em.lock().unwrap() == Some(indice);
            if deve_panicar {
                // `resume_unwind` em vez de `panic!`: o hook de pânico não é
                // chamado, então o `cargo test` não imprime a mensagem do pânico
                // fora da captura do harness. (Um `panic::set_hook` global resolveria
                // o ruído, mas silenciaria o pânico dos outros testes, que rodam em
                // paralelo.)
                std::panic::resume_unwind(Box::new("pânico de propósito no executor"));
            }
            let deve_falhar = *self.falhar_em.lock().unwrap() == Some(indice);
            if deve_falhar {
                return ActionResult::failed(
                    ErrorCode::DeviceGone,
                    "O dispositivo não está mais disponível.",
                );
            }

            // A barreira vale só para a primeira execução.
            let porta = self.porta.lock().unwrap().take();
            if let Some(porta) = porta {
                let _ = porta.entrou.send(());
                let _ = porta.liberar.recv();
            }

            ActionResult::done(true, None)
        }
    }

    /// Recolhe os resultados publicados pelo `report`.
    ///
    /// Devolve o canal dos avisos (a barreira para o teste esperar) e a lista acumulada
    /// (para conferir o conteúdo).
    fn registrador() -> (
        Arc<dyn Fn(&Action, &ActionResult) + Send + Sync>,
        Receiver<(Action, ActionResult)>,
        Arc<Mutex<Vec<(Action, ActionResult)>>>,
    ) {
        let registros: Arc<Mutex<Vec<(Action, ActionResult)>>> = Arc::new(Mutex::new(Vec::new()));
        let destino = registros.clone();
        let (tx, rx) = mpsc::channel::<(Action, ActionResult)>();
        let report: Arc<dyn Fn(&Action, &ActionResult) + Send + Sync> =
            Arc::new(move |acao, resultado| {
                destino.lock().unwrap().push((acao.clone(), resultado.clone()));
                let _ = tx.send((acao.clone(), resultado.clone()));
            });
        (report, rx, registros)
    }

    /// Espera as `quantidade` execuções do worker. Falha com a contagem real.
    fn espera(rx: &Receiver<Action>, quantidade: usize) -> Vec<Action> {
        let mut vistas: Vec<Action> = Vec::new();
        for _ in 0..quantidade {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(acao) => vistas.push(acao),
                Err(err) => panic!(
                    "faltou execução: esperava {quantidade}, recebi {} ({err})",
                    vistas.len()
                ),
            }
        }
        vistas
    }

    /// Espera até `quantidade` execuções e devolve o que chegou.
    ///
    /// Diferente de [`espera`], não falha: devolve o que chegou até o prazo, e quem
    /// chama decide como assertir. Serve para os testes que precisam dizer "vieram
    /// menos do que eu esperava" com uma mensagem própria.
    fn espera_ate(rx: &Receiver<Action>, quantidade: usize, prazo: Duration) -> Vec<Action> {
        let mut vistas: Vec<Action> = Vec::new();
        while vistas.len() < quantidade {
            match rx.recv_timeout(prazo) {
                Ok(acao) => vistas.push(acao),
                Err(_) => break,
            }
        }
        vistas
    }

    /// Espera os `quantidade` avisos publicados pelo `report`.
    fn espera_avisos(rx: &Receiver<(Action, ActionResult)>, quantidade: usize) {
        for _ in 0..quantidade {
            rx.recv_timeout(Duration::from_secs(5))
                .expect("faltou aviso de resultado");
        }
    }

    #[test]
    fn acoes_rodam_na_ordem_de_chegada() {
        let (tx_eventos, rx_eventos) = mpsc::channel();
        let exec = FakeExecutor::new(tx_eventos);
        let (report, _avisos, _registros) = registrador();
        let dispatcher = Dispatcher::spawn(exec.clone(), report, COALESCE_WINDOW);

        let esperadas = vec![
            Action::TogglePanel,
            Action::ActivateProfile {
                profile_id: "jogos".to_string(),
            },
            Action::SetOutputMute { muted: true },
            Action::ToggleOutputMute,
            Action::SetAppMute {
                exe: "spotify.exe".to_string(),
                muted: false,
            },
        ];
        for acao in &esperadas {
            dispatcher.submit(acao.clone()).expect("fila com espaço");
        }

        let vistas = espera(&rx_eventos, esperadas.len());
        assert_eq!(vistas, esperadas);
    }

    #[test]
    fn rajada_de_volume_vira_uma_chamada_so() {
        let (tx_eventos, rx_eventos) = mpsc::channel();
        let exec = FakeExecutor::new(tx_eventos);
        let (report, avisos, registros) = registrador();
        let dispatcher = Dispatcher::spawn(exec.clone(), report, COALESCE_WINDOW);

        for _ in 0..10 {
            dispatcher
                .submit(Action::StepOutputVolume { delta: 0.05 })
                .expect("fila com espaço");
        }

        let vistas = espera(&rx_eventos, 1);
        match vistas[0] {
            Action::StepOutputVolume { delta } => {
                assert!(
                    (delta - 0.5).abs() < 1e-4,
                    "delta somado errado: {delta} (esperava ~0.5)"
                );
            }
            ref outra => panic!("chamada inesperada: {outra:?}"),
        }
        // O aviso sai depois da execução: a barreira espera ele chegar.
        espera_avisos(&avisos, 1);
        {
            let publicados = registros.lock().unwrap().clone();
            assert_eq!(publicados.len(), 1, "o report roda uma vez por execução");
            // O aviso carrega a ação coalescida, não uma das dez originais.
            match &publicados[0].0 {
                Action::StepOutputVolume { delta } => assert!(
                    (delta - 0.5).abs() < 1e-4,
                    "o aviso carregou o delta errado: {delta}"
                ),
                outra => panic!("aviso de ação inesperada: {outra:?}"),
            }
        }
        // Uma rajada vira uma execução só: a sentinela que vem logo depois tem de ser
        // a segunda chamada do executor. Se a rajada tivesse rendido mais de uma, a
        // sentinela seria a terceira.
        dispatcher
            .submit(Action::TogglePanel)
            .expect("fila com espaço");
        let depois = espera(&rx_eventos, 1);
        assert_eq!(
            depois[0],
            Action::TogglePanel,
            "a rajada virou mais de uma chamada: {:?}",
            exec.chamadas()
        );
    }

    #[test]
    fn outra_acao_quebra_o_lote_e_mantem_a_ordem() {
        let (tx_eventos, rx_eventos) = mpsc::channel();
        let exec = FakeExecutor::new(tx_eventos);
        let (report, _avisos, _registros) = registrador();
        let dispatcher = Dispatcher::spawn(exec.clone(), report, COALESCE_WINDOW);

        let esperadas = vec![
            Action::StepOutputVolume { delta: 0.05 },
            Action::ToggleOutputMute,
            Action::StepOutputVolume { delta: 0.05 },
        ];
        for acao in &esperadas {
            dispatcher.submit(acao.clone()).expect("fila com espaço");
        }

        let vistas = espera(&rx_eventos, esperadas.len());
        assert_eq!(vistas, esperadas);
    }

    #[test]
    fn o_prazo_do_lote_conta_a_partir_da_primeira_acao() {
        // Janela de 300 ms. A primeira ação abre o lote; a segunda chega 200 ms
        // depois (ainda dentro da janela) e a terceira 380 ms depois da primeira —
        // ou seja, depois do prazo fixo do lote, mas antes do prazo que uma janela
        // renovadora daria (a segunda ação empurraria o fim do lote para 500 ms).
        //
        // Com a janela renovadora de hoje, as três viram UM lote só (uma chamada).
        // Com o prazo fixo, são dois lotes: o primeiro com os dois primeiros passos
        // e o segundo com o terceiro.
        let window = Duration::from_millis(300);
        let (tx_eventos, rx_eventos) = mpsc::channel();
        let exec = FakeExecutor::new(tx_eventos);
        let (report, _avisos, _registros) = registrador();
        let dispatcher = Dispatcher::spawn(exec.clone(), report, window);

        // Aquecer o worker: ele precisa estar no `recv()` antes do primeiro passo.
        // Se o worker só acordasse depois, o prazo do lote nasceria atrasado e o
        // teste mediria outra coisa.
        dispatcher
            .submit(Action::TogglePanel)
            .expect("fila com espaço");
        espera(&rx_eventos, 1);

        dispatcher
            .submit(Action::StepOutputVolume { delta: 0.01 })
            .expect("fila com espaço");
        std::thread::sleep(Duration::from_millis(200));
        dispatcher
            .submit(Action::StepOutputVolume { delta: 0.02 })
            .expect("fila com espaço");
        std::thread::sleep(Duration::from_millis(180));
        dispatcher
            .submit(Action::StepOutputVolume { delta: 0.04 })
            .expect("fila com espaço");

        let vistas = espera_ate(&rx_eventos, 2, Duration::from_secs(3));
        assert_eq!(
            vistas.len(),
            2,
            "esperava 2 lotes (2 chamadas ao executor), veio {}: {:?}",
            vistas.len(),
            exec.chamadas()
        );
        // O primeiro lote leva o primeiro e o segundo passo (ambos dentro da janela).
        match vistas[0] {
            Action::StepOutputVolume { delta } => assert!(
                (delta - 0.03).abs() < 1e-4,
                "o primeiro lote somou errado: {delta} (esperava ~0.03)"
            ),
            ref outra => panic!("chamada inesperada: {outra:?}"),
        }
        // O terceiro passo chegou depois do prazo fixo: ele abre um lote novo.
        match vistas[1] {
            Action::StepOutputVolume { delta } => assert!(
                (delta - 0.04).abs() < 1e-4,
                "o segundo lote somou errado: {delta} (esperava ~0.04)"
            ),
            ref outra => panic!("chamada inesperada: {outra:?}"),
        }
    }

    #[test]
    fn rajada_continua_drena_em_varios_lotes() {
        // A rajada real: segurar a tecla de volume. O auto-repeat do Windows chega
        // a cada 30–60 ms — sempre dentro da janela de 75 ms. Com a janela
        // renovadora, o lote nunca fechava: o volume não era aplicado durante a
        // rajada inteira e a fila enchia de volumes pendentes.
        const INTERVALO: Duration = Duration::from_millis(20);
        const PASSOS: usize = 15;
        const DELTA: f32 = 0.05;
        let (tx_eventos, rx_eventos) = mpsc::channel();
        let exec = FakeExecutor::new(tx_eventos);
        let (report, _avisos, _registros) = registrador();
        let dispatcher = Dispatcher::spawn_default(exec.clone(), report);

        // Aquecer o worker: com a rajada já enfileirada antes de o worker acordar,
        // tudo cairia num lote só e o teste não diria nada sobre a rajada.
        dispatcher
            .submit(Action::TogglePanel)
            .expect("fila com espaço");
        espera(&rx_eventos, 1);

        let produtor = dispatcher.clone();
        std::thread::spawn(move || {
            for _ in 0..PASSOS {
                produtor
                    .submit(Action::StepOutputVolume { delta: DELTA })
                    .expect("fila com espaço");
                std::thread::sleep(INTERVALO);
            }
        });

        // O que se fixa não é o número exato de lotes (ele depende do alinhamento
        // do tempo), e sim: a rajada drena em MAIS DE UM lote e a soma dos lotes
        // bate com a soma de tudo que foi enviado.
        let total = PASSOS as f32 * DELTA;
        let mut lotes = 0usize;
        let mut soma = 0.0f32;
        let limite = Instant::now() + Duration::from_secs(10);
        while (soma - total).abs() > 1e-4 && Instant::now() < limite {
            match rx_eventos.recv_timeout(Duration::from_millis(500)) {
                Ok(Action::StepOutputVolume { delta }) => {
                    lotes += 1;
                    soma += delta;
                }
                Ok(outra) => panic!("chamada inesperada: {outra:?}"),
                Err(_) => {}
            }
        }
        assert!(
            (soma - total).abs() < 1e-4,
            "a soma dos lotes deu {soma}, esperava {total}"
        );
        assert!(
            lotes > 1,
            "a rajada virou {lotes} lote(s) só: o lote não fecha durante a rajada"
        );
    }

    #[test]
    fn fila_cheia_nao_bloqueia_o_produtor() {
        let (tx_eventos, _rx_eventos) = mpsc::channel();
        let exec = FakeExecutor::new(tx_eventos);
        let (report, _avisos, _registros) = registrador();
        let (tx_entrou, rx_entrou) = mpsc::channel();
        let (tx_liberar, rx_liberar) = mpsc::channel();
        exec.travar_primeira(tx_entrou, rx_liberar);
        let dispatcher = Dispatcher::spawn(exec.clone(), report, COALESCE_WINDOW);

        // A primeira ação ocupa o worker e para dentro do executor. A barreira é o
        // executor que sinaliza: nada de espera por tempo fixo.
        dispatcher
            .submit(Action::ToggleOutputMute)
            .expect("fila com espaço");
        rx_entrou
            .recv_timeout(Duration::from_secs(5))
            .expect("o worker não chegou ao executor");

        // Agora a fila está vazia e o worker está ocupado: quem enfileira tem de
        // receber o erro de volta, não ficar esperando.
        //
        // O produtor roda numa thread-fantasma: se `submit` bloqueasse (se usasse
        // `send` em vez de `try_send`), a 65ª chamada ficaria esperando para
        // sempre e o teste TRAVARIA a suíte inteira. Com a thread-fantasma, o
        // bloqueio vira uma falha legível.
        let fantasma = dispatcher.clone();
        let (tx_resultado, rx_resultado) =
            mpsc::channel::<Result<(usize, usize, Duration), String>>();
        std::thread::spawn(move || {
            let inicio = Instant::now();
            let mut aceitas = 0usize;
            let mut recusadas = 0usize;
            for _ in 0..80 {
                match fantasma.submit(Action::ToggleOutputMute) {
                    Ok(()) => aceitas += 1,
                    Err(SubmitError::Full) => recusadas += 1,
                    Err(outro) => {
                        let _ = tx_resultado
                            .send(Err(format!("erro inesperado ao enfileirar: {outro}")));
                        return;
                    }
                }
            }
            let _ = tx_resultado.send(Ok((aceitas, recusadas, inicio.elapsed())));
        });

        let (aceitas, recusadas, decorrido) =
            match rx_resultado.recv_timeout(Duration::from_secs(5)) {
                Ok(Ok(v)) => v,
                Ok(Err(mensagem)) => panic!("{mensagem}"),
                Err(err) => panic!(
                    "o produtor não voltou em 5 s ({err}): `submit` está bloqueando o produtor"
                ),
            };

        assert!(
            decorrido < Duration::from_secs(1),
            "submit levou {decorrido:?}: bloqueou o produtor"
        );
        assert_eq!(
            aceitas, QUEUE_CAPACITY,
            "a fila deveria aceitar {QUEUE_CAPACITY} ações com o worker ocupado"
        );
        assert_eq!(
            recusadas,
            80 - QUEUE_CAPACITY,
            "as excedentes têm de voltar como SubmitError::Full"
        );

        // Solta o worker para não deixar a thread pendurada no fim do teste.
        let _ = tx_liberar.send(());
    }

    #[test]
    fn acao_com_falha_nao_para_o_worker() {
        let (tx_eventos, rx_eventos) = mpsc::channel();
        let exec = FakeExecutor::new(tx_eventos);
        exec.falhar_na(0);
        let (report, avisos, registros) = registrador();
        let dispatcher = Dispatcher::spawn(exec.clone(), report, COALESCE_WINDOW);

        dispatcher
            .submit(Action::SetOutputMute { muted: true })
            .expect("fila com espaço");
        dispatcher
            .submit(Action::TogglePanel)
            .expect("fila com espaço");

        let vistas = espera(&rx_eventos, 2);
        assert_eq!(
            vistas,
            vec![Action::SetOutputMute { muted: true }, Action::TogglePanel]
        );
        espera_avisos(&avisos, 2);
        let publicados = registros.lock().unwrap().clone();
        assert_eq!(publicados.len(), 2, "o report roda uma vez por execução");
        assert_eq!(publicados[0].1.error_code, Some(ErrorCode::DeviceGone));
        assert!(publicados[1].1.ok, "a segunda ação tem de rodar normalmente");
    }

    #[test]
    fn executor_em_panico_nao_mata_o_worker() {
        let (tx_eventos, rx_eventos) = mpsc::channel();
        let exec = FakeExecutor::new(tx_eventos);
        exec.panicar_na(0);
        let (report, avisos, registros) = registrador();
        let dispatcher = Dispatcher::spawn(exec.clone(), report, COALESCE_WINDOW);

        dispatcher
            .submit(Action::ToggleOutputMute)
            .expect("fila com espaço");
        dispatcher
            .submit(Action::TogglePanel)
            .expect("fila com espaço");

        let vistas = espera(&rx_eventos, 2);
        assert_eq!(
            vistas,
            vec![Action::ToggleOutputMute, Action::TogglePanel]
        );

        // O pânico tem de virar uma falha publicada: o `report` não pode engolir o
        // resultado, senão quem chamou fica sem resposta.
        espera_avisos(&avisos, 2);
        let publicados = registros.lock().unwrap().clone();
        assert_eq!(publicados.len(), 2, "o report roda uma vez por execução");
        assert!(
            !publicados[0].1.ok,
            "o pânico do executor tem de virar falha, não sucesso"
        );
        assert_eq!(
            publicados[0].1.error_code,
            Some(ErrorCode::Internal),
            "o pânico é bug do executor, não recusa do Windows"
        );
        assert_eq!(
            publicados[0].1.message.as_deref(),
            Some("A ação falhou de forma inesperada.")
        );
        assert!(
            publicados[1].1.ok,
            "a segunda ação tem de rodar normalmente"
        );
    }

    #[test]
    fn dispatcher_e_send_e_sync() {
        fn exige<T: Send + Sync>() {}
        exige::<Dispatcher>();
        exige::<SubmitError>();
    }
}
