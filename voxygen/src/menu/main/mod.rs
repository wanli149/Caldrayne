pub(crate) mod client_init;
mod ui;

use super::{char_selection::CharSelectionState, dummy_scene::Scene, server_info::ServerInfoState};
#[cfg(feature = "singleplayer")]
use crate::singleplayer::SingleplayerState;
use crate::{
    Direction, GlobalState, PlayState, PlayStateResult,
    entry::{HostKind, ResolvedConnectHost},
    hud,
    render::{Drawer, GlobalsBindGroup},
    session::SessionState,
    settings::Settings,
    window::Event,
};
use chrono::{DateTime, Local, Utc};
use client::{
    Client, ClientInitStage, ServerInfo,
    addr::ConnectionArgs,
    error::{
        InitProtocolError, NetworkConnectError, NetworkError, OTHER_BAD_ALTITUDE_MAP,
        OTHER_BAD_WORLD_MAP_DIMENSIONS, OTHER_BAD_WORLD_MAP_IMAGE, OTHER_ENTITY_FROM_UID_NOT_FOUND,
        OTHER_NO_IP_ADDR,
    },
};
use client_init::{ClientInit, Error as InitError, Msg as InitMsg};
use common::{comp, event::UpdateCharacterMetadata};
use common_base::span;
use common_net::msg::ClientType;
#[cfg(feature = "plugins")]
use common_state::plugin::PluginMgr;
use i18n::{LocalizationGuard, LocalizationHandle, fluent_args};
#[cfg(feature = "singleplayer")]
use server::{CompatEntryKindV1, CompatFailureKindV1, ServerInitStage};
#[cfg(any(feature = "singleplayer", feature = "plugins"))]
use specs::WorldExt;
use std::{cell::RefCell, path::Path, rc::Rc, sync::Arc};
use tokio::runtime;
use tracing::error;
use ui::{Event as MainMenuEvent, MainMenuUi};

pub use ui::rand_bg_image_spec;

#[derive(Debug)]
pub enum DetailedInitializationStage {
    PreparingHost(HostKind),
    #[cfg(feature = "singleplayer")]
    SingleplayerServer(ServerInitStage),
    Client(ClientInitStage),
    CreatingRenderPipeline(usize, usize),
}

enum InitState {
    None,
    // Waiting on the client initialization
    Client {
        init: ClientInit,
        host: ResolvedConnectHost,
    },
    // Client initialized but still waiting on Renderer pipeline creation
    Pipeline(Box<Client>, hud::PersistedHudState),
}

impl InitState {
    fn client(&self) -> Option<&ClientInit> {
        match self {
            Self::Client { init, .. } => Some(init),
            _ => None,
        }
    }

    fn host_kind(&self) -> Option<HostKind> {
        match self {
            Self::Client { host, .. } => Some(host.kind),
            _ => None,
        }
    }

    fn target_address(&self) -> Option<&str> {
        match self {
            Self::Client { host, .. } => host.target_address.as_deref(),
            _ => None,
        }
    }

    fn local_dedicated_instance_id(&self) -> Option<common::uuid::Uuid> {
        match self {
            Self::Client { host, .. } => host.local_dedicated_instance_id,
            _ => None,
        }
    }
}

pub struct MainMenuState {
    main_menu_ui: MainMenuUi,
    init: InitState,
    scene: Scene,
}

impl MainMenuState {
    /// Create a new `MainMenuState`.
    pub fn new(global_state: &mut GlobalState) -> Self {
        Self {
            main_menu_ui: MainMenuUi::new(global_state),
            init: InitState::None,
            scene: Scene::new(global_state.window.renderer_mut()),
        }
    }
}

#[cfg(feature = "singleplayer")]
const fn compat_entry_msg_key(entry: CompatEntryKindV1) -> &'static str {
    match entry {
        CompatEntryKindV1::Load => "main-servers-world_compat_entry-load",
        CompatEntryKindV1::LoadLegacy => "main-servers-world_compat_entry-load_legacy",
        CompatEntryKindV1::LoadAsset => "main-servers-world_compat_entry-load_asset",
        CompatEntryKindV1::Generate
        | CompatEntryKindV1::Save
        | CompatEntryKindV1::LoadOrGenerate => "main-servers-world_compat_entry-generic",
    }
}

#[cfg(feature = "singleplayer")]
const fn compat_failure_msg_key(failure: CompatFailureKindV1) -> &'static str {
    match failure {
        CompatFailureKindV1::MissingInput => "main-servers-world_compat_failure-missing_input",
        CompatFailureKindV1::ParseError => "main-servers-world_compat_failure-parse_error",
        CompatFailureKindV1::InvalidWorld => "main-servers-world_compat_failure-invalid_world",
        CompatFailureKindV1::OptionMismatch => "main-servers-world_compat_failure-option_mismatch",
        CompatFailureKindV1::None => "main-servers-world_compat_failure-generic",
    }
}

