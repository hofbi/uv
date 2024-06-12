use std::env;
use std::io::stdout;
use std::path::PathBuf;
use std::process::ExitCode;

use anstream::eprintln;
use anyhow::Result;
use clap::error::{ContextKind, ContextValue};
use clap::{CommandFactory, Parser};
use owo_colors::OwoColorize;
use tracing::{debug, instrument};

use cli::{ToolCommand, ToolNamespace, ToolchainCommand, ToolchainNamespace};
use uv_cache::Cache;
use uv_configuration::Concurrency;
use uv_requirements::RequirementsSource;
use uv_workspace::Combine;

use crate::cli::{
    CacheCommand, CacheNamespace, Cli, Commands, PipCommand, PipNamespace, ProjectCommand,
};
#[cfg(feature = "self-update")]
use crate::cli::{SelfCommand, SelfNamespace};
use crate::commands::ExitStatus;
use crate::compat::CompatArgs;
use crate::settings::{
    CacheSettings, GlobalSettings, PipCheckSettings, PipCompileSettings, PipFreezeSettings,
    PipInstallSettings, PipListSettings, PipShowSettings, PipSyncSettings, PipUninstallSettings,
};

#[cfg(target_os = "windows")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(all(
    not(target_os = "windows"),
    not(target_os = "openbsd"),
    any(
        target_arch = "x86_64",
        target_arch = "aarch64",
        target_arch = "powerpc64"
    )
))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

mod cli;
mod commands;
mod compat;
mod logging;
mod printer;
mod settings;
mod shell;
mod version;

