"""Versão do Sonar em um lugar só: sobe ou confere as quatro cópias.

Uso:
  python tools/bump_version.py 0.2.0          # grava 0.2.0 em todos os arquivos
  python tools/bump_version.py --check        # confere se estão iguais
  python tools/bump_version.py --check v0.2.0 # confere também a tag do release

Arquivos: package.json (versão da interface), src-tauri/tauri.conf.json (versão que o
updater compara), src-tauri/Cargo.toml e a entrada "sonar" em src-tauri/Cargo.lock.
Edita por regex para não reformatar o arquivo. Só usa a biblioteca padrão.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SEMVER = re.compile(r"^\d+\.\d+\.\d+$")

# (caminho, regex com um grupo para a versão atual). Cada regex casa uma vez por arquivo.
TARGETS = {
    "package.json": re.compile(r'^(  "version": ")([^"]+)(")', re.MULTILINE),
    "src-tauri/tauri.conf.json": re.compile(r'^(  "version": ")([^"]+)(")', re.MULTILINE),
    "src-tauri/Cargo.toml": re.compile(r'^(version = ")([^"]+)(")', re.MULTILINE),
    "src-tauri/Cargo.lock": re.compile(r'(name = "sonar"\r?\nversion = ")([^"]+)(")'),
}


def read(rel: str) -> str:
    # newline="" preserva CRLF se o arquivo tiver.
    with open(ROOT / rel, encoding="utf-8", newline="") as f:
        return f.read()


def write(rel: str, text: str) -> None:
    with open(ROOT / rel, "w", encoding="utf-8", newline="") as f:
        f.write(text)


def current_versions() -> dict[str, str]:
    found: dict[str, str] = {}
    for rel, pattern in TARGETS.items():
        match = pattern.search(read(rel))
        if not match:
            raise SystemExit(f"Versão não encontrada em {rel}.")
        found[rel] = match.group(2)
    return found


def bump(version: str) -> None:
    if not SEMVER.match(version):
        raise SystemExit(f"Versão inválida: {version!r}. Use X.Y.Z, sem o v.")
    for rel, pattern in TARGETS.items():
        text = read(rel)
        new_text, count = pattern.subn(lambda m: f"{m.group(1)}{version}{m.group(3)}", text, count=1)
        if count != 1:
            raise SystemExit(f"Não encontrei a versão em {rel}.")
        write(rel, new_text)
        print(f"{rel}: {version}")
    print("Pronto. Confira com: git diff, depois commit, tag e push (docs/RELEASE.md).")


def check(tag: str | None) -> None:
    versions = current_versions()
    distinct = set(versions.values())
    for rel, value in versions.items():
        print(f"{rel}: {value}")
    if len(distinct) != 1:
        raise SystemExit("As versões não batem. Rode: python tools/bump_version.py X.Y.Z")
    version = distinct.pop()
    if tag is not None:
        expected = f"v{version}"
        if tag != expected:
            raise SystemExit(f"A tag {tag!r} não bate com a versão do app ({expected!r}).")
        print(f"Tag {tag} confere.")
    print(f"Versão {version} em todos os arquivos.")


def main(argv: list[str]) -> None:
    if not argv:
        raise SystemExit(__doc__)
    if argv[0] == "--check":
        check(argv[1] if len(argv) > 1 else None)
    elif len(argv) == 1:
        bump(argv[0])
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main(sys.argv[1:])
