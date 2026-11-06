//! Ligação entre a fila de ações e o app: o executor que sabe mexer no painel, o aviso de
//! resultado e o dono da recusa da fila.
//!
//! Toda entrada (atalho, bandeja, IPC) vira uma `Action` e passa pelo `Dispatcher`. Este
//! módulo é a camada que faz a fila falar com o app: quem executa, quem avisa o painel e
//! o que acontece quando a fila não aceita a ação.

use std::sync::Arc;

use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::Shortcut;

use crate::actions::{Action, ActionExecutor, ActionResult, ErrorCode};
use crate::bindings::{action_for, Binding};
use crate::dispatcher::{Dispatcher, SubmitError};
use crate::engine::Engine;
use crate::shell;

/// Evento com o resultado de uma ação que não deu certo (falha ou sucesso parcial).
///
/// O evento só é emitido quando `ok == false`: o painel reage ao que deu errado, não ao
/// que deu certo. Um sucesso parcial (`ok == false`, `changed == true`) também é emitido —
/// o usuário precisa saber que o pedido foi cumprido só pela metade.
pub const ACTION_RESULT: &str = "action-result";

/// Executor que sabe falar com o painel.
///
/// O painel é do app, não do motor de áudio: quem o mexe é a `shell`. Todo o resto é
/// delegação ao `Engine`.
///
/// **A trava `apply` nunca é segurada aqui.** Ela é um `Mutex` do std (não reentrante) e
/// pertence ao `Engine`: cada `act_*` a segura por conta própria. Segurá-la também neste
/// nível seria um deadlock garantido.
pub struct AppExecutor {
    engine: Arc<Engine>,
    /// Altera o painel. Na produção é a `shell` pelo `AppHandle`; no teste é uma função
    /// — o `AppHandle` do Tauri não existe fora do app.
    alternar_painel: Box<dyn Fn() -> bool + Send + Sync>,
}

impl AppExecutor {
    pub fn new(engine: Arc<Engine>, app: AppHandle) -> Self {
        let app = app.clone();
        AppExecutor {
            engine,
            alternar_painel: Box::new(move || shell::toggle_from_shortcut(&app)),
        }
    }

    /// O mesmo executor, com o painel sob controle do teste. O resto do caminho é idêntico
    /// ao do atalho: é o que prova que atalho e IPC dão o mesmo resultado (SPEC §8).
    #[cfg(test)]
    pub fn com_painel(
        engine: Arc<Engine>,
        alternar_painel: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        AppExecutor {
            engine,
            alternar_painel: Box::new(alternar_painel),
        }
    }
}

impl ActionExecutor for AppExecutor {
    fn execute(&self, action: Action) -> ActionResult {
        match action {
            // O painel não é do motor de áudio: quem o mexe é a `shell`.
            Action::TogglePanel => {
                let mudou = (self.alternar_painel)();
                ActionResult::done(mudou, None)
            }
            outra => self.engine.execute(outra),
        }
    }
}

/// O aviso vai para o painel quando a ação não deu certo?
///
/// Só quando `ok == false`: o painel reage ao que deu errado, não ao que deu certo. Um
/// sucesso parcial (`ok == false`, `changed == true`) **também** avisa — o pedido foi
/// cumprido só pela metade e o usuário precisa saber (Ruling 35/39).
pub fn reporta_se_falhou(resultado: &ActionResult) -> bool {
    !resultado.ok
}

/// Publica uma falha: no log e no evento `action-result`.
///
/// O painel pode estar oculto — o evento fica esperando ele abrir. O log garante que a
/// informação não se perca.
pub fn publish_failure(app: &AppHandle, resultado: &ActionResult) {
    publish_failure_to(
        &|evento, resultado| {
            let _ = app.emit(evento, resultado);
        },
        resultado,
    );
}