#[cfg(feature = "singleplayer")]
const fn compat_remediation_msg_key(entry: CompatEntryKindV1) -> &'static str {
    match entry {
        CompatEntryKindV1::Load | CompatEntryKindV1::LoadLegacy => {
            "main-servers-world_compat_remediation-load"
        },
        CompatEntryKindV1::LoadAsset => "main-servers-world_compat_remediation-load_asset",
        CompatEntryKindV1::Generate
        | CompatEntryKindV1::Save
        | CompatEntryKindV1::LoadOrGenerate => "main-servers-world_compat_remediation-generic",
    }
}

#[cfg(feature = "singleplayer")]
const fn compat_notice_remediation_msg_key(entry: CompatEntryKindV1) -> &'static str {
    match entry {
        CompatEntryKindV1::Load | CompatEntryKindV1::LoadLegacy => {
            "main-servers-world_compat_notice_remediation-load"
        },
        CompatEntryKindV1::LoadAsset => "main-servers-world_compat_notice_remediation-load_asset",
        CompatEntryKindV1::Generate
        | CompatEntryKindV1::Save
        | CompatEntryKindV1::LoadOrGenerate => {
            "main-servers-world_compat_notice_remediation-generic"
        },
    }
}

#[cfg(feature = "singleplayer")]
fn localized_compat_world_error(
    localization: &LocalizationGuard,
    audit: server::CompatAuditV1,
) -> String {
    localization
        .get_msg_ctx("main-servers-world_compat_error", &fluent_args! {
            "entry" => localization.get_msg(compat_entry_msg_key(audit.entry)),
            "failure" => localization.get_msg(compat_failure_msg_key(audit.failure_kind)),
            "remediation" => localization.get_msg(compat_remediation_msg_key(audit.entry)),
        })
        .into_owned()
}

#[cfg(feature = "singleplayer")]
fn localized_compat_world_notice(
    localization: &LocalizationGuard,
    audit: server::CompatAuditV1,
) -> String {
    localization
        .get_msg_ctx("main-servers-world_compat_notice", &fluent_args! {
            "entry" => localization.get_msg(compat_entry_msg_key(audit.entry)),
            "failure" => localization.get_msg(compat_failure_msg_key(audit.failure_kind)),
            "remediation" => localization.get_msg(compat_notice_remediation_msg_key(audit.entry)),
        })
        .into_owned()
}

#[cfg(feature = "singleplayer")]
fn localized_world_error(localization: &LocalizationGuard, error: String) -> String {
    localization
        .get_msg_ctx("main-servers-other_error", &fluent_args! {
            "raw_error" => error,
        })
        .into_owned()
}

impl PlayState for MainMenuState {
    fn enter(&mut self, global_state: &mut GlobalState, _: Direction) {
        // Kick off title music
        if global_state.settings.audio.output.is_enabled() && global_state.audio.music_enabled() {
            global_state.audio.play_title_music();
        }

        if let Some(message) = global_state.entry_policy.public_mode_blocker_message() {
            global_state.info_message = Some(message);
        }

        // Reset singleplayer server if it was running already
        #[cfg(feature = "singleplayer")]
        {
            global_state.singleplayer = SingleplayerState::None;
        }

        // Updated localization in case the selected language was changed
        self.main_menu_ui
            .update_language(global_state.i18n, &global_state.settings);
        // Set scale mode in case it was change
        self.main_menu_ui
            .set_scale_mode(global_state.settings.interface.ui_scale);

        #[cfg(feature = "discord")]
        global_state.discord.enter_main_menu();
    }

