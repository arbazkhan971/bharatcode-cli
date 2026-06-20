use crate::cli::StreamableHttpOptions;

use super::output;
use super::CliSession;
use console::style;
use goose::agents::{Agent, Container, ExtensionError};
use goose::config::resolve_extensions_for_new_session;
use goose::config::{Config, ExtensionConfig, GooseMode};
use goose::model_config::model_config_from_user_config;
use goose::providers::create;
use goose::recipe::Recipe;
use goose::session::session_manager::SessionType;
use goose::session::EnabledExtensionsState;
use rustyline::EditMode;
use std::collections::BTreeSet;
use std::process;
use std::sync::Arc;
use tokio::task::JoinSet;

// `session/mod.rs` is a contended shared file in this wave, so the opt-in
// framework-migration advisory module is declared here, from builder.rs (the
// file that wires it into the session build path), via an explicit `#[path]`.
#[path = "migrate.rs"]
mod migrate;

// Declared here (not in the contended `session/mod.rs`) for the same reason as
// `migrate` above: builder.rs is the file that wires the startup session-DB
// integrity quick-check into the session build path.
#[path = "db_preflight.rs"]
mod db_preflight;

// Opt-in recipe import/export round-trip hardening lives in an owned crate-root
// module. It is wired in from builder.rs (the session-build path) via an
// explicit `#[path]`, avoiding edits to `cli.rs`/`lib.rs`.
#[path = "../recipe_lock.rs"]
mod recipe_lock;

// Opt-in deeper-git-context injection (BHARATCODE_GIT_CONTEXT). Lives next to
// `git_helper.rs` under `commands/`, but `commands/mod.rs` is owned by a sibling
// in this wave, so it is declared here (the file that wires it into the session
// build path) via an explicit `#[path]`.
#[path = "../commands/git_context.rs"]
mod git_context;

// Embedded quick-start tutorials and the first-run nudge. Lives under
// `commands/`, but `commands/mod.rs` is owned by a sibling in this wave, so it
// is declared here (the file that wires the nudge into the session build path)
// via an explicit `#[path]`.
#[path = "../commands/tutorials.rs"]
mod tutorials;

const EXTENSION_HINT_MAX_LEN: usize = 5;

fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
    let truncated: String = s.chars().take(max_len).collect();
    if s.chars().count() > max_len {
        format!("{}…", truncated)
    } else {
        truncated
    }
}

fn parse_cli_flag_extensions(
    extensions: &[String],
    streamable_http_extensions: &[StreamableHttpOptions],
    builtins: &[String],
) -> Vec<(String, ExtensionConfig)> {
    let mut extensions_to_load = Vec::new();

    for (idx, ext_str) in extensions.iter().enumerate() {
        match CliSession::parse_stdio_extension(ext_str) {
            Ok(config) => {
                let hint = truncate_with_ellipsis(ext_str, EXTENSION_HINT_MAX_LEN);
                let label = format!("stdio #{}({})", idx + 1, hint);
                extensions_to_load.push((label, config));
            }
            Err(e) => {
                eprintln!(
                    "{}",
                    style(format!(
                        "Warning: Invalid --extension value '{}' ({}); ignoring",
                        ext_str, e
                    ))
                    .yellow()
                );
            }
        }
    }

    for (idx, opts) in streamable_http_extensions.iter().enumerate() {
        let config = CliSession::parse_streamable_http_extension(&opts.url, opts.timeout);
        let hint = truncate_with_ellipsis(&opts.url, EXTENSION_HINT_MAX_LEN);
        let label = format!("http #{}({})", idx + 1, hint);
        extensions_to_load.push((label, config));
    }

    for builtin_str in builtins {
        let configs = CliSession::parse_builtin_extensions(builtin_str);
        for config in configs {
            extensions_to_load.push((config.name(), config));
        }
    }

    extensions_to_load
}