#[instrument]
async fn run() -> Result<ExitStatus> {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(mut err) => {
            if let Some(ContextValue::String(subcommand)) = err.get(ContextKind::InvalidSubcommand)
            {
                match subcommand.as_str() {
                    "compile" | "lock" => {
                        err.insert(
                            ContextKind::SuggestedSubcommand,
                            ContextValue::String("uv pip compile".to_string()),
                        );
                    }
                    "sync" => {
                        err.insert(
                            ContextKind::SuggestedSubcommand,
                            ContextValue::String("uv pip sync".to_string()),
                        );
                    }
                    "install" | "add" => {
                        err.insert(
                            ContextKind::SuggestedSubcommand,
                            ContextValue::String("uv pip install".to_string()),
                        );
                    }
                    "uninstall" | "remove" => {
                        err.insert(
                            ContextKind::SuggestedSubcommand,
                            ContextValue::String("uv pip uninstall".to_string()),
                        );
                    }
                    "freeze" => {
                        err.insert(
                            ContextKind::SuggestedSubcommand,
                            ContextValue::String("uv pip freeze".to_string()),
                        );
                    }
                    "list" => {
                        err.insert(
                            ContextKind::SuggestedSubcommand,
                            ContextValue::String("uv pip list".to_string()),
                        );
                    }
                    "show" => {
                        err.insert(
                            ContextKind::SuggestedSubcommand,
                            ContextValue::String("uv pip show".to_string()),
                        );
                    }
                    _ => {}
                }
            }
            err.exit()
        }
    };

    // enable flag to pick up warnings generated by workspace loading.
    if !cli.global_args.quiet {
        uv_warnings::enable();
    }

    // Load the workspace settings, prioritizing (in order):
    // 1. The configuration file specified on the command-line.
    // 2. The configuration file in the current directory.
    // 3. The user configuration file.
    let workspace = if let Some(config_file) = cli.config_file.as_ref() {
        Some(uv_workspace::Workspace::from_file(config_file)?)
    } else if cli.global_args.isolated {
        None
    } else {
        // TODO(charlie): This needs to discover settings from the workspace _root_. Right now, it
        // discovers the closest `pyproject.toml`, which could be a workspace _member_.
        let project = uv_workspace::Workspace::find(env::current_dir()?)?;
        let user = uv_workspace::Workspace::user()?;
        project.combine(user)
    };

    // Resolve the global settings.
    let globals = GlobalSettings::resolve(cli.global_args, workspace.as_ref());

    // Configure the `tracing` crate, which controls internal logging.
    #[cfg(feature = "tracing-durations-export")]
    let (duration_layer, _duration_guard) = logging::setup_duration()?;
    #[cfg(not(feature = "tracing-durations-export"))]
    let duration_layer = None::<tracing_subscriber::layer::Identity>;
    logging::setup_logging(
        match globals.verbose {
            0 => logging::Level::Default,
            1 => logging::Level::Verbose,
            2.. => logging::Level::ExtraVerbose,
        },
        duration_layer,
    )?;

    // Configure the `Printer`, which controls user-facing output in the CLI.
    let printer = if globals.quiet {
        printer::Printer::Quiet
    } else if globals.verbose > 0 {
        printer::Printer::Verbose
    } else {
        printer::Printer::Default
    };

    // Configure the `warn!` macros, which control user-facing warnings in the CLI.
    if globals.quiet {
        uv_warnings::disable();
    } else {
        uv_warnings::enable();
    }

    anstream::ColorChoice::write_global(globals.color.into());

    miette::set_hook(Box::new(|_| {
        Box::new(
            miette::MietteHandlerOpts::new()
                .break_words(false)
                .word_separator(textwrap::WordSeparator::AsciiSpace)
                .word_splitter(textwrap::WordSplitter::NoHyphenation)
                .wrap_lines(env::var("UV_NO_WRAP").map(|_| false).unwrap_or(true))
                .build(),
        )
    }))?;

    debug!("uv {}", version::version());

    // Resolve the cache settings.
    let cache = CacheSettings::resolve(cli.cache_args, workspace.as_ref());
    let cache = Cache::from_settings(cache.no_cache, cache.cache_dir)?;

    match cli.command {
        Commands::Pip(PipNamespace {
            command: PipCommand::Compile(args),
        }) => {
            args.compat_args.validate()?;

            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = PipCompileSettings::resolve(args, workspace);
            rayon::ThreadPoolBuilder::new()
                .num_threads(args.pip.concurrency.installs)
                .build_global()
                .expect("failed to initialize global rayon pool");

            // Initialize the cache.
            let cache = cache.init()?.with_refresh(args.refresh);

            let requirements = args
                .src_file
                .into_iter()
                .map(RequirementsSource::from_requirements_file)
                .collect::<Vec<_>>();
            let constraints = args
                .constraint
                .into_iter()
                .map(RequirementsSource::from_constraints_txt)
                .collect::<Vec<_>>();
            let overrides = args
                .r#override
                .into_iter()
                .map(RequirementsSource::from_overrides_txt)
                .collect::<Vec<_>>();

            commands::pip_compile(
                &requirements,
                &constraints,
                &overrides,
                args.overrides_from_workspace,
                args.pip.extras,
                args.pip.output_file.as_deref(),
                args.pip.resolution,
                args.pip.prerelease,
                args.pip.dependency_mode,
                args.upgrade,
                args.pip.generate_hashes,
                args.pip.no_emit_package,
                args.pip.no_strip_extras,
                !args.pip.no_annotate,
                !args.pip.no_header,
                args.pip.custom_compile_command,
                args.pip.emit_index_url,
                args.pip.emit_find_links,
                args.pip.emit_marker_expression,
                args.pip.emit_index_annotation,
                args.pip.index_locations,
                args.pip.index_strategy,
                args.pip.keyring_provider,
                args.pip.setup_py,
                args.pip.config_setting,
                globals.connectivity,
                args.pip.no_build_isolation,
                args.pip.no_build,
                args.pip.no_binary,
                args.pip.python_version,
                args.pip.python_platform,
                args.pip.exclude_newer,
                args.pip.annotation_style,
                args.pip.link_mode,
                args.pip.python,
                args.pip.system,
                args.pip.concurrency,
                globals.native_tls,
                globals.quiet,
                globals.preview,
                cache,
                printer,
            )
            .await
        }
        Commands::Pip(PipNamespace {
            command: PipCommand::Sync(args),
        }) => {
            args.compat_args.validate()?;

            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = PipSyncSettings::resolve(args, workspace);
            rayon::ThreadPoolBuilder::new()
                .num_threads(args.pip.concurrency.installs)
                .build_global()
                .expect("failed to initialize global rayon pool");

            // Initialize the cache.
            let cache = cache.init()?.with_refresh(args.refresh);

            let requirements = args
                .src_file
                .into_iter()
                .map(RequirementsSource::from_requirements_file)
                .collect::<Vec<_>>();
            let constraints = args
                .constraint
                .into_iter()
                .map(RequirementsSource::from_constraints_txt)
                .collect::<Vec<_>>();

            commands::pip_sync(
                &requirements,
                &constraints,
                &args.reinstall,
                args.pip.link_mode,
                args.pip.compile_bytecode,
                args.pip.require_hashes,
                args.pip.index_locations,
                args.pip.index_strategy,
                args.pip.keyring_provider,
                args.pip.setup_py,
                globals.connectivity,
                &args.pip.config_setting,
                args.pip.no_build_isolation,
                args.pip.no_build,
                args.pip.no_binary,
                args.pip.python_version,
                args.pip.python_platform,
                args.pip.strict,
                args.pip.exclude_newer,
                args.pip.python,
                args.pip.system,
                args.pip.break_system_packages,
                args.pip.target,
                args.pip.prefix,
                args.pip.concurrency,
                globals.native_tls,
                globals.preview,
                cache,
                args.dry_run,
                printer,
            )
            .await
        }
        Commands::Pip(PipNamespace {
            command: PipCommand::Install(args),
        }) => {
            args.compat_args.validate()?;

            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = PipInstallSettings::resolve(args, workspace);
            rayon::ThreadPoolBuilder::new()
                .num_threads(args.pip.concurrency.installs)
                .build_global()
                .expect("failed to initialize global rayon pool");

            // Initialize the cache.
            let cache = cache.init()?.with_refresh(args.refresh);
            let requirements = args
                .package
                .into_iter()
                .map(RequirementsSource::from_package)
                .chain(args.editable.into_iter().map(RequirementsSource::Editable))
                .chain(
                    args.requirement
                        .into_iter()
                        .map(RequirementsSource::from_requirements_file),
                )
                .collect::<Vec<_>>();
            let constraints = args
                .constraint
                .into_iter()
                .map(RequirementsSource::from_constraints_txt)
                .collect::<Vec<_>>();
            let overrides = args
                .r#override
                .into_iter()
                .map(RequirementsSource::from_overrides_txt)
                .collect::<Vec<_>>();

            commands::pip_install(
                &requirements,
                &constraints,
                &overrides,
                args.overrides_from_workspace,
                &args.pip.extras,
                args.pip.resolution,
                args.pip.prerelease,
                args.pip.dependency_mode,
                args.upgrade,
                args.pip.index_locations,
                args.pip.index_strategy,
                args.pip.keyring_provider,
                args.reinstall,
                args.pip.link_mode,
                args.pip.compile_bytecode,
                args.pip.require_hashes,
                args.pip.setup_py,
                globals.connectivity,
                &args.pip.config_setting,
                args.pip.no_build_isolation,
                args.pip.no_build,
                args.pip.no_binary,
                args.pip.python_version,
                args.pip.python_platform,
                args.pip.strict,
                args.pip.exclude_newer,
                args.pip.python,
                args.pip.system,
                args.pip.break_system_packages,
                args.pip.target,
                args.pip.prefix,
                args.pip.concurrency,
                globals.native_tls,
                globals.preview,
                cache,
                args.dry_run,
                printer,
            )
            .await
        }
        Commands::Pip(PipNamespace {
            command: PipCommand::Uninstall(args),
        }) => {
            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = PipUninstallSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?;

            let sources = args
                .package
                .into_iter()
                .map(RequirementsSource::from_package)
                .chain(
                    args.requirement
                        .into_iter()
                        .map(RequirementsSource::from_requirements_txt),
                )
                .collect::<Vec<_>>();
            commands::pip_uninstall(
                &sources,
                args.pip.python,
                args.pip.system,
                args.pip.break_system_packages,
                args.pip.target,
                args.pip.prefix,
                cache,
                globals.connectivity,
                globals.native_tls,
                globals.preview,
                args.pip.keyring_provider,
                printer,
            )
            .await
        }
        Commands::Pip(PipNamespace {
            command: PipCommand::Freeze(args),
        }) => {
            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = PipFreezeSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?;

            commands::pip_freeze(
                args.exclude_editable,
                args.pip.strict,
                args.pip.python.as_deref(),
                args.pip.system,
                globals.preview,
                &cache,
                printer,
            )
        }
        Commands::Pip(PipNamespace {
            command: PipCommand::List(args),
        }) => {
            args.compat_args.validate()?;

            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = PipListSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?;

            commands::pip_list(
                args.editable,
                args.exclude_editable,
                &args.exclude,
                &args.format,
                args.pip.strict,
                args.pip.python.as_deref(),
                args.pip.system,
                globals.preview,
                &cache,
                printer,
            )
        }
        Commands::Pip(PipNamespace {
            command: PipCommand::Show(args),
        }) => {
            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = PipShowSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?;

            commands::pip_show(
                args.package,
                args.pip.strict,
                args.pip.python.as_deref(),
                args.pip.system,
                globals.preview,
                &cache,
                printer,
            )
        }
        Commands::Pip(PipNamespace {
            command: PipCommand::Check(args),
        }) => {
            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = PipCheckSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?;

            commands::pip_check(
                args.pip.python.as_deref(),
                args.pip.system,
                globals.preview,
                &cache,
                printer,
            )
        }
        Commands::Cache(CacheNamespace {
            command: CacheCommand::Clean(args),
        })
        | Commands::Clean(args) => commands::cache_clean(&args.package, &cache, printer),
        Commands::Cache(CacheNamespace {
            command: CacheCommand::Prune,
        }) => commands::cache_prune(&cache, printer),
        Commands::Cache(CacheNamespace {
            command: CacheCommand::Dir,
        }) => {
            commands::cache_dir(&cache);
            Ok(ExitStatus::Success)
        }
        Commands::Venv(args) => {
            args.compat_args.validate()?;

            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = settings::VenvSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?;

            // Since we use ".venv" as the default name, we use "." as the default prompt.
            let prompt = args.prompt.or_else(|| {
                if args.name == PathBuf::from(".venv") {
                    Some(".".to_string())
                } else {
                    None
                }
            });

            commands::venv(
                &args.name,
                args.pip.python.as_deref(),
                args.pip.link_mode,
                &args.pip.index_locations,
                args.pip.index_strategy,
                args.pip.keyring_provider,
                uv_virtualenv::Prompt::from_args(prompt),
                args.system_site_packages,
                globals.connectivity,
                args.seed,
                args.allow_existing,
                args.pip.exclude_newer,
                globals.native_tls,
                globals.preview,
                &cache,
                printer,
            )
            .await
        }
        Commands::Project(ProjectCommand::Run(args)) => {
            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = settings::RunSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?.with_refresh(args.refresh);

            let requirements = args
                .with
                .into_iter()
                .map(RequirementsSource::from_package)
                // TODO(zanieb): Consider editable package support. What benefit do these have in an ephemeral
                //               environment?
                // .chain(
                //     args.with_editable
                //         .into_iter()
                //         .map(RequirementsSource::Editable),
                // )
                // TODO(zanieb): Consider requirements file support, this comes with additional complexity due to
                //               to the extensive configuration allowed in requirements files
                // .chain(
                //     args.with_requirements
                //         .into_iter()
                //         .map(RequirementsSource::from_requirements_file),
                // )
                .collect::<Vec<_>>();

            commands::run(
                args.extras,
                args.dev,
                args.target,
                args.args,
                requirements,
                args.python,
                args.upgrade,
                args.package,
                args.installer,
                globals.isolated,
                globals.preview,
                globals.connectivity,
                Concurrency::default(),
                globals.native_tls,
                &cache,
                printer,
            )
            .await
        }
        Commands::Project(ProjectCommand::Sync(args)) => {
            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = settings::SyncSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?.with_refresh(args.refresh);

            commands::sync(
                args.extras,
                args.dev,
                args.python,
                args.installer,
                globals.preview,
                globals.connectivity,
                Concurrency::default(),
                globals.native_tls,
                &cache,
                printer,
            )
            .await
        }
        Commands::Project(ProjectCommand::Lock(args)) => {
            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = settings::LockSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?.with_refresh(args.refresh);

            commands::lock(
                args.upgrade,
                args.python,
                args.installer,
                globals.preview,
                globals.connectivity,
                Concurrency::default(),
                globals.native_tls,
                &cache,
                printer,
            )
            .await
        }
        Commands::Project(ProjectCommand::Add(args)) => {
            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = settings::AddSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?;

            commands::add(
                args.requirements,
                args.python,
                globals.preview,
                globals.connectivity,
                Concurrency::default(),
                globals.native_tls,
                &cache,
                printer,
            )
            .await
        }
        Commands::Project(ProjectCommand::Remove(args)) => {
            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = settings::RemoveSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?;

            commands::remove(
                args.requirements,
                args.python,
                globals.preview,
                globals.connectivity,
                Concurrency::default(),
                globals.native_tls,
                &cache,
                printer,
            )
            .await
        }
        #[cfg(feature = "self-update")]
        Commands::Self_(SelfNamespace {
            command: SelfCommand::Update,
        }) => commands::self_update(printer).await,
        Commands::Version { output_format } => {
            commands::version(output_format, &mut stdout())?;
            Ok(ExitStatus::Success)
        }
        Commands::GenerateShellCompletion { shell } => {
            shell.generate(&mut Cli::command(), &mut stdout());
            Ok(ExitStatus::Success)
        }
        Commands::Tool(ToolNamespace {
            command: ToolCommand::Run(args),
        }) => {
            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = settings::ToolRunSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?;

            commands::run_tool(
                args.target,
                args.args,
                args.python,
                args.from,
                args.with,
                args.installer,
                globals.isolated,
                globals.preview,
                globals.connectivity,
                Concurrency::default(),
                globals.native_tls,
                &cache,
                printer,
            )
            .await
        }
        Commands::Toolchain(ToolchainNamespace {
            command: ToolchainCommand::List(args),
        }) => {
            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = settings::ToolchainListSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?;

            commands::toolchain_list(args.includes, globals.preview, &cache, printer).await
        }
        Commands::Toolchain(ToolchainNamespace {
            command: ToolchainCommand::Install(args),
        }) => {
            // Resolve the settings from the command-line arguments and workspace configuration.
            let args = settings::ToolchainInstallSettings::resolve(args, workspace);

            // Initialize the cache.
            let cache = cache.init()?;

            commands::toolchain_install(
                args.target,
                globals.native_tls,
                globals.connectivity,
                globals.preview,
                &cache,
                printer,
            )
            .await
        }
    }
}

fn main() -> ExitCode {
    let result = if let Ok(stack_size) = env::var("UV_STACK_SIZE") {
        // Artificially limit the stack size to test for stack overflows. Windows has a default stack size of 1MB,
        // which is lower than the linux and mac default.
        // https://learn.microsoft.com/en-us/cpp/build/reference/stack-stack-allocations?view=msvc-170
        let stack_size = stack_size.parse().expect("Invalid stack size");
        let tokio_main = move || {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .thread_stack_size(stack_size)
                .build()
                .expect("Failed building the Runtime")
                .block_on(run())
        };
        std::thread::Builder::new()
            .stack_size(stack_size)
            .spawn(tokio_main)
            .expect("Tokio executor failed, was there a panic?")
            .join()
            .expect("Tokio executor failed, was there a panic?")
    } else {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("Failed building the Runtime")
            .block_on(run())
    };

    match result {
        Ok(code) => code.into(),
        Err(err) => {
            let mut causes = err.chain();
            eprintln!("{}: {}", "error".red().bold(), causes.next().unwrap());
            for err in causes {
                eprintln!("  {}: {}", "Caused by".red().bold(), err);
            }
            ExitStatus::Error.into()
        }
    }
}
