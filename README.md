<p align="center"><img src="assets/icon/png/sonar-256.png" alt="Sonar" width="128"></p>

# Sonar

App de bandeja para Windows 10/11 (Tauri: Rust + React/TS) que controla o áudio melhor que o mixer do Windows: **cada app numa saída diferente** (ex.: Spotify na caixa, Discord e Chrome no headset), perfis com ícone e regras que o app lembra sozinho.

Estado: **fase de estudo**. Existe só um spike descartável em Rust (`src/main.rs`) que provou que o roteamento por app funciona. Decisões e pendências em [`docs/DECISOES.md`](docs/DECISOES.md).

## Spike (descartável)

```
cargo run -- list                    # dispositivos de saída + apps com áudio
cargo run -- get <pid>               # rota gravada para o app
cargo run -- route <pid> <idx>       # manda o app para o dispositivo #idx
cargo run -- clear <pid>             # volta o app para o padrão do Windows
cargo run -- roundtrip <pid>         # lê a rota e grava de volta (diagnóstico)
```

Com `MIC=1` (variável de ambiente) todos os comandos operam em **entrada de áudio** (microfones) em vez de saída.

Usa a interface não documentada `Windows.Media.Internal.AudioPolicyConfig` (`SetPersistedDefaultAudioEndpoint`), a mesma do EarTrumpet (MIT) e do Earmark. Validado em Windows 11 build 26300 (variante "21H2" da interface). Windows 10 **não testado** (usa a variante Downlevel).

### Armadilha que custou horas

O ID do dispositivo enviado ao Windows precisa ser **exatamente**
`\\?\SWD#MMDEVAPI#{0.0.0.00000000}.{GUID}#{e6327cad-dcec-4949-ae8a-991e976a79d2}`
(duas barras no início). Com uma barra a menos o Windows responde `E_INVALIDARG` (0x80070057) sem dar pista. Diagnóstico que resolveu: `get` devolve o texto que o Windows guardou; comparar o tamanho (111 caracteres) com o que o código monta.

## Ícone

Fonte vetorial em `assets/icon/sonar.svg` (app) e `sonar-tray.svg` (bandeja). Os PNGs e o `.ico` são gerados por `python tools/make_icons.py` (Pillow). Paleta: grafite `#1B1A18`, latão `#D9A441`, branco `#F7F4EC`. A bandeja tem versão clara (`tray/`) e escura (`tray-light/`, para a barra de tarefas do tema claro).

## Referências

- EarTrumpet: https://github.com/File-New-Project/EarTrumpet (`EarTrumpet/Interop/MMDeviceAPI/`, `AudioPolicyConfigService.cs`)
- Earmark: https://github.com/hoobio/earmark (`src/Earmark.Audio/Interop/`)