/// Configuration for building a new BharatCode session
///
/// This struct contains all the parameters needed to create a new session,
/// including session identification, extension configuration, and debug settings.
#[derive(Clone, Debug)]
pub struct SessionBuilderConfig {
    /// Session id, optional need to deduce from context
    pub session_id: Option<String>,
    /// Whether to resume an existing session
    pub resume: bool,
    /// Whether to fork an existing session (creates a copy of the original/existing session then resumes the copy)
    pub fork: bool,
    /// Whether to run without a session file
    pub no_session: bool,
    /// List of stdio extension commands to add
    pub extensions: Vec<String>,
    /// List of streamable HTTP extension commands to add
    pub streamable_http_extensions: Vec<StreamableHttpOptions>,
    /// List of builtin extension commands to add
    pub builtins: Vec<String>,
    pub no_profile: bool,
    /// Recipe for the session
    pub recipe: Option<Recipe>,
    /// Any additional system prompt to append to the default
    pub additional_system_prompt: Option<String>,
    /// Provider override from CLI arguments
    pub provider: Option<String>,
    /// Model override from CLI arguments
    pub model: Option<String>,
    /// Enable debug printing
    pub debug: bool,
    /// Maximum number of consecutive identical tool calls allowed
    pub max_tool_repetitions: Option<u32>,
    /// Maximum number of turns (iterations) allowed without user input
    pub max_turns: Option<u32>,
    /// ID of the scheduled job that triggered this session (if any)
    pub scheduled_job_id: Option<String>,
    /// Whether this session will be used interactively (affects debugging prompts)
    pub interactive: bool,
    /// Quiet mode - suppress non-response output
    pub quiet: bool,
    /// Output format (text, json)
    pub output_format: String,
    /// Docker container to run stdio extensions inside
    pub container: Option<Container>,
    /// Print generation statistics after headless runs.
    pub stats: bool,
}

/// Manual implementation of Default to ensure proper initialization of output_format
/// This struct requires explicit default value for output_format field
impl Default for SessionBuilderConfig {
    fn default() -> Self {
        SessionBuilderConfig {
            session_id: None,
            resume: false,
            fork: false,
            no_session: false,
            extensions: Vec::new(),
            streamable_http_extensions: Vec::new(),
            builtins: Vec::new(),
            no_profile: false,
            recipe: None,
            additional_system_prompt: None,
            provider: None,
            model: None,
            debug: false,
            max_tool_repetitions: None,
            max_turns: None,
            scheduled_job_id: None,
            interactive: false,
            quiet: false,
            output_format: "text".to_string(),
            container: None,
            stats: false,
        }
    }
}

async fn load_extensions(
    agent: Agent,
    extensions_to_load: Vec<(String, ExtensionConfig)>,
    session_id: &str,
) -> Arc<Agent> {
    let mut set = JoinSet::new();
    let agent_ptr = Arc::new(agent);

    let mut waiting_ids: BTreeSet<usize> = (0..extensions_to_load.len()).collect();
    for (id, (_label, extension)) in extensions_to_load.iter().enumerate() {
        let agent_ptr = agent_ptr.clone();
        let cfg = extension.clone();
        let sid = session_id.to_string();
        set.spawn(async move { (id, agent_ptr.add_extension(cfg, &sid).await) });
    }

    let get_message = |waiting_ids: &BTreeSet<usize>| {
        let labels: Vec<String> = waiting_ids
            .iter()
            .map(|id| {
                extensions_to_load
                    .get(*id)
                    .map(|e| e.0.clone())
                    .unwrap_or_default()
            })
            .collect();
        format!(
            "starting {} extensions: {}",
            waiting_ids.len(),
            labels.join(", ")
        )
    };

    let spinner = cliclack::spinner();
    spinner.start(get_message(&waiting_ids));

    let mut failed: Vec<(usize, anyhow::Error)> = Vec::new();
    while let Some(result) = set.join_next().await {
        match result {
            Ok((id, Ok(_))) => {
                waiting_ids.remove(&id);
                spinner.set_message(get_message(&waiting_ids));
            }
            Ok((id, Err(e))) => failed.push((id, e.into())),
            Err(e) => tracing::error!("failed to add extension: {}", e),
        }
    }

    spinner.clear();

    for (id, err) in failed {
        let label = extensions_to_load
            .get(id)
            .map(|e| e.0.clone())
            .unwrap_or_default();
        eprintln!(
            "{}",
            style(format!(
                "Warning: Failed to start extension '{}' ({}), continuing without it",
                label, err
            ))
            .yellow()
        );
        eprintln!(
            "{}",
            style(format!(
                "  Hint: once the session starts, ask bharatcode to help debug the '{}' extension",
                label
            ))
            .dim()
        );
    }

    agent_ptr
}

