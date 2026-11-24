tab-overview = Visão geral
tab-code = Código
tab-commits = Commits
tab-branches = Branches
tab-releases = Releases
tab-issues = Issues
tab-pulls = Pull requests
tab-actions = Actions
tab-charts = Gráficos
tab-diff = Diff

commits-title = Commits
commits-empty-title = Nenhum commit sincronizado
commits-empty-body = Sincronize este repositório para carregar o histórico.
commits-loading = Carregando commits…
commits-sha = SHA
commits-message = Mensagem
commits-author = Autor
commits-date = Data
commits-changes = Alterações
commits-open-diff = Abrir diff
commits-count = { $count ->
    [one] { $count } commit
   *[other] { $count } commits
}
commits-load-more = Carregar mais
commits-showing = Mostrando { $shown } de { $total }

code-empty = Nenhum arquivo nesta pasta.
code-loading = Carregando arquivos…
code-binary = Arquivo binário.
code-too-large = Arquivo grande demais para exibir.
code-root = raiz
code-up = Subir um nível
code-readme = README
code-file-loading = Carregando arquivo…
code-select-branch = Selecione um branch
code-refresh = Recarregar pasta
code-edit = Editar arquivo
code-commit-message = Mensagem do commit
code-save-commit = Salvar e commitar
code-cancel-edit = Cancelar edição
code-committed = Commit criado.
code-edit-conflict = O arquivo foi alterado no GitHub. Recarregue antes de salvar.
code-edit-unavailable = Este arquivo não pode ser editado.
code-files-title = Arquivos do repositório
code-files-summary = { $files } { $files ->
        [one] arquivo
       *[other] arquivos
    } · { $dirs } { $dirs ->
        [one] pasta
       *[other] pastas
    }
code-tree-partial = parcial
code-tree-truncated = O GitHub limitou a listagem desta árvore; alguns arquivos podem não aparecer.
code-close-viewer = Fechar visualizador
code-read-only = Somente leitura
code-editing-mode = Editando
code-commit-placeholder = Descreva a alteração
code-commit-default = Atualiza { $path }
code-view-truncated = Exibindo os primeiros 50.000 caracteres. Abra no GitHub para ver o arquivo completo.
code-files-name = Nome
code-files-size = Tamanho
code-go-to-file = Ir para arquivo
code-go-to-file-empty = Nenhum arquivo encontrado.
code-clone = Código
code-clone-copied = URL do repositório copiada.
code-about = Sobre
code-languages = Idiomas
code-no-releases = Nenhuma release publicada.
code-view-releases = Ver releases
code-branches-count = { $count } { $count ->
        [one] branch
       *[other] branches
    }

branches-title = Branches
branches-empty = Nenhum branch sincronizado.
branches-default = padrão
branches-protected = protegida
branches-view-code = Ver código
branches-refresh = Recarregar branches

releases-title = Releases
releases-empty = Nenhuma release publicada.
releases-prerelease = pré-release
releases-draft = rascunho
releases-published = Publicada em { $when }
releases-assets = { $count ->
    [one] { $count } asset
   *[other] { $count } assets
}
releases-no-body = Sem notas de release.
releases-refresh = Recarregar releases
releases-notes = Notas

issues-title = Issues
issues-empty = Nenhuma issue encontrada.
issues-open = Abertas
issues-closed = Fechadas
issues-all = Todas
issues-refresh = Recarregar issues
issues-select = Selecione uma issue
issues-comments = { $count ->
    [one] { $count } comentário
   *[other] { $count } comentários
}
issues-no-body = Sem descrição.
issues-opened-by = por { $author }
issues-updated-at = atualizada { $when }

pulls-title = Pull requests
pulls-empty = Nenhum pull request encontrado.
pulls-open = Abertos
pulls-closed = Fechados
pulls-all = Todos
pulls-refresh = Recarregar pull requests
pulls-select = Selecione um pull request
pulls-draft = rascunho
pulls-merged = mesclado
pulls-from-to = { $head } → { $base }
pulls-comments = { $count ->
    [one] { $count } comentário
   *[other] { $count } comentários
}
pulls-commits = { $count ->
    [one] { $count } commit
   *[other] { $count } commits
}
pulls-files = { $count ->
    [one] { $count } arquivo
   *[other] { $count } arquivos
}
pulls-view-diff = Ver diff
pulls-loading-diff = Carregando diff…
pulls-no-diff = Nenhum arquivo de diff carregado.
pulls-no-body = Sem descrição.
pulls-opened-by = por { $author }
pulls-updated-at = atualizado { $when }

actions-title = Actions
actions-empty = Nenhuma execução encontrada.
actions-refresh = Recarregar Actions
actions-all = Todas
actions-active = Em andamento
actions-completed = Concluídas
actions-run = Execução #{ $number }
actions-event = evento { $event }
actions-branch = branch { $branch }
actions-updated = atualizada { $when }
actions-jobs = Jobs
actions-logs = Logs
actions-loading-logs = Carregando logs…
actions-no-logs = Selecione um job e clique em Logs.
actions-steps = Etapas
actions-runner = runner { $name }
actions-status-queued = na fila
actions-status-in-progress = em andamento
actions-status-completed = concluída
actions-status-success = sucesso
actions-status-failure = falha
actions-status-cancelled = cancelada
actions-status-skipped = ignorada
actions-status-neutral = neutra
actions-status-timed-out = tempo esgotado
actions-status-action-required = ação necessária
actions-status-stale = obsoleta
actions-status-unknown = desconhecida
