//! Atualização automática: checa o `latest.json` publicado nas GitHub Releases, baixa e
//! instala a versão assinada. A checagem em segundo plano só avisa (evento `update-available`
//! e o ponto no ícone de engrenagem); a instalação só começa quando o usuário aperta o botão.
//! Funções puras (DTO, progresso, classificação de erro, trava) ficam no topo e têm testes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::UpdaterExt;

/// Evento com `UpdateInfo` quando uma versão nova é encontrada (checagem automática ou manual).
pub const UPDATE_AVAILABLE: &str = "update-available";
/// Evento com `Progress` durante o download da atualização.
pub const UPDATE_PROGRESS: &str = "update-progress";

/// Primeira checagem automática: espera o painel terminar de carregar.
const FIRST_CHECK: Duration = Duration::from_secs(30);
/// Intervalo entre checagens automáticas.
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// No máximo um evento de progresso a cada 150 ms (o último pedaço sempre sai).
const PROGRESS_EVERY: Duration = Duration::from_millis(150);

/// Versão nova encontrada (`check_for_update`). `notes` é texto puro, pode vir vazio.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    /// Data de publicação em segundos desde 1970 (UTC), quando o manifesto traz.
    pub date: Option<i64>,
}

/// Payload de `update-progress`. `total` é `None` quando o servidor não informa o tamanho.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Progress {
    pub downloaded: u64,
    pub total: Option<u64>,
}

/// Motivo de falha, em termos que a interface entende. A frase vem de `message_for`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Offline,
    Timeout,
    NoRelease,
    Signature,
    Other,
}

/// Estado do updater no Rust: última versão encontrada e a trava contra instalação dupla.
#[derive(Default)]
pub struct UpdateState {
    found: Mutex<Option<UpdateInfo>>,
    installing: AtomicBool,
}

impl UpdateState {
    /// Marca a instalação como em curso. `false` se já havia uma (clique duplo).
    pub fn begin_install(&self) -> bool {
        !self.installing.swap(true, Ordering::SeqCst)
    }

    /// Libera a trava depois de uma falha (em caso de sucesso o processo já saiu).
    pub fn end_install(&self) {
        self.installing.store(false, Ordering::SeqCst);
    }

    fn remember(&self, found: Option<UpdateInfo>) {
        *lock(&self.found) = found;
    }

    fn pending(&self) -> Option<UpdateInfo> {
        lock(&self.found).clone()
    }
}

/// Trava envenenada (uma thread caiu segurando) não impede de seguir: o dado continua válido.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Monta o `UpdateInfo` a partir dos campos do manifesto. Notas vazias viram `""` e as
/// quebras de linha do Windows (`\r\n`) viram `\n`.
pub fn info_from_parts(version: &str, body: Option<&str>, date: Option<i64>) -> UpdateInfo {
    UpdateInfo {
        version: version.trim().to_string(),
        notes: body.unwrap_or("").replace("\r\n", "\n").trim().to_string(),
        date,
    }
}

/// Soma os pedaços baixados e decide quando emitir progresso (throttle de `PROGRESS_EVERY`).
#[derive(Debug, Default)]
pub struct ProgressTally {
    downloaded: u64,
    last_emit: Option<Instant>,
}

impl ProgressTally {
    /// Registra um pedaço de `len` bytes. Devolve o progresso quando vale emitir: a primeira
    /// vez, a cada `PROGRESS_EVERY` e sempre quando o download chega ao total.
    pub fn chunk(&mut self, len: usize, total: Option<u64>, now: Instant) -> Option<Progress> {
        self.downloaded = self.downloaded.saturating_add(len as u64);
        let finished = total.is_some_and(|t| self.downloaded >= t);
        let due = self
            .last_emit
            .is_none_or(|at| now.duration_since(at) >= PROGRESS_EVERY);
        if finished || due {
            self.last_emit = Some(now);
            Some(Progress {
                downloaded: self.downloaded,
                total,
            })
        } else {
            None
        }
    }
}