    fn tick(&mut self, global_state: &mut GlobalState, events: Vec<Event>) -> PlayStateResult {
        span!(_guard, "tick", "<MainMenuState as PlayState>::tick");

        // Pull in localizations
        let localized_strings = &global_state.i18n.read();

        // Poll server creation
        #[cfg(feature = "singleplayer")]
        {
            if let Some(singleplayer) = global_state.singleplayer.as_running() {
                if let Ok(stage_update) = singleplayer.init_stage_receiver.try_recv() {
                    self.main_menu_ui.update_stage(
                        DetailedInitializationStage::SingleplayerServer(stage_update),
                    );
                }

                match singleplayer.receiver.try_recv() {
                    Ok(Ok(init_outcome)) => {
                        if init_outcome.compat_audit.is_strict_load_contract_gap() {
                            self.main_menu_ui.show_connection_notice(
                                localized_compat_world_notice(
                                    localized_strings,
                                    init_outcome.compat_audit,
                                ),
                            );
                        }

                        // Attempt login after the server is finished initializing
                        attempt_login(
                            &mut global_state.info_message,
                            "singleplayer".to_owned(),
                            "".to_owned(),
                            ResolvedConnectHost {
                                kind: HostKind::DevSingleplayer,
                                connection_args: ConnectionArgs::Mpsc(14004),
                                target_address: None,
                                local_dedicated_instance_id: None,
                            },
                            &mut self.init,
                            &global_state.tokio_runtime,
                            global_state.settings.language.send_to_server.then_some(
                                global_state.settings.language.selected_language.clone(),
                            ),
                            &global_state.i18n,
                            &global_state.config_dir,
                            global_state.args.client_type.0,
                        );
                    },
                    Ok(Err(e)) => {
                        error!(?e, "Could not start server");
                        global_state.singleplayer = SingleplayerState::None;
                        self.init = InitState::None;
                        self.main_menu_ui.cancel_connection();
                        let server_err = match e {
                            server::Error::NetworkErr(e) => localized_strings
                                .get_msg_ctx("main-servers-network_error", &i18n::fluent_args! {
                                    "raw_error" => e.to_string()
                                })
                                .into_owned(),
                            server::Error::ParticipantErr(e) => localized_strings
                                .get_msg_ctx(
                                    "main-servers-participant_error",
                                    &i18n::fluent_args! {
                                        "raw_error" => e.to_string()
                                    },
                                )
                                .into_owned(),
                            server::Error::StreamErr(e) => localized_strings
                                .get_msg_ctx("main-servers-stream_error", &i18n::fluent_args! {
                                    "raw_error" => e.to_string()
                                })
                                .into_owned(),
                            server::Error::DatabaseErr(e) => localized_strings
                                .get_msg_ctx("main-servers-database_error", &i18n::fluent_args! {
                                    "raw_error" => e.to_string()
                                })
                                .into_owned(),
                            server::Error::PersistenceErr(e) => localized_strings
                                .get_msg_ctx(
                                    "main-servers-persistence_error",
                                    &i18n::fluent_args! {
                                        "raw_error" => e.to_string()
                                    },
                                )
                                .into_owned(),
                            server::Error::RtsimError(e) => localized_strings
                                .get_msg_ctx("main-servers-rtsim_error", &i18n::fluent_args! {
                                    "raw_error" => e.to_string(),
                                })
                                .into_owned(),
                            server::Error::WorldErr(e) => match e.compat_audit() {
                                Some(audit) => {
                                    localized_compat_world_error(localized_strings, audit)
                                },
                                None => localized_world_error(localized_strings, e.to_string()),
                            },
                            server::Error::Other(e) => localized_strings
                                .get_msg_ctx("main-servers-other_error", &i18n::fluent_args! {
                                    "raw_error" => e,
                                })
                                .into_owned(),
                        };
                        global_state.info_message = Some(
                            localized_strings
                                .get_msg_ctx(
                                    "main-servers-singleplayer_error",
                                    &i18n::fluent_args! {
                                        "sp_error" => server_err
                                    },
                                )
                                .into_owned(),
                        );
                    },
                    Err(_) => (),
                }
            }
        }
        // Handle window events.
        for event in events {
            // Pass all events to the ui first.
            if self.main_menu_ui.handle_event(event.clone()) {
                continue;
            }

            // Shutdown on Close, ignore all other events.
            if matches!(event, Event::Close) {
                return PlayStateResult::Shutdown;
            }
        }

        if let Some(client_stage_update) = self.init.client().and_then(|init| init.stage_update()) {
            self.main_menu_ui
                .update_stage(DetailedInitializationStage::Client(client_stage_update));
        }

        // Poll client creation.
        match self.init.client().and_then(|init| init.poll()) {
            Some(InitMsg::Done(Ok(mut client))) => {
                if self.init.host_kind() == Some(HostKind::DevLocalDedicated) {
                    let server_info = client.server_info().clone();
                    let changed = if let Some(instance_id) = self.init.local_dedicated_instance_id()
                    {
                        global_state
                            .settings
                            .networking
                            .update_local_dedicated_observation_by_instance_id(
                                instance_id,
                                server_info.realm_id,
                                &server_info.name,
                            )
                    } else if let Some(target_address) = self.init.target_address() {
                        global_state
                            .settings
                            .networking
                            .update_local_dedicated_observation(
                                target_address,
                                server_info.realm_id,
                                &server_info.name,
                            )
                    } else {
                        false
                    };
                    if changed {
                        global_state
                            .settings
                            .save_to_file_warn(&global_state.config_dir);
                    }
                }

                // load local plugins needed by the server
                #[cfg(feature = "plugins")]
                for path in client.take_local_plugins().drain(..) {
                    if let Err(e) = client
                        .state_mut()
                        .ecs_mut()
                        .write_resource::<PluginMgr>()
                        .load_server_plugin(path)
                    {
                        tracing::error!(?e, "load local plugin");
                    }
                }
                // Register voxygen components / resources
                crate::ecs::init(client.state_mut().ecs_mut());
                self.init =
                    InitState::Pipeline(Box::new(client), hud::PersistedHudState::default());
            },
            Some(InitMsg::Done(Err(e))) => {
                self.init = InitState::None;
                error!(?e, "Client Init failed raw error");
                let e = get_client_init_msg_error(e, &global_state.i18n);
                // Log error for possible additional use later or in case that the error
                // displayed is cut of.
                error!(?e, "Client Init failed");
                global_state.info_message = Some(
                    localized_strings
                        .get_msg_ctx("main-login-client_init_failed", &i18n::fluent_args! {
                            "init_fail_reason" => e
                        })
                        .into_owned(),
                );
            },
            Some(InitMsg::IsAuthTrusted(auth_server)) => {
                if global_state.entry_policy.is_auth_server_trusted(
                    self.init.host_kind().unwrap_or(HostKind::PublicOfficial),
                    &auth_server,
                    &global_state.settings.networking.trusted_auth_servers,
                ) {
                    // Can't fail since we just polled it, it must be Some
                    self.init.client().unwrap().auth_trust(auth_server, true);
                } else if self.init.host_kind().is_some_and(|host_kind| {
                    global_state
                        .entry_policy
                        .should_prompt_for_auth_trust(host_kind)
                }) {
                    // Show warning that auth server is not trusted and prompt for approval
                    self.main_menu_ui.auth_trust_prompt(auth_server);
                } else {
                    self.init.client().unwrap().auth_trust(auth_server, false);
                }
            },
            None => {},
        }

        // Tick the client to keep the connection alive if we are waiting on pipelines
        if let InitState::Pipeline(client, _) = &mut self.init {
            match client.tick(comp::ControllerInputs::default(), global_state.clock.dt()) {
                Ok(events) => {
                    for event in events {
                        match event {
                            client::Event::SetViewDistance(_vd) => {},
                            client::Event::Disconnect => {
                                global_state.info_message = Some(
                                    localized_strings
                                        .get_msg("main-login-server_shut_down")
                                        .into_owned(),
                                );
                                self.init = InitState::None;
                            },
                            client::Event::Chat(m) => {
                                if let InitState::Pipeline(client, persisted_state) = &mut self.init
                                {
                                    persisted_state.message_backlog.new_message(
                                        client,
                                        &global_state.profile,
                                        m,
                                    )
                                }
                            },
                            client::Event::MapMarker(marker_event) => {
                                if let InitState::Pipeline(_client, persisted_state) =
                                    &mut self.init
                                {
                                    persisted_state.location_markers.update(marker_event);
                                }
                            },
                            #[cfg_attr(not(feature = "plugins"), expect(unused_variables))]
                            client::Event::PluginDataReceived(data) => {
                                #[cfg(feature = "plugins")]
                                {
                                    tracing::info!("plugin data {}", data.len());
                                    if let InitState::Pipeline(client, _) = &mut self.init {
                                        let hash = client
                                            .state()
                                            .ecs()
                                            .write_resource::<PluginMgr>()
                                            .cache_server_plugin(&global_state.config_dir, data);
                                        match hash {
                                            Ok(hash) => {
                                                if client.plugin_received(hash) == 0 {
                                                    // now load characters (plugins might contain
                                                    // items)
                                                    client.load_character_list();
                                                }
                                            },
                                            Err(e) => tracing::error!(?e, "cache_server_plugin"),
                                        }
                                    }
                                }
                            },
                            _ => {},
                        }
                    }
                },
                Err(err) => {
                    error!(?err, "[main menu] Failed to tick the client");
                    global_state.info_message =
                        Some(get_client_msg_error(err, None, &global_state.i18n.read()));
                    self.init = InitState::None;
                },
            }
        }

        // Poll renderer pipeline creation
        if let InitState::Pipeline(..) = &self.init {
            if let Some((done, total)) = &global_state.window.renderer().pipeline_creation_status()
            {
                self.main_menu_ui.update_stage(
                    DetailedInitializationStage::CreatingRenderPipeline(*done, *total),
                );
            // If complete go to char select screen
            } else {
                // Always succeeds since we check above
                if let InitState::Pipeline(mut client, persisted_state) =
                    core::mem::replace(&mut self.init, InitState::None)
                {
                    self.main_menu_ui.connected();

                    // If the client cannot enter the game but spectate, skip from the character
                    // menu directly to spectating.
                    if client.client_type().can_spectate()
                        && !client.client_type().can_enter_character()
                    {
                        client.request_spectate(global_state.settings.graphics.view_distances());

                        return PlayStateResult::Push(Box::new(SessionState::new(
                            global_state,
                            UpdateCharacterMetadata::default(),
                            Rc::new(RefCell::new(*client)),
                            Rc::new(RefCell::new(persisted_state)),
                        )));
                    }

                    let server_info = client.server_info().clone();
                    let server_description = client.server_description().clone();

                    let char_select = CharSelectionState::new(
                        global_state,
                        Rc::new(RefCell::new(*client)),
                        Rc::new(RefCell::new(persisted_state)),
                    );

                    let new_state = ServerInfoState::try_from_server_info(
                        global_state,
                        self.main_menu_ui.bg_img_spec(),
                        char_select,
                        server_info,
                        server_description,
                        false,
                    )
                    .map(|s| Box::new(s) as _)
                    .unwrap_or_else(|s| Box::new(s) as _);

                    return PlayStateResult::Push(new_state);
                }
            }
        }

        // Maintain the UI.
        for event in self
            .main_menu_ui
            .maintain(global_state, global_state.clock.dt())
        {
            match event {
                MainMenuEvent::LoginAttempt {
                    username,
                    password,
                    server_address,
                    local_dedicated_instance_id,
                    host_kind,
                } => {
                    let entry_policy = global_state.entry_policy.clone();
                    {
                        let net_settings = &mut global_state.settings.networking;
                        entry_policy.apply_login_settings(
                            net_settings,
                            host_kind,
                            &username,
                            &server_address,
                            local_dedicated_instance_id,
                        );
                    }
                    global_state
                        .settings
                        .save_to_file_warn(&global_state.config_dir);

                    let host = match entry_policy.resolve_multiplayer_host(
                        host_kind,
                        &server_address,
                        local_dedicated_instance_id,
                        &global_state.settings.networking,
                    ) {
                        Ok(host) => {
                            if host.kind != host_kind {
                                tracing::warn!(
                                    ui_host_kind = ?host_kind,
                                    resolved_host_kind = ?host.kind,
                                    "Main menu UI host kind drifted from resolved host kind"
                                );
                            }
                            host
                        },
                        Err(error) => {
                            global_state.info_message = Some(error);
                            self.init = InitState::None;
                            self.main_menu_ui.cancel_connection();
                            continue;
                        },
                    };
                    attempt_login(
                        &mut global_state.info_message,
                        username,
                        password,
                        host,
                        &mut self.init,
                        &global_state.tokio_runtime,
                        global_state
                            .settings
                            .language
                            .send_to_server
                            .then_some(global_state.settings.language.selected_language.clone()),
                        &global_state.i18n,
                        &global_state.config_dir,
                        global_state.args.client_type.0,
                    );
                },
                MainMenuEvent::CancelLoginAttempt => {
                    // init contains InitState::Client(ClientInit), which spawns a thread which
                    // contains a TcpStream::connect() call This call is
                    // blocking TODO fix when the network rework happens
                    #[cfg(feature = "singleplayer")]
                    {
                        global_state.singleplayer = SingleplayerState::None;
                    }
                    self.init = InitState::None;
                    self.main_menu_ui.cancel_connection();
                },
                MainMenuEvent::ChangeLanguage(new_language) => {
                    global_state.settings.language.selected_language =
                        new_language.language_identifier;
                    global_state.i18n = LocalizationHandle::load_expect(
                        &global_state.settings.language.selected_language,
                    );
                    global_state
                        .i18n
                        .set_english_fallback(global_state.settings.language.use_english_fallback);
                    self.main_menu_ui
                        .update_language(global_state.i18n, &global_state.settings);
                },
                #[cfg(feature = "singleplayer")]
                MainMenuEvent::StartSingleplayer => {
                    if global_state.entry_policy.can_use_singleplayer() {
                        global_state.singleplayer.run(
                            &global_state.tokio_runtime,
                            &global_state.settings.language.selected_language,
                            &global_state.i18n,
                        );
                    }
                },
                #[cfg(feature = "singleplayer")]
                MainMenuEvent::InitSingleplayer => {
                    if global_state.entry_policy.can_use_singleplayer() {
                        global_state.singleplayer = SingleplayerState::init();
                    }
                },
                #[cfg(feature = "singleplayer")]
                MainMenuEvent::SinglePlayerChange(change) => {
                    if global_state.entry_policy.can_use_singleplayer() {
                        if let SingleplayerState::Init(ref mut init) = global_state.singleplayer {
                            match change {
                                ui::WorldsChange::SetActive(world) => init.current = world,
                                ui::WorldsChange::Delete(world) => init.remove(world),
                                ui::WorldsChange::Regenerate(world) => init.delete_map_file(world),
                                ui::WorldsChange::AddNew => init.new_world(),
                                ui::WorldsChange::CurrentWorldChange(change) => {
                                    if let Some(world) = init.current.map(|i| &mut init.worlds[i]) {
                                        change.apply(world);
                                        init.save_current_meta();
                                    }
                                },
                            }
                        }
                    }
                },
                MainMenuEvent::Quit => return PlayStateResult::Shutdown,
                // Note: Keeping in case we re-add the disclaimer
                /*MainMenuEvent::DisclaimerAccepted => {
                    global_state.settings.show_disclaimer = false
                },*/
                MainMenuEvent::AuthServerTrust(auth_server, trust) => {
                    if trust
                        && self.init.host_kind().is_some_and(|host_kind| {
                            global_state
                                .entry_policy
                                .should_persist_auth_trust(host_kind)
                        })
                    {
                        global_state
                            .settings
                            .networking
                            .trusted_auth_servers
                            .insert(auth_server.clone());
                        global_state
                            .settings
                            .save_to_file_warn(&global_state.config_dir);
                    }
                    self.init
                        .client()
                        .map(|init| init.auth_trust(auth_server, trust));
                },
                MainMenuEvent::DeleteServer { server_index } => {
                    if global_state.entry_policy.can_manage_server_history() {
                        let net_settings = &mut global_state.settings.networking;
                        if server_index < net_settings.servers.len() {
                            net_settings.servers.remove(server_index);
                        }
                    }

                    global_state
                        .settings
                        .save_to_file_warn(&global_state.config_dir);
                },
                MainMenuEvent::RegisterLocalDedicated { server_address } => {
                    if global_state.entry_policy.can_manage_server_history() {
                        global_state
                            .settings
                            .networking
                            .register_manual_local_dedicated_from_direct_connect(&server_address);
                    }

                    global_state
                        .settings
                        .save_to_file_warn(&global_state.config_dir);
                },
                MainMenuEvent::UpdateLocalDedicated {
                    instance_id,
                    display_name,
                    server_address,
                    connection_kind,
                    validate_tls,
                } => {
                    if global_state.entry_policy.can_manage_server_history() {
                        global_state
                            .settings
                            .networking
                            .update_manual_local_dedicated_registration(
                                instance_id,
                                crate::settings::ManualLocalDedicatedServerSpec {
                                    instance_id: Some(instance_id),
                                    data_dir: None,
                                    display_name,
                                    server_address,
                                    connection_kind,
                                    validate_tls,
                                },
                            );
                    }

                    global_state
                        .settings
                        .save_to_file_warn(&global_state.config_dir);
                },
                MainMenuEvent::DeleteLocalDedicated { instance_id } => {
                    if global_state.entry_policy.can_manage_server_history() {
                        global_state
                            .settings
                            .networking
                            .remove_local_dedicated_manual_registration(instance_id);
                    }

                    global_state
                        .settings
                        .save_to_file_warn(&global_state.config_dir);
                },
            }
        }

        if let Some(info) = global_state.info_message.take() {
            self.main_menu_ui.show_info(info);
        }

        PlayStateResult::Continue
    }