/// O mesmo aviso, para um emissor qualquer.
///
/// O emissor vem por parâmetro para o aviso poder ser testado **sem o Tauri**: em
/// produção é o `app.emit`, no teste é uma fila. O `let _ = app.emit(...)` engole o erro
/// do emit — o que o painel recebe é o resultado, e é isso que o teste confere.
pub fn publish_failure_to(emit: &dyn Fn(&str, &ActionResult), resultado: &ActionResult) {
    let frase = resultado.message.as_deref().unwrap_or("sem mensagem");
    eprintln!("sonar: a ação falhou: {frase}");
    emit(ACTION_RESULT, resultado);
}

/// O aviso que vai para o painel quando uma ação não deu certo.
///
/// É o mesmo caminho do `publish_failure`, pronto para ser usado como callback.
pub fn notificador(app: &AppHandle) -> impl Fn(&Action, &ActionResult) + Send + Sync + 'static {
    let app = app.clone();
    move |_acao, resultado| publish_failure(&app, resultado)
}

/// Enfileira uma ação e trata a recusa da fila.
///
/// A fila cheia não pode ser engolida (RNF-02): quem produz precisa saber que a ação não
/// entrou. A recusa vira log **e** aviso no painel, com `ok: false` e um código de erro
/// próprio.
pub fn submit(
    dispatcher: &Dispatcher,
    action: Action,
    avisar: &dyn Fn(&Action, &ActionResult),
) {
    if let Err(err) = dispatcher.submit(action.clone()) {
        eprintln!("sonar: a ação não entrou na fila: {err}");
        avisar(&action, &queue_failure(err));
    }
}

/// O aviso que o painel recebe quando a fila não aceita a ação.
pub fn queue_failure(err: SubmitError) -> ActionResult {
    match err {
        SubmitError::Full => ActionResult::failed(
            ErrorCode::QueueFull,
            "A fila de ações está cheia. Tente de novo em instantes.",
        ),
        // O worker encerrou: é falha do app, não recusa do Windows.
        SubmitError::Closed => ActionResult::failed(
            ErrorCode::Internal,
            "O processador de ações encerrou.",
        ),
    }
}