/// Frase em português para cada motivo de falha. Não mostra detalhe técnico.
pub fn message_for(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Offline => "Sem conexão com a internet. Verifique a rede e tente de novo.",
        ErrorKind::Timeout => "A verificação demorou demais. Tente de novo.",
        ErrorKind::NoRelease => "Nenhuma versão publicada para atualizar ainda.",
        ErrorKind::Signature => "A assinatura da atualização não confere. Nada foi instalado.",
        ErrorKind::Other => "Não foi possível concluir a atualização agora. Tente de novo.",
    }
}

/// Classifica um erro do plugin. 404 do repositório chega como `ReleaseNotFound` (o plugin
/// não distingue "sem release" de "repositório privado"), por isso a frase é genérica.
pub fn error_kind(err: &tauri_plugin_updater::Error) -> ErrorKind {
    use tauri_plugin_updater::Error;
    match err {
        Error::ReleaseNotFound | Error::TargetNotFound(_) | Error::TargetsNotFound(_) => {
            ErrorKind::NoRelease
        }
        Error::Minisign(_) | Error::SignatureUtf8(_) | Error::Base64(_) => ErrorKind::Signature,
        Error::Reqwest(err) if err.is_timeout() => ErrorKind::Timeout,
        Error::Reqwest(_) | Error::Network(_) => ErrorKind::Offline,
        _ => ErrorKind::Other,
    }
}

fn error_text(err: &tauri_plugin_updater::Error) -> String {
    message_for(error_kind(err)).to_string()
}

/// Consulta o endpoint. `Ok(None)` quando já está na versão mais recente.
async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app.updater().map_err(|err| error_text(&err))?;
    let found = updater.check().await.map_err(|err| error_text(&err))?;
    Ok(found.map(|update| {
        info_from_parts(
            &update.version,
            update.body.as_deref(),
            update.date.map(|date| date.unix_timestamp()),
        )
    }))
}

/// Guarda o resultado (para o ponto da engrenagem e para a tela abrir já sabendo) e avisa.
fn remember(app: &AppHandle, found: Option<UpdateInfo>) {
    if let Some(info) = &found {
        // Falha ao emitir não importa: o estado guardado já basta para a interface.
        let _ = app.emit(UPDATE_AVAILABLE, info);
    }
    app.state::<UpdateState>().remember(found);
}

/// Checagem manual (botão nas configurações). Erros voltam como frase para a tela.
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let found = check(&app).await?;
    remember(&app, found.clone());
    Ok(found)
}

/// Última versão encontrada nesta execução (`null` quando não há). A engrenagem lê ao abrir.
#[tauri::command]
pub fn pending_update(state: State<'_, UpdateState>) -> Option<UpdateInfo> {
    state.pending()
}

/// Baixa, verifica a assinatura e instala. No Windows o instalador (modo passivo) fecha o app
/// e o reabre na versão nova: esta função não volta nesse caminho, então a tela deve ficar em
/// "instalando" sem tempo limite. Só uma falha antes do instalador devolve `Err`.
#[tauri::command]
pub async fn install_update(app: AppHandle, state: State<'_, UpdateState>) -> Result<(), String> {
    if !state.begin_install() {
        return Err("A atualização já está sendo instalada.".into());
    }
    let result = install(&app).await;
    if result.is_err() {
        state.end_install();
    }
    result
}

async fn install(app: &AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|err| error_text(&err))?;
    // Confere de novo: a versão pode ter mudado desde a última checagem.
    let update = updater
        .check()
        .await
        .map_err(|err| error_text(&err))?
        .ok_or_else(|| "A versão instalada já é a mais recente.".to_string())?;
    let mut tally = ProgressTally::default();
    let progress_handle = app.clone();
    update
        .download_and_install(
            move |len, total| {
                if let Some(progress) = tally.chunk(len, total, Instant::now()) {
                    let _ = progress_handle.emit(UPDATE_PROGRESS, progress);
                }
            },
            || {},
        )
        .await
        .map_err(|err| error_text(&err))
}