    fn name(&self) -> &'static str { "Title" }

    fn capped_fps(&self) -> bool { true }

    fn globals_bind_group(&self) -> &GlobalsBindGroup { self.scene.global_bind_group() }

    fn render(&self, drawer: &mut Drawer<'_>, _: &Settings) {
        // Draw the UI to the screen.
        let mut third_pass = drawer.third_pass();
        if let Some(mut ui_drawer) = third_pass.draw_ui() {
            self.main_menu_ui.render(&mut ui_drawer);
        };
    }

    fn egui_enabled(&self) -> bool { false }
}

#[cfg(all(test, feature = "singleplayer"))]
mod tests {
    use super::{compat_entry_msg_key, compat_failure_msg_key, compat_remediation_msg_key};
    use server::{CompatEntryKindV1, CompatFailureKindV1};

    #[test]
    fn compat_entry_keys_match_expected_variants() {
        assert_eq!(
            compat_entry_msg_key(CompatEntryKindV1::Load),
            "main-servers-world_compat_entry-load"
        );
        assert_eq!(
            compat_entry_msg_key(CompatEntryKindV1::LoadAsset),
            "main-servers-world_compat_entry-load_asset"
        );
        assert_eq!(
            compat_entry_msg_key(CompatEntryKindV1::Generate),
            "main-servers-world_compat_entry-generic"
        );
    }

