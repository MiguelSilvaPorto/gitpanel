# GitPanel

Objetivo: painel desktop em Rust para listar e criar repositórios do GitHub, sincronizar commits, exibir gráficos de frequência e diffs de código.

Stack fixa: Rust 2024 + eframe/egui, interface em PT-BR, SQLite local, reqwest, autenticação via `gh`, token somente em memória.

## Comandos

- Setup local: `./scripts/setup-dev.sh`
- Checagem rápida: `cargo check -p gitpanel`
- Executar: `cargo run -p gitpanel`
- Testes: `cargo test --workspace`
- Formatação: `cargo fmt --all`
- Lint: `cargo clippy --workspace --all-targets -- -D warnings`

O `target/` local aponta para o NVMe via `setup-dev.sh`; não limpar sem necessidade. O linker usa `mold` quando disponível.

## Regras

- Todo texto de interface passa por `gitpanel-i18n`; chaves em kebab-case, com placeholders e plural do Fluent. Nunca concatenar fragmentos traduzidos.
- `gitpanel-core`, `gitpanel-github`, `gitpanel-db`, `gitpanel-sync`, `gitpanel-diff` e `gitpanel-metrics` não dependem de i18n. Erros carregam códigos; datas ficam em UTC.
- O token do `gh` nunca é gravado em disco nem logado. Nada de `unwrap`/`expect` em produção e nada de `unsafe`.
- Exclusão de repositório está fora do MVP. Na fase 2, exige `delete_repo`, confirmação digitando `owner/repo` e auditoria local.
- O README descreve apenas o objetivo central; estrutura e detalhes ficam no código.
- Commits semânticos. Não commitar `target/`, bancos SQLite, `.env` ou segredos.
- Sync cobre todos os repositórios acessíveis, na branch padrão, de forma incremental, respeitando o rate limit.
