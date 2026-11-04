# Publicar uma versão do Sonar

Como o app recebe atualizações: a cada tag `v*`, o GitHub Actions gera o instalador do Windows,
assina com a chave do updater e publica um release com dois arquivos principais: o instalador
(`Sonar_X.Y.Z_x64-setup.exe`) e o `latest.json`. O app consulta o `latest.json` pelo endpoint
configurado em `src-tauri/tauri.conf.json` (`plugins.updater.endpoints`). Se a versão for maior
que a instalada, o ponto aparece na engrenagem e a seção **Atualizações** oferece instalar.
Nada é instalado sem o usuário apertar o botão.

## Antes da primeira versão: escolha a visibilidade do repositório

O updater baixa o `latest.json` e o instalador **sem login**. Repositório privado no GitHub
responde 404 para quem não está autenticado, então a atualização não funciona enquanto o repo for
privado. Escolha uma opção **antes** de publicar a primeira versão:

1. **Tornar o repositório público (recomendado).** É o caminho de menos trabalho. O código fica
   visível, e o que o app precisa (o instalador assinado e o `latest.json`) já é público de
   qualquer forma. Depois de tornar público, nada muda no endpoint atual.
2. **Publicar os releases em outro repositório público** (ex.: `sthevan027/sonar-releases`).
   Precisa trocar o endpoint em `tauri.conf.json` para
   `https://github.com/sthevan027/sonar-releases/releases/latest/download/latest.json`, e o
   workflow precisa de um token (PAT) com permissão de escrita nesse repositório, guardado num
   secret, passado ao `tauri-action` pelos inputs `owner` e `repo`. O `GITHUB_TOKEN` padrão só
   cria releases no próprio repositório.
3. **Hospedar o `latest.json` em outro lugar** (ex.: Vercel ou um bucket público). Mesma troca de
   endpoint; o workflow deixa de publicar o JSON e você copia o arquivo para o host.

O endpoint é uma única linha em `src-tauri/tauri.conf.json`:

```json
"endpoints": ["https://github.com/sthevan027/sonar/releases/latest/download/latest.json"]
```

Hoje o endpoint está configurado para a opção 1 (repositório público, mesmo caminho).

## Chave de assinatura (uma vez só)

A chave pública já está em `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`). A chave
**privada** fica fora do repositório, em `%USERPROFILE%\.tauri\sonar-updater.key` (no PC de casa).

> **Faça backup da chave privada** (gerenciador de senhas ou pendrive). Se ela se perder,
> nenhuma versão instalada aceita atualização assinada por uma chave nova: quem tem o Sonar
> instalado precisa reinstalar à mão.

Para gerar de novo (só se a chave for perdida, e sabendo que isso quebra o caminho de atualização
das versões antigas):

```powershell
bun run tauri signer generate -w "$env:USERPROFILE\.tauri\sonar-updater.key" -p "" --ci
```

Depois troque `pubkey` em `tauri.conf.json` pelo conteúdo do arquivo `.pub`.

### Secrets no GitHub

Em **Settings > Secrets and variables > Actions** do repositório:

| Secret | Valor |
|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | Conteúdo inteiro do arquivo `sonar-updater.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | **Não crie.** A chave foi gerada sem senha. O GitHub não aceita secret vazio, então simplesmente deixe este secret de fora. |

Pelo terminal (com o `gh` autenticado), sem mostrar a chave na tela:

```powershell
Get-Content -Raw "$env:USERPROFILE\.tauri\sonar-updater.key" | gh secret set TAURI_SIGNING_PRIVATE_KEY --repo sthevan027/sonar
```

## Subir a versão

A versão fica em quatro lugares: `package.json`, `src-tauri/tauri.conf.json`,
`src-tauri/Cargo.toml` e a entrada `sonar` do `src-tauri/Cargo.lock`. O script mantém as quatro
iguais:

```powershell
python tools/bump_version.py 0.2.0          # grava 0.2.0 nos quatro arquivos
python tools/bump_version.py --check        # confere se estão iguais
```

Depois:

```powershell
git add package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "chore: versão 0.2.0"
git tag v0.2.0
git push origin v0.2.0
```

O workflow `release.yml` confere que a tag (`v0.2.0`) bate com a versão dos arquivos. Se não bater,
para antes de gerar qualquer coisa.

Para publicar sem tag, use **Actions > Release > Run workflow**. Ele usa a versão dos arquivos.

## Onde aparece o resultado

- **Release:** `https://github.com/sthevan027/sonar/releases/tag/v0.2.0`, com o instalador e o
  `.sig` ao lado.
- **latest.json:** `https://github.com/sthevan027/sonar/releases/latest/download/latest.json`.
  É o arquivo que o app lê. Só aponta para o último release não marcado como pré-lançamento.

## Testar a atualização de ponta a ponta

Não dá para testar tudo sem publicar. Roteiro:

1. Instale uma versão **que já tenha o updater** (a primeira versão com essa funcionalidade precisa
   ser instalada à mão; versões anteriores não sabem atualizar). Instale o `Sonar_X.Y.Z_x64-setup.exe`
   (NSIS, por usuário, sem UAC), não o `.msi`: o updater escolhe o mesmo tipo de instalador que estiver
   instalado, então quem instalou o `.msi` continua recebendo `.msi`.
2. Suba a versão seguinte (ex.: `0.2.0` depois de `0.1.0`) e espere o workflow terminar.
3. No app instalado: **Configurações > Atualizações > Verificar atualizações**. Deve aparecer
   "Versão 0.2.0 disponível". Apertar **Atualizar e reiniciar** baixa, verifica a assinatura e roda
   o instalador em modo passivo. No Windows o app fecha e reabre na versão nova.

Em build de desenvolvimento (`bun run tauri dev`) a checagem automática fica desligada; o botão
manual funciona.

## Build local do instalador

`bun run tauri build` gera também o `.sig` (`createUpdaterArtifacts`), então precisa da chave na
sessão. Pela sessão do PowerShell, sem gravar a chave em arquivo do projeto:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content -Raw "$env:USERPROFILE\.tauri\sonar-updater.key"
bun run tauri build
```

O build de instalador pede bastante memória (ver `docs/VERIFICACAO.md`). Sem a variável, o build
local falha no passo de assinatura; isso não afeta `tauri dev`.