    #[test]
    fn compat_failure_keys_match_expected_variants() {
        assert_eq!(
            compat_failure_msg_key(CompatFailureKindV1::MissingInput),
            "main-servers-world_compat_failure-missing_input"
        );
        assert_eq!(
            compat_failure_msg_key(CompatFailureKindV1::OptionMismatch),
            "main-servers-world_compat_failure-option_mismatch"
        );
        assert_eq!(
            compat_failure_msg_key(CompatFailureKindV1::None),
            "main-servers-world_compat_failure-generic"
        );
    }

    #[test]
    fn compat_remediation_keys_match_expected_variants() {
        assert_eq!(
            compat_remediation_msg_key(CompatEntryKindV1::LoadLegacy),
            "main-servers-world_compat_remediation-load"
        );
        assert_eq!(
            compat_remediation_msg_key(CompatEntryKindV1::LoadAsset),
            "main-servers-world_compat_remediation-load_asset"
        );
        assert_eq!(
            compat_remediation_msg_key(CompatEntryKindV1::LoadOrGenerate),
            "main-servers-world_compat_remediation-generic"
        );
    }
}

pub(crate) fn get_client_msg_error(
    error: client::Error,
    mismatched_server_info: Option<ServerInfo>,
    localization: &LocalizationGuard,
) -> String {
    // When a network error is received and there is a mismatch between the client
    // and server version it is almost definitely due to this mismatch rather than
    // a true networking error.
    let net_error = |error: String, mismatched_server_info: Option<ServerInfo>| -> String {
        if let Some(server_info) = mismatched_server_info.filter(|info| {
            info.git_hash != *common::util::GIT_HASH
                || info.git_timestamp != *common::util::GIT_TIMESTAMP
        }) {
            format!(
                "{} {}: {} {}: {}",
                localization.get_msg("main-login-network_wrong_version"),
                localization.get_msg("main-login-client_version"),
                &*common::util::DISPLAY_VERSION,
                localization.get_msg("main-login-server_version"),
                common::util::make_display_version(server_info.git_hash, server_info.git_timestamp),
            )
        } else {
            format!(
                "{}: {}",
                localization.get_msg("main-login-network_error"),
                error
            )
        }
    };

    use client::Error;
    match error {
        Error::SpecsErr(e) => {
            format!(
                "{}: {}",
                localization.get_msg("main-login-internal_error"),
                e
            )
        },
        Error::AuthErr(e) => format!(
            "{}: {}",
            localization.get_msg("main-login-authentication_error"),
            e
        ),
        Error::Kicked(reason) => localization
            .get_msg_ctx("main-login-kicked", &fluent_args! {
                "reason" => reason,
            })
            .into(),
        Error::TooManyPlayers => localization.get_msg("main-login-server_full").into(),
        Error::AuthServerNotTrusted => localization
            .get_msg("main-login-untrusted_auth_server")
            .into(),
        Error::IncompatibleServerGeneration { client, server } => format!(
            "{} Client compatibility generation: {} (min {}), server compatibility generation: {} \
             (min {}).",
            localization.get_msg("main-login-network_wrong_version"),
            client.generation,
            client.minimum_supported_generation,
            server.generation,
            server.minimum_supported_generation,
        ),
        Error::ServerTimeout => localization.get_msg("main-login-timeout").into(),
        Error::ServerShutdown => localization.get_msg("main-login-server_shut_down").into(),
        Error::NotOnWhitelist => localization.get_msg("main-login-not_on_whitelist").into(),
        Error::Banned(ban_info) => if let Some(end_time) = ban_info
            .until
            .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
        {
            let end_date = end_time.with_timezone(&Local);
            let end_date_str = end_date.format("%Y-%m-%d %H:%M").to_string();

            localization.get_msg_ctx("main-login-banned_until", &fluent_args! {
                "reason" => ban_info.reason,
                "end_date" => end_date_str,
            })
        } else {
            localization.get_msg_ctx("main-login-banned", &fluent_args! {
                "reason" => ban_info.reason
            })
        }
        .into(),
        Error::InvalidCharacter => localization.get_msg("main-login-invalid_character").into(),
        Error::NetworkErr(NetworkError::ConnectFailed(NetworkConnectError::Handshake(
            InitProtocolError::WrongVersion(_),
        ))) => net_error(
            localization
                .get_msg("main-login-network_wrong_version")
                .into_owned(),
            mismatched_server_info,
        ),
        Error::NetworkErr(e) => net_error(e.to_string(), mismatched_server_info),
        Error::ParticipantErr(e) => net_error(e.to_string(), mismatched_server_info),
        Error::StreamErr(e) => net_error(e.to_string(), mismatched_server_info),
        Error::RustlsErr(e) => net_error(e.to_string(), mismatched_server_info),
        Error::HostnameLookupFailed(e) => {
            format!(
                "{}: {}",
                localization.get_msg("main-login-server_not_found"),
                e
            )
        },
        Error::Other(e) => match e.as_str() {
            OTHER_NO_IP_ADDR => localization.get_msg("main-login-no_ip_addr").into(),
            OTHER_BAD_WORLD_MAP_DIMENSIONS => localization
                .get_msg("main-login-bad_world_map_dimensions")
                .into(),
            OTHER_BAD_WORLD_MAP_IMAGE => localization
                .get_msg("main-login-bad_world_map_image")
                .into(),
            OTHER_BAD_ALTITUDE_MAP => localization.get_msg("main-login-bad_altitude_map").into(),
            OTHER_ENTITY_FROM_UID_NOT_FOUND => {
                localization.get_msg("main-login-entity_sync_failed").into()
            },
            _ => format!("{}: {}", localization.get_msg("common-error"), e),
        },
        Error::AuthClientError(e) => match e {
            // TODO: remove parentheses
            client::AuthClientError::RequestError(e) => format!(
                "{}: {}",
                localization.get_msg("main-login-failed_sending_request"),
                e
            ),
            client::AuthClientError::ResponseError(e) => format!(
                "{}: {}",
                localization.get_msg("main-login-failed_sending_request"),
                e
            ),
            client::AuthClientError::CertificateLoad(e) => format!(
                "{}: {}",
                localization.get_msg("main-login-failed_sending_request"),
                e
            ),
            client::AuthClientError::JsonError(e) => format!(
                "{}: {}",
                localization.get_msg("main-login-failed_sending_request"),
                e
            ),
            client::AuthClientError::InsecureSchema => localization
                .get_msg("main-login-insecure_auth_scheme")
                .into(),
            client::AuthClientError::ServerError(_, e) => String::from_utf8_lossy(&e).into(),
        },
        Error::AuthServerUrlInvalid(e) => {
            format!(
                "{}: https://{}",
                localization.get_msg("main-login-failed_auth_server_url_invalid"),
                e
            )
        },
    }
}

