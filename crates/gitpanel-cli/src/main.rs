use anyhow::{Context, Result};
use chrono::Local;
use clap::{Parser, Subcommand};
use gitpanel_config::Config;
use gitpanel_db::Db;
use gitpanel_i18n::{LanguagePreference, Translator, keys};

#[derive(Debug, Parser)]
#[command(
    name = "gitpanel-cli",
    version,
    about = "Consulta local dos dados sincronizados pelo GitPanel"
)]
struct Cli {
    /// Idioma: pt-BR, system
    #[arg(long, global = true)]
    lang: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Lista os repositórios no cache local
    List {
        /// Filtro por nome, dono, linguagem ou descrição
        #[arg(long)]
        search: Option<String>,
    },
    /// Mostra estatísticas de commits de um repositório
    Stats {
        /// Nome completo (owner/repo) ou apenas o nome
        repo: Option<String>,
    },
    /// Mostra informações de configuração e caminhos
    Info,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = Config::load().context("não foi possível carregar a configuração")?;
    let preference = cli
        .lang
        .as_deref()
        .map(|value| {
            if value.eq_ignore_ascii_case("system") {
                LanguagePreference::System
            } else {
                LanguagePreference::Locale(value.to_owned())
            }
        })
        .unwrap_or_else(|| config.settings.ui.language.clone());
    let translator = Translator::new(&preference).context("não foi possível carregar os textos")?;
    let db =
        Db::open(config.paths.database_file.clone()).context("não foi possível abrir o banco")?;

    match cli.command.unwrap_or(Command::List { search: None }) {
        Command::List { search } => list(&db, &translator, search.as_deref())?,
        Command::Stats { repo } => stats(&db, &translator, repo.as_deref())?,
        Command::Info => info(&config, &translator),
    }
    Ok(())
}

fn list(db: &Db, translator: &Translator, search: Option<&str>) -> Result<()> {
    let query = search.unwrap_or_default().to_ascii_lowercase();
    let repositories = db.list_repositories()?;
    let filtered: Vec<_> = repositories
        .into_iter()
        .filter(|repository| {
            query.is_empty()
                || format!(
                    "{} {} {} {}",
                    repository.full_name,
                    repository.description.as_deref().unwrap_or_default(),
                    repository.language.as_deref().unwrap_or_default(),
                    repository.owner
                )
                .to_ascii_lowercase()
                .contains(&query)
        })
        .collect();
    if filtered.is_empty() {
        println!("{}", translator.text(keys::REPOS_EMPTY_TITLE));
        return Ok(());
    }
    println!(
        "{:<48} {:<10} {:<14} {:>6}  {}",
        translator.text(keys::COMMON_REPOSITORIES),
        translator.text(keys::COMMON_VISIBILITY),
        translator.text(keys::COMMON_LANGUAGE),
        translator.text(keys::COMMON_STARS),
        translator.text(keys::COMMON_STATUS)
    );
    for repository in filtered {
        let visibility = if repository.private {
            translator.text(keys::COMMON_PRIVATE)
        } else {
            translator.text(keys::COMMON_PUBLIC)
        };
        let status = if repository.last_sync_error.is_some() {
            translator.text(keys::REPOS_SYNC_STATUS_ERROR)
        } else if repository.synced_at.is_some() {
            translator.text(keys::REPOS_SYNC_STATUS_SYNCED)
        } else {
            translator.text(keys::REPOS_SYNC_STATUS_PENDING)
        };
        println!(
            "{:<48} {:<10} {:<14} {:>6}  {}",
            repository.full_name,
            visibility,
            repository.language.unwrap_or_else(|| "—".to_owned()),
            repository.stars,
            status
        );
    }
    Ok(())
}

fn stats(db: &Db, translator: &Translator, repo: Option<&str>) -> Result<()> {
    let query = repo.unwrap_or_default().to_ascii_lowercase();
    let repositories = db.list_repositories()?;
    let selected: Vec<_> = repositories
        .into_iter()
        .filter(|repository| {
            query.is_empty()
                || repository.full_name.to_ascii_lowercase() == query
                || repository.name.to_ascii_lowercase() == query
                || repository.full_name.to_ascii_lowercase().contains(&query)
        })
        .collect();
    if selected.is_empty() {
        println!("{}", translator.text(keys::ERROR_NOT_FOUND));
        return Ok(());
    }
    for repository in selected {
        let (commits, additions, deletions) = db.commit_summary(repository.github_id)?;
        println!("{}", repository.full_name);
        println!(
            "  {}: {}",
            translator.text(keys::CHART_TOTAL_COMMITS),
            commits
        );
        println!("  {}: {additions}", translator.text(keys::CHART_ADDITIONS));
        println!("  {}: {deletions}", translator.text(keys::CHART_DELETIONS));
        if let Some(synced_at) = repository.synced_at {
            println!(
                "  {}: {}",
                translator.text(keys::REPOS_DETAIL_SYNCED_AT),
                synced_at
                    .with_timezone(&Local)
                    .format(translator.locale().date_time())
            );
        }
        let commits = db.list_all_commits(repository.github_id)?;
        let mut authors: std::collections::BTreeMap<String, u32> =
            std::collections::BTreeMap::new();
        for commit in commits {
            *authors
                .entry(commit.author_display().to_owned())
                .or_default() += 1;
        }
        if !authors.is_empty() {
            println!("  {}:", translator.text(keys::CHART_TOP_CONTRIBUTORS));
            let mut authors: Vec<_> = authors.into_iter().collect();
            authors.sort_by_key(|entry| std::cmp::Reverse(entry.1));
            for (author, count) in authors.into_iter().take(5) {
                println!("    {author}: {count}");
            }
        }
    }
    Ok(())
}

fn info(config: &Config, translator: &Translator) {
    println!(
        "{} {}",
        translator.text(keys::APP_NAME),
        env!("CARGO_PKG_VERSION")
    );
    println!(
        "{}: {}",
        translator.text(keys::SETTINGS_CONFIG_FILE),
        config.paths.config_file.display()
    );
    println!(
        "{}: {}",
        translator.text(keys::SETTINGS_DATA_DIR),
        config.paths.data_dir.display()
    );
    println!(
        "{}: {}",
        translator.text(keys::SETTINGS_CACHE_DIR),
        config.paths.cache_dir.display()
    );
}