struct ResolvedProviderConfig {
    provider_name: String,
    model_name: String,
    model_config: goose_providers::model::ModelConfig,
}

fn resolve_provider_and_model(
    session_config: &SessionBuilderConfig,
    config: &Config,
    saved_provider: Option<String>,
    saved_model_config: Option<goose_providers::model::ModelConfig>,
) -> ResolvedProviderConfig {
    let recipe_settings = session_config
        .recipe
        .as_ref()
        .and_then(|r| r.settings.as_ref());

    let provider_name = session_config
        .provider
        .clone()
        .or(saved_provider)
        .or_else(|| recipe_settings.and_then(|s| s.goose_provider.clone()))
        .or_else(|| config.get_bharatcode_provider().ok())
        .unwrap_or_else(|| {
            output::render_error(&crate::tr!("error.no_provider"));
            process::exit(1);
        });

    let model_name = session_config
        .model
        .clone()
        .or_else(|| saved_model_config.as_ref().map(|mc| mc.model_name.clone()))
        .or_else(|| recipe_settings.and_then(|s| s.goose_model.clone()))
        .or_else(|| config.get_bharatcode_model().ok())
        .unwrap_or_else(|| {
            output::render_error("No model configured. Run 'bharatcode configure' first.");
            process::exit(1);
        });

    let model_config = if session_config.resume
        && saved_model_config
            .as_ref()
            .is_some_and(|mc| mc.model_name == model_name)
    {
        let mut config = saved_model_config.unwrap();
        config.normalize_effort_suffix();
        if let Some(temp) = recipe_settings.and_then(|s| s.temperature) {
            config = config.with_temperature(Some(temp));
        }
        config
    } else {
        let mut config =
            goose::model_config::model_config_from_user_config(&provider_name, &model_name)
                .unwrap_or_else(|e| {
                    output::render_error(&format!("Failed to create model configuration: {}", e));
                    process::exit(1);
                });
        if let Some(temp) = recipe_settings.and_then(|s| s.temperature) {
            config = config.with_temperature(Some(temp));
        }
        config
    };

    ResolvedProviderConfig {
        provider_name,
        model_name,
        model_config,
    }
}

async fn resolve_session_id(
    session_config: &SessionBuilderConfig,
    session_manager: &goose::session::session_manager::SessionManager,
    goose_mode: GooseMode,
) -> String {
    if session_config.no_session {
        let working_dir = std::env::current_dir().unwrap_or_else(|e| {
            output::render_error(&format!("Could not get working directory: {}", e));
            process::exit(1);
        });
        let session = session_manager
            .create_session(
                working_dir,
                "CLI Session".to_string(),
                SessionType::Hidden,
                goose_mode,
            )
            .await
            .unwrap_or_else(|e| {
                output::render_error(&format!("Could not create session: {}", e));
                process::exit(1);
            });
        session.id
    } else if session_config.resume {
        if let Some(ref session_id) = session_config.session_id {
            match session_manager.get_session(session_id, false).await {
                Ok(_) => session_id.clone(),
                Err(_) => {
                    output::render_error(&format!(
                        "Cannot resume session {} - no such session exists",
                        style(session_id).cyan()
                    ));
                    process::exit(1);
                }
            }
        } else {
            match session_manager.list_sessions().await {
                Ok(sessions) if !sessions.is_empty() => sessions[0].id.clone(),
                _ => {
                    output::render_error("Cannot resume - no previous sessions found");
                    process::exit(1);
                }
            }
        }
    } else {
        session_config.session_id.clone().unwrap()
    }
}