fn get_client_init_msg_error(
    error: client_init::Error,
    localized_strings: &LocalizationHandle,
) -> String {
    let localization = localized_strings.read();

    match error {
        InitError::ClientError {
            error,
            mismatched_server_info,
        } => get_client_msg_error(error, mismatched_server_info, &localization),
        InitError::ClientCrashed => localization.get_msg("main-login-client_crashed").into(),
        InitError::ServerNotFound => localization.get_msg("main-login-server_not_found").into(),
    }
}

fn attempt_login(
    info_message: &mut Option<String>,
    username: String,
    password: String,
    host: ResolvedConnectHost,
    init: &mut InitState,
    runtime: &Arc<runtime::Runtime>,
    locale: Option<String>,
    localized_strings: &LocalizationHandle,
    config_dir: &Path,
    client_type: ClientType,
) {
    let localization = localized_strings.read();
    if let Err(err) = comp::Player::alias_validate(&username) {
        match err {
            comp::AliasError::ForbiddenCharacters => {
                *info_message = Some(
                    localization
                        .get_msg("main-login-username_bad_characters")
                        .into_owned(),
                );
            },
            comp::AliasError::TooLong => {
                *info_message = Some(
                    localization
                        .get_msg_ctx("main-login-username_too_long", &i18n::fluent_args! {
                            "max_len" => comp::MAX_ALIAS_LEN
                        })
                        .into_owned(),
                );
            },
        }
        return;
    }

    // Don't try to connect if there is already a connection in progress.
    if let InitState::None = init {
        *init = InitState::Client {
            init: ClientInit::new(
                host.connection_args.clone(),
                username,
                password,
                Arc::clone(runtime),
                locale,
                config_dir,
                client_type,
            ),
            host,
        };
    }
}
