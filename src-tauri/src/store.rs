//! Persistência de JSON versionado com gravação atômica e backup.
//!
//! Cada documento vive em `<dir>/<name>.json`, no formato `{"version": N, "data": ...}`.
//! Arquivos auxiliares: `.json.tmp` (gravação em andamento), `.json.bak` (versão anterior)
//! e `.json.corrupt` (arquivo ilegível movido para fora do caminho de leitura).

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub const CURRENT_VERSION: u32 = 2;

pub struct Store {
    dir: PathBuf,
}

#[derive(Serialize)]
struct Envelope<'a, T> {
    version: u32,
    data: &'a T,
}

#[derive(Deserialize)]
struct OwnedEnvelope<T> {
    version: u32,
    data: T,
}

impl Store {
    pub fn open(dir: PathBuf) -> Store {
        Store { dir }
    }

    /// Nunca falha: devolve `T::default()` quando não há dado legível.
    ///
    /// É o `load_versioned` sem a versão: o caminho de recuperação (principal, `.corrupt`,
    /// `.bak`) fica num lugar só.
    pub fn load<T: DeserializeOwned + Default>(&self, name: &str) -> T {
        self.load_versioned(name).1
    }

    /// Lê um documento junto com a versão do formato em que ele está.
    ///
    /// A versão é **0** quando o arquivo não existe ou não dá para ler. Quem chama
    /// decide o que fazer com a versão (é o que permite migrar um formato antigo).
    pub fn load_versioned<T: DeserializeOwned + Default>(&self, name: &str) -> (u32, T) {
        let main = self.path(name, ".json");
        if main.exists() {
            if let Some(encontrado) = read_versioned_document(&main) {
                return encontrado;
            }
            // Ilegível: guarda para diagnóstico e tenta o backup.
            let _ = fs::rename(&main, self.path(name, ".json.corrupt"));
        }
        read_versioned_document(&self.path(name, ".json.bak")).unwrap_or((0, T::default()))
    }

    pub fn save<T: Serialize>(&self, name: &str, value: &T) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let bytes = serde_json::to_vec_pretty(&Envelope {
            version: CURRENT_VERSION,
            data: value,
        })
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

        let main = self.path(name, ".json");
        let tmp = self.path(name, ".json.tmp");
        {
            let mut file = File::create(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
        }
        if main.exists() {
            fs::copy(&main, self.path(name, ".json.bak"))?;
        }
        // No Windows, rename substitui o destino existente.
        fs::rename(&tmp, &main)
    }

    fn path(&self, name: &str, suffix: &str) -> PathBuf {
        self.dir.join(format!("{name}{suffix}"))
    }
}

/// Lê um documento e devolve a versão junto com o conteúdo.
fn read_versioned_document<T: DeserializeOwned>(path: &Path) -> Option<(u32, T)> {
    let bytes = fs::read(path).ok()?;
    let envelope: OwnedEnvelope<T> = serde_json::from_slice(&bytes).ok()?;
    Some((envelope.version, envelope.data))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use serde_json::json;
    use std::fs;

    #[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
    struct Sample {
        n: u32,
        s: String,
    }

    fn sample(n: u32, s: &str) -> Sample {
        Sample { n, s: s.to_string() }
    }

    fn open_temp() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf());
        (dir, store)
    }

    #[test]
    fn save_then_load_roundtrip() {
        let (_dir, store) = open_temp();
        store.save("config", &sample(7, "abc")).unwrap();
        let loaded: Sample = store.load("config");
        assert_eq!(loaded, sample(7, "abc"));
    }

    #[test]
    fn save_keeps_previous_as_bak() {
        let (dir, store) = open_temp();
        store.save("config", &sample(1, "primeiro")).unwrap();
        store.save("config", &sample(2, "segundo")).unwrap();

        let bak = fs::read(dir.path().join("config.json.bak")).unwrap();
        let bak: serde_json::Value = serde_json::from_slice(&bak).unwrap();
        assert_eq!(bak["data"], json!({"n": 1, "s": "primeiro"}));

        let main: Sample = store.load("config");
        assert_eq!(main, sample(2, "segundo"));
    }

    #[test]
    fn load_falls_back_to_bak_when_main_is_truncated() {
        let (dir, store) = open_temp();
        store.save("config", &sample(1, "bom")).unwrap();
        store.save("config", &sample(2, "novo")).unwrap();
        // Simula queda de energia no meio da gravação do arquivo principal.
        fs::write(dir.path().join("config.json"), r#"{"version":1,"#).unwrap();

        let loaded: Sample = store.load("config");
        assert_eq!(loaded, sample(1, "bom"));
        assert!(dir.path().join("config.json.corrupt").exists());
    }

    #[test]
    fn load_returns_default_and_keeps_corrupt_file_when_both_invalid() {
        let (dir, store) = open_temp();
        fs::write(dir.path().join("config.json"), "isto não é json").unwrap();
        fs::write(dir.path().join("config.json.bak"), "{também quebrado").unwrap();

        let loaded: Sample = store.load("config");
        assert_eq!(loaded, Sample::default());
        assert!(dir.path().join("config.json.corrupt").exists());
    }

    #[test]
    fn load_missing_file_returns_default() {
        let (_dir, store) = open_temp();
        let loaded: Sample = store.load("nao-existe");
        assert_eq!(loaded, Sample::default());
    }

    #[test]
    fn save_does_not_leave_tmp_file() {
        let (dir, store) = open_temp();
        store.save("config", &sample(3, "x")).unwrap();
        assert!(!dir.path().join("config.json.tmp").exists());
        assert!(dir.path().join("config.json").exists());
    }

    #[test]
    fn load_versioned_returns_zero_when_the_file_is_missing() {
        let (_dir, store) = open_temp();
        let (version, value) = store.load_versioned::<Sample>("nao-existe");
        assert_eq!(version, 0);
        assert_eq!(value, Sample::default());
    }

    #[test]
    fn load_versioned_returns_the_version_written_on_disk() {
        let (dir, store) = open_temp();
        fs::write(
            dir.path().join("config.json"),
            r#"{"version":1,"data":{"n":3,"s":"x"}}"#,
        )
        .unwrap();

        let (version, value) = store.load_versioned::<Sample>("config");

        assert_eq!(version, 1);
        assert_eq!(value, sample(3, "x"));
    }

    #[test]
    fn load_versioned_returns_zero_when_the_file_is_garbage() {
        let (dir, store) = open_temp();
        fs::write(dir.path().join("config.json"), "isto não é json").unwrap();

        let (version, value) = store.load_versioned::<Sample>("config");

        assert_eq!(version, 0);
        assert_eq!(value, Sample::default());
        assert!(dir.path().join("config.json.corrupt").exists());
    }

    #[test]
    fn save_writes_the_current_version() {
        let (dir, store) = open_temp();
        store.save("config", &sample(1, "x")).unwrap();

        let bytes = fs::read(dir.path().join("config.json")).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["version"], CURRENT_VERSION);
    }
}