async fn handle_resumed_session_workdir(agent: &Agent, session_id: &str, interactive: bool) {
    let session = agent
        .config
        .session_manager
        .get_session(session_id, false)
        .await
        .unwrap_or_else(|e| {
            output::render_error(&format!("Failed to read session metadata: {}", e));
            process::exit(1);
        });

    let current_workdir = std::env::current_dir().unwrap_or_else(|e| {
        output::render_error(&format!("Failed to get current working directory: {}", e));
        process::exit(1);
    });
    if current_workdir == session.working_dir {
        return;
    }

    if interactive {
        let change_workdir = cliclack::confirm(format!(
            "{} The original working directory of this session was set to {}. \
             Your current directory is {}. \
             Do you want to switch back to the original working directory?",
            style("WARNING:").yellow(),
            style(session.working_dir.display()).cyan(),
            style(current_workdir.display()).cyan(),
        ))
        .initial_value(true)
        .interact()
        .unwrap_or_else(|e| {
            output::render_error(&format!("Failed to get user input: {}", e));
            process::exit(1);
        });

        if change_workdir {
            if !session.working_dir.exists() {
                output::render_error(&format!(
                    "Cannot switch to original working directory - {} no longer exists",
                    style(session.working_dir.display()).cyan()
                ));
            } else if let Err(e) = std::env::set_current_dir(&session.working_dir) {
                output::render_error(&format!(
                    "Failed to switch to original working directory: {}",
                    e
                ));
            }
        }
    } else {
        eprintln!(
            "{}",
            style(format!(
                "Warning: Working directory differs from session (current: {}, session: {}). \
                 Staying in current directory.",
                current_workdir.display(),
                session.working_dir.display()
            ))
            .yellow()
        );
    }
}

async fn collect_extension_configs(
    agent: &Agent,
    session_config: &SessionBuilderConfig,
    recipe: Option<&Recipe>,
    session_id: &str,
) -> Result<Vec<ExtensionConfig>, ExtensionError> {
    let recipe_extensions = recipe.and_then(|r| r.extensions.as_deref());
    let configured_extensions: Vec<ExtensionConfig> = if session_config.resume {
        EnabledExtensionsState::for_session(
            &agent.config.session_manager,
            session_id,
            Config::global(),
        )
        .await
    } else if session_config.no_profile {
        Vec::new()
    } else {
        resolve_extensions_for_new_session(recipe_extensions, None)
    };

    let cli_flag_extensions = parse_cli_flag_extensions(
        &session_config.extensions,
        &session_config.streamable_http_extensions,
        &session_config.builtins,
    );

    let mut all: Vec<ExtensionConfig> = configured_extensions;
    if !session_config.no_profile && !session_config.resume && recipe_extensions.is_none() {
        let project_root = std::env::current_dir().ok();
        all.extend(goose::plugins::mcp_servers::enabled_plugin_mcp_servers(
            project_root.as_deref(),
        ));
    }
    all.extend(cli_flag_extensions.into_iter().map(|(_, cfg)| cfg));

    Ok(all)
}

async fn resolve_and_load_extensions(
    agent: Agent,
    extensions: Vec<ExtensionConfig>,
    session_id: &str,
) -> Arc<Agent> {
    for warning in goose::config::get_warnings() {
        eprintln!("{}", style(format!("Warning: {}", warning)).yellow());
    }

    let extensions_to_load: Vec<(String, ExtensionConfig)> = extensions
        .into_iter()
        .map(|cfg| (cfg.name(), cfg))
        .collect();

    load_extensions(agent, extensions_to_load, session_id).await
}

async fn configure_session_prompts(
    session: &CliSession,
    config: &Config,
    session_config: &SessionBuilderConfig,
    session_id: &str,
) {
    if let Err(e) = session.agent.persist_extension_state(session_id).await {
        tracing::warn!("Failed to save extension state: {}", e);
    }

    if let Some(ref additional_prompt) = session_config.additional_system_prompt {
        session
            .agent
            .extend_system_prompt("additional".to_string(), additional_prompt.clone())
            .await;
    }

    let system_prompt_file: Option<String> =
        config.get_param("BHARATCODE_SYSTEM_PROMPT_FILE_PATH").ok();
    if let Some(ref path) = system_prompt_file {
        let override_prompt = std::fs::read_to_string(path).unwrap_or_else(|e| {
            output::render_error(&format!(
                "Failed to read system prompt file '{}': {}",
                path, e
            ));
            process::exit(1);
        });
        session.agent.override_system_prompt(override_prompt).await;
    }
}