/// O handler do atalho: procura a ação na tabela e **enfileira**.
///
/// O atalho não executa nada: a execução é do worker da fila. Assim o atalho nunca
/// espera pelo Windows (RF-02/RNF-01) — ele devolve na hora, mesmo com a fila ocupada.
///
/// Atalho sem ação na tabela é ignorado.
pub fn enfileirar_atalho(
    dispatcher: &Dispatcher,
    bindings: &[Binding],
    pressed: &Shortcut,
    avisar: &dyn Fn(&Action, &ActionResult),
) {
    let Some(acao) = action_for(bindings, pressed) else {
        return;
    };
    submit(dispatcher, acao, avisar);
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::bindings::default_bindings;
    use crate::dispatcher::QUEUE_CAPACITY;

    /// Executor de mentira: a primeira execução fica presa até o teste mandar soltar.
    struct ExecutorDeTeste {
        chamadas: Mutex<Vec<Action>>,
        porta: Mutex<Option<Porta>>,
    }

    /// Barreira do executor de teste.
    struct Porta {
        entrou: mpsc::Sender<()>,
        liberar: mpsc::Receiver<()>,
    }

    impl ExecutorDeTeste {
        fn new() -> Arc<ExecutorDeTeste> {
            Arc::new(ExecutorDeTeste {
                chamadas: Mutex::new(Vec::new()),
                porta: Mutex::new(None),
            })
        }

        /// Trava a primeira execução até o teste soltar.
        fn travar(&self, entrou: mpsc::Sender<()>, liberar: mpsc::Receiver<()>) {
            *self.porta.lock().unwrap() = Some(Porta { entrou, liberar });
        }

        fn chamadas(&self) -> Vec<Action> {
            self.chamadas.lock().unwrap().clone()
        }
    }

    impl ActionExecutor for ExecutorDeTeste {
        fn execute(&self, action: Action) -> ActionResult {
            self.chamadas.lock().unwrap().push(action.clone());
            let porta = self.porta.lock().unwrap().take();
            if let Some(porta) = porta {
                let _ = porta.entrou.send(());
                let _ = porta.liberar.recv();
            }
            ActionResult::done(true, None)
        }
    }

    /// Report que não faz nada: o teste olha a fila, não os avisos.
    fn report_mudo() -> Arc<dyn Fn(&Action, &ActionResult) + Send + Sync> {
        Arc::new(|_acao: &Action, _resultado: &ActionResult| {})
    }

    #[test]
    fn atalho_so_enfileira_e_nao_executa() {
        // RF-02/RNF-01: com o executor preso, o handler do atalho tem de voltar na hora
        // e não pode chamar o executor — a execução é do worker.
        let exec = ExecutorDeTeste::new();
        let (tx_entrou, rx_entrou) = mpsc::channel();
        let (tx_liberar, rx_liberar) = mpsc::channel();
        exec.travar(tx_entrou, rx_liberar);
        let dispatcher = Dispatcher::spawn_default(exec.clone(), report_mudo());

        // Ocupa o worker: a primeira execução fica parada dentro do executor.
        dispatcher
            .submit(Action::ToggleOutputMute)
            .expect("fila com espaço");
        rx_entrou
            .recv_timeout(Duration::from_secs(5))
            .expect("o worker não chegou ao executor");

        let pressed: Shortcut = "Ctrl+Alt+Up".parse().unwrap();
        let inicio = Instant::now();
        enfileirar_atalho(
            &dispatcher,
            &default_bindings(),
            &pressed,
            &|_acao, _resultado| {},
        );
        let decorrido = inicio.elapsed();

        assert!(
            decorrido < Duration::from_millis(200),
            "o handler do atalho esperou a execução: {decorrido:?}"
        );
        assert_eq!(
            exec.chamadas(),
            vec![Action::ToggleOutputMute],
            "o handler não pode executar a ação: ele só enfileira"
        );

        // Solta o worker: a ação enfileirada chega ao executor (é o worker que executa).
        let _ = tx_liberar.send(());
        let limite = Instant::now() + Duration::from_secs(5);
        while exec.chamadas().len() < 2 && Instant::now() < limite {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            exec.chamadas(),
            vec![
                Action::ToggleOutputMute,
                Action::StepOutputVolume { delta: 0.05 }
            ],
            "a ação do atalho tem de chegar ao executor pela fila"
        );
    }

    #[test]
    fn fila_cheia_devolve_full_e_avisa_o_painel() {
        // RNF-02: a fila cheia precisa de dono. A recusa tem de chegar ao painel como um
        // aviso com `ok: false` e um código de erro próprio.
        let exec = ExecutorDeTeste::new();
        let (tx_entrou, rx_entrou) = mpsc::channel();
        let (tx_liberar, rx_liberar) = mpsc::channel();
        exec.travar(tx_entrou, rx_liberar);
        let dispatcher = Dispatcher::spawn_default(exec.clone(), report_mudo());

        dispatcher
            .submit(Action::ToggleOutputMute)
            .expect("fila com espaço");
        rx_entrou
            .recv_timeout(Duration::from_secs(5))
            .expect("o worker não chegou ao executor");

        // Enche a fila: o worker está preso, as vagas restantes ficam com as ações.
        for _ in 0..QUEUE_CAPACITY {
            dispatcher
                .submit(Action::ToggleOutputMute)
                .expect("fila com espaço");
        }

        // A próxima ação não entra: a fila está cheia.
        assert_eq!(
            dispatcher.submit(Action::ToggleOutputMute),
            Err(SubmitError::Full)
        );

        // E o dono da recusa avisa o painel.
        let avisos: Arc<Mutex<Vec<ActionResult>>> = Arc::new(Mutex::new(Vec::new()));
        let destino = avisos.clone();
        submit(
            &dispatcher,
            Action::ToggleOutputMute,
            &|_acao, resultado| destino.lock().unwrap().push(resultado.clone()),
        );

        let publicados = avisos.lock().unwrap().clone();
        assert_eq!(publicados.len(), 1, "a recusa tem de gerar um aviso");
        assert!(!publicados[0].ok, "o aviso de fila cheia não é sucesso");
        assert_eq!(
            publicados[0].error_code,
            Some(ErrorCode::QueueFull),
            "a fila cheia tem código próprio"
        );
        assert!(publicados[0]
            .message
            .as_deref()
            .is_some_and(|frase| !frase.is_empty()));

        let _ = tx_liberar.send(());
    }

    #[test]
    fn fila_cheia_nao_trava_quem_produz() {
        // Quem produz não espera: a recusa volta na hora.
        let exec = ExecutorDeTeste::new();
        let (tx_entrou, rx_entrou) = mpsc::channel();
        let (tx_liberar, rx_liberar) = mpsc::channel();
        exec.travar(tx_entrou, rx_liberar);
        let dispatcher = Dispatcher::spawn_default(exec.clone(), report_mudo());

        dispatcher
            .submit(Action::ToggleOutputMute)
            .expect("fila com espaço");
        rx_entrou
            .recv_timeout(Duration::from_secs(5))
            .expect("o worker não chegou ao executor");

        for _ in 0..QUEUE_CAPACITY {
            dispatcher
                .submit(Action::ToggleOutputMute)
                .expect("fila com espaço");
        }

        let inicio = Instant::now();
        let recusa = dispatcher.submit(Action::ToggleOutputMute);
        let decorrido = inicio.elapsed();

        assert_eq!(recusa, Err(SubmitError::Full));
        assert!(
            decorrido < Duration::from_millis(100),
            "a recusa demorou {decorrido:?}: quem produz não pode esperar"
        );

        let _ = tx_liberar.send(());
    }

    #[test]
    fn fila_fechada_vira_falha_interna() {
        // O worker encerrado é falha do app, não recusa do Windows.
        let falha = queue_failure(SubmitError::Closed);

        assert!(!falha.ok);
        assert_eq!(falha.error_code, Some(ErrorCode::Internal));
    }

    // ---------- o filtro do Ruling 17 ----------

    #[test]
    fn o_sucesso_nao_vira_aviso() {
        // O painel reage ao que deu errado, não ao que deu certo.
        assert!(!reporta_se_falhou(&ActionResult::done(true, None)));
        assert!(!reporta_se_falhou(&ActionResult::done(false, None)));
    }

    #[test]
    fn a_falha_vira_aviso() {
        assert!(reporta_se_falhou(&ActionResult::failed(
            ErrorCode::QueueFull,
            "A fila de ações está cheia."
        )));
    }

    #[test]
    fn o_sucesso_parcial_vira_aviso() {
        // `ok == false` não implica `changed == false`: o sucesso parcial também é
        // avisado (Ruling 35/39).
        let parcial = ActionResult::partial(
            ActionResult::done(true, Some("discord.exe".to_string())),
            ErrorCode::Unsupported,
            "O mudo foi aplicado em parte.",
        );

        assert!(reporta_se_falhou(&parcial));
    }

    // ---------- o evento da falha ----------

    #[test]
    fn o_evento_da_falha_tem_o_nome_esperado() {
        // O nome é o que o TypeScript escuta: `action-result`.
        assert_eq!(ACTION_RESULT, "action-result");
    }

    #[test]
    fn a_falha_publicada_vai_no_evento_com_o_erro_dela() {
        // O publicador é testado sem o Tauri: o emissor é uma fila.
        let publicado: Arc<Mutex<Vec<(String, ActionResult)>>> = Arc::new(Mutex::new(Vec::new()));
        let fila = publicado.clone();
        let falha = queue_failure(SubmitError::Full);

        publish_failure_to(
            &|evento, resultado| fila.lock().unwrap().push((evento.to_string(), resultado.clone())),
            &falha,
        );

        let avisos = publicado.lock().unwrap().clone();
        assert_eq!(avisos.len(), 1, "a falha tem de gerar um aviso");
        assert_eq!(avisos[0].0, "action-result", "o evento é o `action-result`");
        assert!(!avisos[0].1.ok, "o aviso de fila cheia não é sucesso");
        assert_eq!(
            avisos[0].1.error_code,
            Some(ErrorCode::QueueFull),
            "a fila cheia tem código próprio"
        );
    }
}
