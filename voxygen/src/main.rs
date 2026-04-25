#![deny(unsafe_code)]
#![recursion_limit = "2048"]
#![cfg_attr(
    all(target_os = "windows", not(feature = "singleplayer")),
    windows_subsystem = "windows"
)]

#[cfg(all(
    target_os = "windows",
    not(feature = "tracy-memory"),
    not(feature = "hot-egui"),
    not(feature = "hot-anim"),
))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// Allow profiling allocations with Tracy
#[cfg_attr(feature = "tracy-memory", global_allocator)]
#[cfg(feature = "tracy-memory")]
static GLOBAL: common_base::tracy_client::ProfiledAllocator<std::alloc::System> =
    common_base::tracy_client::ProfiledAllocator::new(std::alloc::System, 128);

use i18n::{self, LocalizationHandle};
#[cfg(feature = "singleplayer")]
use veloren_voxygen::singleplayer::SingleplayerState;
use veloren_voxygen::{
    GlobalState,
    audio::AudioFrontend,
    cli,
    entry::EntryPolicy,
    panic_handler,
    profile::Profile,
    run,
    scene::terrain::SpriteRenderContext,
    settings::{AudioOutput, DEFAULT_LANGUAGE, Settings, get_fps},
    window::Window,
};

use chrono::Utc;
use common::clock::Clock;
use std::{panic, path::PathBuf};
use tracing::{info, warn};
#[cfg(feature = "egui-ui")]
use veloren_voxygen::ui::egui::EguiState;
use wgpu::{Backends, Instance};