pub async fn build_session(session_config: SessionBuilderConfig) -> CliSession {
    #[cfg(feature = "telemetry")]
    goose::posthog::set_session_context("cli", session_config.resume);

    // Direct, opt-in access to the embedded quick-start tutorials without
    // starting a session: `BHARATCODE_TUTORIAL=list` prints the index, and
    // `BHARATCODE_TUTORIAL=<id>` prints a single guide. This keeps the tutorials
    // reachable in the running binary; default (unset) behaviour is unchanged.
    if let Ok(arg) = std::env::var("BHARATCODE_TUTORIAL") {
        let arg = arg.trim();
        if !arg.is_empty() {
            match arg {
                "list" => println!("{}", tutorials::list()),
                id => match tutorials::show(id) {
                    Some(body) => println!("{body}"),
                    None => {
                        eprintln!("{}", tutorials::list());
                        process::exit(1);
                    }
                },
            }
            process::exit(0);
        }
    }

    // Best-effort, non-blocking physical-integrity quick-check of the session
    // database before the agent/session is constructed. A healthy DB or a check
    // that cannot run is silent; corruption warns (and, with
    // BHARATCODE_DB_PREFLIGHT set, points the user at `bharatcode db --vacuum`).
    let _ = db_preflight::preflight().await;

    let config = Config::global();
    let agent: Agent = Agent::new();

    if session_config.container.is_some() {
        agent.set_container(session_config.container.clone()).await;
    }

    let session_manager = agent.config.session_manager.clone();

    let (saved_provider, saved_model_config) = if session_config.resume {
        if let Some(ref session_id) = session_config.session_id {
            match session_manager.get_session(session_id, false).await {
                Ok(session_data) => (session_data.provider_name, session_data.model_config),
                Err(_) => (None, None),
            }
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    let resolved =
        resolve_provider_and_model(&session_config, config, saved_provider, saved_model_config);

    let recipe = session_config.recipe.as_ref();

    agent
        .apply_recipe_components(recipe.and_then(|r| r.response.clone()), true)
        .await;

    let session_id =
        resolve_session_id(&session_config, &session_manager, agent.config.goose_mode).await;

    if session_config.resume {
        handle_resumed_session_workdir(&agent, &session_id, session_config.interactive).await;
    }

    let extensions_for_provider =
        match collect_extension_configs(&agent, &session_config, recipe, &session_id).await {
            Ok(exts) => exts,
            Err(e) => {
                output::render_error(&format!("Failed to collect extensions: {}", e));
                process::exit(1);
            }
        };

    let (new_provider, effective_provider_name, effective_model_name) = match create(
        &resolved.provider_name,
        resolved.model_config.clone(),
        extensions_for_provider.clone(),
    )
    .await
    {
        Ok(provider) => (
            provider,
            resolved.provider_name.clone(),
            resolved.model_name.clone(),
        ),
        Err(e)
            if session_config.resume
                && session_config.provider.is_none()
                && is_provider_unavailable_error(&e) =>
        {
            let fallback_provider = config.get_bharatcode_provider().unwrap_or_else(|_| {
                output::render_error(&crate::tr!("error.no_provider"));
                process::exit(1);
            });
            let fallback_model = config.get_bharatcode_model().unwrap_or_else(|_| {
                output::render_error(&crate::tr!("error.no_model"));
                process::exit(1);
            });
            eprintln!(
                "{}",
                style(format!(
                    "Warning: Could not create the session's original provider '{}' ({}). \
                    Falling back to the default provider '{}'.",
                    resolved.provider_name, e, fallback_provider
                ))
                .yellow()
            );
            let fallback_model_config =
                model_config_from_user_config(fallback_provider.as_str(), &fallback_model)
                    .unwrap_or_else(|e| {
                        output::render_error(&format!(
                            "Failed to create model configuration: {}",
                            e
                        ));
                        process::exit(1);
                    });
            match create(
                &fallback_provider,
                fallback_model_config,
                extensions_for_provider.clone(),
            )
            .await
            {
                Ok(provider) => (provider, fallback_provider, fallback_model),
                Err(e2) => {
                    output::render_error(&format!(
                        "Error {}.\n\
                        Please check your system keychain and run 'bharatcode configure' again.\n\
                        If your system is unable to use the keyring, please try setting secret key(s) via environment variables.\n\
                        For more info, see: https://bharatcode-docs.ai/docs/troubleshooting/#keychainkeyring-errors",
                        e2
                    ));
                    process::exit(1);
                }
            }
        }
        Err(e) => {
            output::render_error(&format!(
                "Error {}.\n\
                Please check your system keychain and run 'bharatcode configure' again.\n\
                If your system is unable to use the keyring, please try setting secret key(s) via environment variables.\n\
                For more info, see: https://bharatcode-docs.ai/docs/troubleshooting/#keychainkeyring-errors",
                e
            ));
            process::exit(1);
        }
    };
    tracing::info!("🤖 Using model: {}", effective_model_name);

    agent
        .update_provider(new_provider, &session_id)
        .await
        .unwrap_or_else(|e| {
            output::render_error(&format!("Failed to initialize agent: {}", e));
            process::exit(1);
        });

    agent
        .update_goose_mode(agent.config.goose_mode, &session_id)
        .await
        .unwrap_or_else(|e| {
            output::render_error(&format!("Failed to set session mode: {}", e));
            process::exit(1);
        });

    if let Some(recipe) = session_config.recipe.clone() {
        if let Err(e) = session_manager
            .update(&session_id)
            .recipe(Some(recipe))
            .apply()
            .await
        {
            tracing::warn!("Failed to store recipe on session: {}", e);
        }
    }

    // Extensions are loaded after session creation because we may change directory when resuming
    let agent_ptr = resolve_and_load_extensions(agent, extensions_for_provider, &session_id).await;

    // Opt-in framework-migration advisory: when BHARATCODE_MIGRATE=<from>:<to>
    // is set, inject a compact migration-strategy block so the agent plans the
    // migration consistently. Default (unset) leaves the prompt unchanged.
    if let Some(spec) = migrate::from_env() {
        agent_ptr
            .extend_system_prompt(
                "bharatcode_migration".to_string(),
                migrate::advisory_block(&spec),
            )
            .await;
    }

    // Opt-in deeper git awareness: when BHARATCODE_GIT_CONTEXT is set, inject a
    // compact, read-only `# Git context` block (worktrees, branch/upstream
    // ahead-behind, and recent blame ownership of changed files). Default
    // (unset) leaves the prompt byte-identical and runs no git subprocess.
    if git_context::is_enabled() {
        let cwd = git_context::current_dir();
        if let Some(block) = git_context::git_context_block(&git_context::collect(&cwd)) {
            agent_ptr
                .extend_system_prompt("bharatcode_git_context".to_string(), block)
                .await;
        }
    }

    // Opt-in recipe round-trip hardening: when BHARATCODE_RECIPE_LOCK points at a
    // recipe file, canonicalize and hash it into a `.bharatcode/recipe.lock`
    // sidecar so shared/imported recipes are reproducible, warning on drift.
    // Default (unset) is a no-op.
    if recipe_lock::is_enabled() {
        if let Some(lock_path) = recipe_lock::recipe_path() {
            match recipe_lock::lock_recipe(&lock_path) {
                Ok(outcome) => tracing::info!(?outcome, "recipe lock"),
                Err(e) => tracing::warn!(%e, "recipe lock failed"),
            }
        }
    }

    // First-run nudge: on the very first run (no session database yet), print a
    // single localized line pointing new users at `bharatcode tutorials`.
    // Suppressible via BHARATCODE_NO_NUDGE; silent for every established user.
    if !session_config.quiet {
        if let Some(nudge) = tutorials::first_run_nudge() {
            println!("{}", crate::theme::muted(nudge));
        }
    }

    let edit_mode = config
        .get_param::<String>("EDIT_MODE")
        .ok()
        .and_then(|edit_mode| match edit_mode.to_lowercase().as_str() {
            "emacs" => Some(EditMode::Emacs),
            "vi" => Some(EditMode::Vi),
            _ => {
                eprintln!("Invalid EDIT_MODE specified, defaulting to Emacs");
                None
            }
        });

    let keybindings = crate::keybindings::Keybindings::from_config(config);
    tracing::debug!(?keybindings, "Loaded interactive keybindings");

    let subagent_settings = crate::subagent_settings::SubagentSettings::from_config(config);
    tracing::debug!(?subagent_settings, "Loaded subagent settings");

    let debug_mode = session_config.debug || config.get_param("BHARATCODE_DEBUG").unwrap_or(false);

    let session = CliSession::new(
        Arc::try_unwrap(agent_ptr).unwrap_or_else(|_| panic!("There should be no more references")),
        session_id.clone(),
        debug_mode,
        session_config.scheduled_job_id.clone(),
        session_config.max_turns,
        edit_mode,
        keybindings,
        recipe.and_then(|r| r.retry.clone()),
        session_config.output_format.clone(),
        session_config.stats,
    )
    .await;

    configure_session_prompts(&session, config, &session_config, &session_id).await;

    if !session_config.quiet {
        output::display_session_info(
            session_config.resume,
            &effective_provider_name,
            &effective_model_name,
            &Some(session_id),
        );
    }
    session
}

fn is_provider_unavailable_error(e: &anyhow::Error) -> bool {
    let msg = e.to_string();
    msg.contains("is not set")
        || msg.contains("not configured")
        || msg.contains("Configuration value not found")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_builder_config_creation() {
        let config = SessionBuilderConfig {
            session_id: None,
            resume: false,
            fork: false,
            no_session: false,
            extensions: vec!["echo test".to_string()],
            streamable_http_extensions: vec![StreamableHttpOptions {
                url: "http://localhost:8080/mcp".to_string(),
                timeout: goose::config::DEFAULT_EXTENSION_TIMEOUT,
            }],
            builtins: vec!["developer".to_string()],
            no_profile: false,
            recipe: None,
            additional_system_prompt: Some("Test prompt".to_string()),
            provider: None,
            model: None,
            debug: true,
            max_tool_repetitions: Some(5),
            max_turns: None,
            scheduled_job_id: None,
            interactive: true,
            quiet: false,
            output_format: "text".to_string(),
            container: None,
            stats: false,
        };

        assert_eq!(config.extensions.len(), 1);
        assert_eq!(config.streamable_http_extensions.len(), 1);
        assert_eq!(config.builtins.len(), 1);
        assert!(config.debug);
        assert_eq!(config.max_tool_repetitions, Some(5));
        assert!(config.max_turns.is_none());
        assert!(config.scheduled_job_id.is_none());
        assert!(config.interactive);
        assert!(!config.quiet);
    }

    #[test]
    fn test_session_builder_config_default() {
        let config = SessionBuilderConfig::default();

        assert!(config.session_id.is_none());
        assert!(!config.resume);
        assert!(!config.no_session);
        assert!(config.extensions.is_empty());
        assert!(config.streamable_http_extensions.is_empty());
        assert!(config.builtins.is_empty());
        assert!(!config.no_profile);
        assert!(config.recipe.is_none());
        assert!(config.additional_system_prompt.is_none());
        assert!(!config.debug);
        assert!(config.max_tool_repetitions.is_none());
        assert!(config.max_turns.is_none());
        assert!(config.scheduled_job_id.is_none());
        assert!(!config.interactive);
        assert!(!config.quiet);
        assert!(!config.fork);
    }

    #[test]
    fn test_truncate_with_ellipsis() {
        assert_eq!(truncate_with_ellipsis("abc", 5), "abc");

        assert_eq!(truncate_with_ellipsis("abcde", 5), "abcde");

        assert_eq!(truncate_with_ellipsis("abcdef", 5), "abcde…");
        assert_eq!(truncate_with_ellipsis("hello world", 5), "hello…");

        assert_eq!(truncate_with_ellipsis("", 5), "");
    }
}