/// Checagem em segundo plano: uma vez logo após abrir e depois a cada 6 h. Só avisa; nunca
/// instala. Desligada em depuração (`cargo run`/`tauri dev`) para não gerar ruído.
pub fn start_background_checks(app: AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    std::thread::spawn(move || {
        std::thread::sleep(FIRST_CHECK);
        loop {
            crate::guarded("verificação de atualização", || {
                match tauri::async_runtime::block_on(check(&app)) {
                    Ok(found) => remember(&app, found),
                    // Sem rede ou sem release: silêncio (o botão manual mostra o motivo).
                    Err(err) => eprintln!("sonar: verificação de atualização falhou: {err}"),
                }
            });
            std::thread::sleep(CHECK_EVERY);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_keeps_version_and_date() {
        let info = info_from_parts(" 0.2.0 ", Some("Notas"), Some(1_700_000_000));
        assert_eq!(info.version, "0.2.0");
        assert_eq!(info.notes, "Notas");
        assert_eq!(info.date, Some(1_700_000_000));
    }

    #[test]
    fn info_without_body_has_empty_notes() {
        let info = info_from_parts("0.2.0", None, None);
        assert_eq!(info.notes, "");
        assert_eq!(info.date, None);
    }

    #[test]
    fn info_normalizes_windows_line_breaks() {
        let info = info_from_parts("0.2.0", Some("linha 1\r\nlinha 2\r\n"), None);
        assert_eq!(info.notes, "linha 1\nlinha 2");
    }

    #[test]
    fn progress_emits_first_chunk_then_throttles() {
        let mut tally = ProgressTally::default();
        let t0 = Instant::now();
        let first = tally.chunk(100, Some(1000), t0).expect("primeiro pedaço sai");
        assert_eq!(first, Progress { downloaded: 100, total: Some(1000) });
        assert!(tally.chunk(100, Some(1000), t0 + Duration::from_millis(10)).is_none());
        let later = tally
            .chunk(100, Some(1000), t0 + PROGRESS_EVERY)
            .expect("passou o intervalo");
        assert_eq!(later.downloaded, 300);
    }

    #[test]
    fn progress_always_emits_completion() {
        let mut tally = ProgressTally::default();
        let t0 = Instant::now();
        tally.chunk(500, Some(1000), t0);
        let done = tally
            .chunk(500, Some(1000), t0 + Duration::from_millis(1))
            .expect("chegou ao total");
        assert_eq!(done.downloaded, 1000);
    }

    #[test]
    fn progress_without_total_keeps_counting() {
        let mut tally = ProgressTally::default();
        let t0 = Instant::now();
        let first = tally.chunk(40, None, t0).expect("sai");
        assert_eq!(first, Progress { downloaded: 40, total: None });
    }

    #[test]
    fn messages_are_portuguese_and_distinct() {
        let kinds = [
            ErrorKind::Offline,
            ErrorKind::Timeout,
            ErrorKind::NoRelease,
            ErrorKind::Signature,
            ErrorKind::Other,
        ];
        let texts: Vec<&str> = kinds.iter().map(|k| message_for(*k)).collect();
        for (i, a) in texts.iter().enumerate() {
            assert!(!a.is_empty());
            for b in &texts[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert!(message_for(ErrorKind::Offline).contains("internet"));
        assert!(message_for(ErrorKind::Signature).contains("assinatura"));
    }

    #[test]
    fn plugin_errors_map_to_kinds() {
        use tauri_plugin_updater::Error;
        assert_eq!(error_kind(&Error::ReleaseNotFound), ErrorKind::NoRelease);
        assert_eq!(
            error_kind(&Error::TargetNotFound("windows-x86_64".into())),
            ErrorKind::NoRelease
        );
        assert_eq!(
            error_kind(&Error::SignatureUtf8("zz".into())),
            ErrorKind::Signature
        );
        assert_eq!(error_kind(&Error::Network("queda".into())), ErrorKind::Offline);
        assert_eq!(error_kind(&Error::EmptyEndpoints), ErrorKind::Other);
    }

    #[test]
    fn install_guard_blocks_second_start_until_released() {
        let state = UpdateState::default();
        assert!(state.begin_install());
        assert!(!state.begin_install());
        state.end_install();
        assert!(state.begin_install());
    }

    #[test]
    fn remember_replaces_and_clears_pending() {
        let state = UpdateState::default();
        assert_eq!(state.pending(), None);
        let info = info_from_parts("0.2.0", None, None);
        state.remember(Some(info.clone()));
        assert_eq!(state.pending(), Some(info));
        state.remember(None);
        assert_eq!(state.pending(), None);
    }
}