fn main() {
    // Process CLI arguments
    use clap::Parser;
    let args = cli::Args::parse();

    if let Some(command) = args.command {
        match command {
            cli::Commands::ListWgpuBackends => {
                #[cfg(target_os = "windows")]
                let backends = &["opengl", "dx12", "vulkan"];
                #[cfg(not(any(target_os = "windows", target_os = "macos")))]
                let backends = &["opengl", "vulkan"];
                #[cfg(target_os = "macos")]
                let backends = &["metal"];

                for backend in backends {
                    println!("{backend}");
                }
                return;
            },
            cli::Commands::ListWgpuDevices => {
                let adapters = Instance::new(&wgpu::InstanceDescriptor::from_env_or_default())
                    .enumerate_adapters(Backends::default());
                for adapter in adapters {
                    println!("{}", adapter.get_info().name);
                }
                return;
            },
        }
    }

    #[cfg(feature = "tracy")]
    common_base::tracy_client::Client::start();

    let userdata_dir = common_base::userdata_dir();

    // Determine where Voxygen's logs should go
    // Choose a path to store the logs by the following order:
    //  - The VOXYGEN_LOGS environment variable
    //  - The <userdata>/voxygen/logs
    let logs_dir = std::env::var_os("VOXYGEN_LOGS")
        .map(PathBuf::from)
        .unwrap_or_else(|| userdata_dir.join("voxygen").join("logs"));

    // Init logging and hold the guards.
    let now = Utc::now();
    let log_filename = format!("{}_voxygen.log", now.format("%Y-%m-%d"));
    let _guards = common_frontend::init_stdout(Some((&logs_dir, &log_filename)));

    // Re-run userdata selection so any warnings will be logged
    common_base::userdata_dir();

    info!("Using userdata dir at: {}", userdata_dir.display());

    // Determine Voxygen's config directory either by env var or placed in veloren's
    // userdata folder
    let config_dir = std::env::var_os("VOXYGEN_CONFIG")
        .map(PathBuf::from)
        .and_then(|path| {
            if path.exists() {
                Some(path)
            } else {
                warn!(?path, "VOXYGEN_CONFIG points to invalid path.");
                None
            }
        })
        .unwrap_or_else(|| userdata_dir.join("voxygen"));
    info!("Using config dir at: {}", config_dir.display());

    // Load the settings
    let mut settings = Settings::load(&config_dir);
    settings.display_warnings();

    let entry_policy = EntryPolicy::load(&args);
    let entry_upgrade_contract = entry_policy.upgrade_boundary_contract();
    let public_auth_handoff_contract = entry_policy.public_official_auth_handoff_contract();
    info!(
        product_mode = ?entry_policy.product_mode(),
        official_source_kind = ?entry_upgrade_contract.official_source_kind,
        launcher_may_replace_official_source =
            entry_upgrade_contract.launcher_may_replace_official_source,
        launcher_must_not_override_runtime_policy =
            entry_upgrade_contract.launcher_must_not_override_runtime_policy,
        lightweight_discovery_remains_optional_observability_plane =
            entry_upgrade_contract
                .lightweight_discovery_boundary
                .remains_optional_observability_plane,
        public_bundled_official_entry_artifact_identity =
            public_auth_handoff_contract
                .bundled_official_entry_artifact_identity
                .as_str(),
        public_bundled_target_kind = public_auth_handoff_contract.bundled_target_kind.as_str(),
        public_bundled_target_is_non_local_candidate =
            public_auth_handoff_contract.bundled_target_is_non_local_candidate,
        public_bundled_transport_kind =
            public_auth_handoff_contract.bundled_transport_kind.as_str(),
        public_bundled_official_entry_use_srv = public_auth_handoff_contract.bundled_use_srv,
        public_bundled_official_entry_use_quic = public_auth_handoff_contract.bundled_use_quic,
        public_bundled_official_entry_validate_tls =
            public_auth_handoff_contract.bundled_validate_tls,
        public_bundled_auth_mode = public_auth_handoff_contract.bundled_auth_mode.as_str(),
        public_external_auth_rollout_ready =
            public_auth_handoff_contract.external_auth_rollout_ready,
        public_non_local_cutover_ready = public_auth_handoff_contract.non_local_cutover_ready,
        public_non_local_cutover_gap_reasons =
            ?public_auth_handoff_contract.non_local_cutover_gap_reasons,
        "Entry upgrade boundary contract loaded"
    );

    panic_handler::set_panic_hook(log_filename, logs_dir);

    // Setup tokio runtime
    use common::consts::MIN_RECOMMENDED_TOKIO_THREADS;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tokio::runtime::Builder;

    // TODO: evaluate std::thread::available_concurrency as a num_cpus replacement
    let cores = num_cpus::get();
    let tokio_runtime = Arc::new(
        Builder::new_multi_thread()
            .enable_all()
            .worker_threads((cores / 4).max(MIN_RECOMMENDED_TOKIO_THREADS))
            .thread_name_fn(|| {
                static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
                let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
                format!("tokio-voxygen-{}", id)
            })
            .build()
            .unwrap(),
    );

    // Initialise watcher for animation hot-reloading
    #[cfg(feature = "hot-anim")]
    {
        anim::init();
    }

    // Initialise watcher for egui hot-reloading
    #[cfg(feature = "hot-egui")]
    {
        voxygen_egui::init();
    }

    // Setup audio
    let mut audio = match settings.audio.output {
        AudioOutput::Off => AudioFrontend::no_audio(),
        AudioOutput::Automatic => AudioFrontend::new(
            settings.audio.num_sfx_channels,
            settings.audio.num_ui_channels,
            settings.audio.subtitles,
            // settings.audio.combat_music_enabled,
            false, // We're disabling combat music for now
            settings.audio.buffer_size,
            settings.audio.sample_rate,
        ),
    };

    audio.set_master_volume(settings.audio.master_volume.get_checked());
    audio.set_music_volume(settings.audio.music_volume.get_checked());
    audio.set_sfx_volume(settings.audio.sfx_volume.get_checked());
    audio.set_ambience_volume(settings.audio.ambience_volume.get_checked());
    audio.set_music_spacing(settings.audio.music_spacing);

    // Load the profile.
    let profile = Profile::load(&config_dir);

    let mut i18n =
        LocalizationHandle::load(&settings.language.selected_language).unwrap_or_else(|error| {
            let selected_language = &settings.language.selected_language;
            warn!(
                ?error,
                ?selected_language,
                "无法加载所选语言，已回退到默认语言（简体中文）。",
            );
            DEFAULT_LANGUAGE.clone_into(&mut settings.language.selected_language);
            LocalizationHandle::load_expect(&settings.language.selected_language)
        });
    i18n.set_english_fallback(settings.language.use_english_fallback);

    // Create window
    use veloren_voxygen::{error::Error, render::RenderError};
    let (mut window, event_loop) = match Window::new(&settings, &tokio_runtime) {
        Ok(ok) => ok,
        // Custom panic message when a graphics backend could not be found
        Err(Error::RenderError(RenderError::CouldNotFindAdapter)) => {
            #[cfg(target_os = "windows")]
            const POTENTIAL_FIX_KEY: &str = "main-login-render_backend_failed-fix-windows";
            #[cfg(target_os = "macos")]
            const POTENTIAL_FIX_KEY: &str = "main-login-render_backend_failed-fix-none";
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            const POTENTIAL_FIX_KEY: &str = "main-login-render_backend_failed-fix-vulkan";

            let potential_fix = i18n.read().get_msg(POTENTIAL_FIX_KEY).into_owned();
            panic!(
                "{}",
                i18n.read()
                    .get_msg_ctx("main-login-render_backend_failed", &i18n::fluent_args! {
                        "potential_fix" => potential_fix,
                    })
            );
        },
        Err(error) => panic!(
            "{}",
            i18n.read()
                .get_msg_ctx("main-login-window_create_failed", &i18n::fluent_args! {
                    "raw_error" => format!("{error:?}"),
                })
        ),
    };

    let clipboard = veloren_voxygen::ui::ice::Clipboard::connect(window.window());

    let lazy_init = SpriteRenderContext::new(window.renderer_mut());

    #[cfg(feature = "egui-ui")]
    let egui_state = EguiState::new(&window);

    #[cfg(feature = "discord")]
    let discord = if settings.networking.enable_discord_integration {
        veloren_voxygen::discord::Discord::start(&tokio_runtime)
    } else {
        veloren_voxygen::discord::Discord::Inactive
    };

    let global_state = GlobalState {
        userdata_dir,
        config_dir,
        audio,
        entry_policy,
        profile,
        window,
        tokio_runtime,
        #[cfg(feature = "egui-ui")]
        egui_state,
        lazy_init,
        clock: Clock::new(std::time::Duration::from_secs_f64(
            1.0 / get_fps(settings.graphics.max_fps) as f64,
        )),
        settings,
        info_message: None,
        #[cfg(feature = "singleplayer")]
        singleplayer: SingleplayerState::None,
        i18n,
        clipboard,
        clear_shadows_next_frame: false,
        #[cfg(feature = "discord")]
        discord,
        args: args.clone(),
    };

    run::run(global_state, event_loop).unwrap();
}
