# GitPanel

Aplicativo desktop em Rust para gerenciar e acompanhar repositórios do GitHub: listar os repositórios acessíveis, criar repositórios, sincronizar commits, navegar por pastas e arquivos em páginas dedicadas, editar arquivos com commit direto, explorar branches, releases, issues, pull requests e Actions, visualizar a frequência de commits em gráficos e inspecionar diffs de código.

A interface usa egui/eframe, é escrita em português — com estrutura pronta para novos idiomas — e autentica pelo GitHub CLI (`gh`), mantendo o token apenas em memória.

Licenciado sob MIT OR Apache-2.0.
