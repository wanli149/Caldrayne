mod connecting;
// Note: Keeping in case we re-add the disclaimer
//mod disclaimer;
mod credits;
mod local_dedicated;
mod login;
mod servers;
#[cfg(feature = "singleplayer")]
mod world_selector;

use crate::{
    GlobalState,
    credits::Credits,
    entry::{DevMultiplayerEntry, EntryPolicy, HostKind},
    render::UiDrawer,
    ui::{
        self, Graphic,
        fonts::IcedFonts as Fonts,
        ice::{Element, IcedUi as Ui, load_font, style, widget},
        img_ids::ImageGraphic,
    },
    window::{self, TextInputTarget},
};
use i18n::{LanguageMetadata, LocalizationHandle};
use iced::{Column, Container, HorizontalAlignment, Length, Row, Space, text_input};
//ImageFrame, Tooltip,
use crate::settings::Settings;
use common::{
    assets::{AssetExt, Image, Ron},
    uuid::Uuid,
};
use rand::{rng, seq::IndexedRandom};
use std::time::Duration;
use tracing::warn;

use super::DetailedInitializationStage;

// TODO: what is this? (showed up in rebase)
//const COL1: Color = Color::Rgba(0.07, 0.1, 0.1, 0.9);

pub const TEXT_COLOR: iced::Color = iced::Color::from_rgb(1.0, 1.0, 1.0);
pub const DISABLED_TEXT_COLOR: iced::Color = iced::Color::from_rgba(1.0, 1.0, 1.0, 0.2);

pub const FILL_FRAC_ONE: f32 = 0.67;
pub const FILL_FRAC_TWO: f32 = 0.53;

image_ids_ice! {
    struct Imgs {
        <ImageGraphic>
        v_logo: "voxygen.element.v_logo",
        bg: "voxygen.background.bg_main",
        banner_top: "voxygen.element.ui.generic.frames.banner_top",
        banner_gradient_bottom: "voxygen.element.ui.generic.frames.banner_gradient_bottom",
        button: "voxygen.element.ui.generic.buttons.button",
        button_hover: "voxygen.element.ui.generic.buttons.button_hover",
        button_press: "voxygen.element.ui.generic.buttons.button_press",
        input_bg: "voxygen.element.ui.generic.textbox",
        loading_art: "voxygen.element.ui.generic.frames.loading_screen.loading_bg",
        loading_art_l: "voxygen.element.ui.generic.frames.loading_screen.loading_bg_l",
        loading_art_r: "voxygen.element.ui.generic.frames.loading_screen.loading_bg_r",
        selection: "voxygen.element.ui.generic.frames.selection",
        selection_hover: "voxygen.element.ui.generic.frames.selection_hover",
        selection_press: "voxygen.element.ui.generic.frames.selection_press",

        #[cfg(feature = "singleplayer")]
        slider_range: "voxygen.element.ui.generic.slider.track",
        #[cfg(feature = "singleplayer")]
        slider_indicator: "voxygen.element.ui.generic.slider.indicator",

        unlock: "voxygen.element.ui.generic.buttons.unlock",
        unlock_hover: "voxygen.element.ui.generic.buttons.unlock_hover",
        unlock_press: "voxygen.element.ui.generic.buttons.unlock_press",
    }
}

// Randomly loaded background images
const BG_IMGS: [&str; 41] = [
    "voxygen.background.bg_1",
    "voxygen.background.bg_2",
    "voxygen.background.bg_3",
    "voxygen.background.bg_4",
    "voxygen.background.bg_5",
    "voxygen.background.bg_6",
    "voxygen.background.bg_7",
    "voxygen.background.bg_8",
    "voxygen.background.bg_9",
    "voxygen.background.bg_10",
    "voxygen.background.bg_11",
    "voxygen.background.bg_12",
    "voxygen.background.bg_13",
    "voxygen.background.bg_14",
    "voxygen.background.bg_15",
    "voxygen.background.bg_16",
    "voxygen.background.bg_17",
    "voxygen.background.bg_18",
    "voxygen.background.bg_19",
    "voxygen.background.bg_20",
    "voxygen.background.bg_21",
    "voxygen.background.bg_22",
    "voxygen.background.bg_23",
    "voxygen.background.bg_24",
    "voxygen.background.bg_25",
    "voxygen.background.bg_26",
    "voxygen.background.bg_27",
    "voxygen.background.bg_28",
    "voxygen.background.bg_29",
    "voxygen.background.bg_30",
    "voxygen.background.bg_31",
    "voxygen.background.bg_32",
    "voxygen.background.bg_33",
    "voxygen.background.bg_34",
    "voxygen.background.bg_35",
    "voxygen.background.bg_36",
    "voxygen.background.bg_37",
    "voxygen.background.bg_38",
    "voxygen.background.bg_39",
    "voxygen.background.bg_40",
    "voxygen.background.bg_41",
];

#[cfg(feature = "singleplayer")]
#[derive(Clone)]
pub enum WorldChange {
    Name(String),
    Seed(u32),
    DayLength(f64),
    SizeX(u32),
    SizeY(u32),
    Scale(f64),
    MapKind(common::resources::MapKind),
    ErosionQuality(f32),
    DefaultGenOps,
}

#[cfg(feature = "singleplayer")]
impl WorldChange {
    pub fn apply(self, world: &mut crate::singleplayer::SingleplayerWorld) {
        let mut def = Default::default();
        {
            let gen_opts = world.gen_opts.as_mut().unwrap_or(&mut def);
            match self {
                WorldChange::Name(name) => world.name = name,
                WorldChange::Seed(seed) => world.seed = seed,
                WorldChange::DayLength(d) => world.day_length = d,
                WorldChange::SizeX(s) => gen_opts.x_lg = s,
                WorldChange::SizeY(s) => gen_opts.y_lg = s,
                WorldChange::Scale(scale) => gen_opts.scale = scale,
                WorldChange::MapKind(kind) => gen_opts.map_kind = kind,
                WorldChange::ErosionQuality(q) => gen_opts.erosion_quality = q,
                WorldChange::DefaultGenOps => world.gen_opts = Some(Default::default()),
            }
        }

        if !world.is_generated {
            world.refresh_pending_source_contract();
        }
    }
}

#[cfg(feature = "singleplayer")]
#[derive(Clone)]
pub enum WorldsChange {
    SetActive(Option<usize>),
    Delete(usize),
    Regenerate(usize),
    AddNew,
    CurrentWorldChange(WorldChange),
}

pub enum Event {
    LoginAttempt {
        username: String,
        password: String,
        server_address: String,
        local_dedicated_instance_id: Option<Uuid>,
        host_kind: HostKind,
    },
    CancelLoginAttempt,
    ChangeLanguage(LanguageMetadata),
    #[cfg(feature = "singleplayer")]
    StartSingleplayer,
    #[cfg(feature = "singleplayer")]
    InitSingleplayer,
    #[cfg(feature = "singleplayer")]
    SinglePlayerChange(WorldsChange),
    Quit,
    // Note: Keeping in case we re-add the disclaimer
    //DisclaimerAccepted,
    AuthServerTrust(String, bool),
    RegisterLocalDedicated {
        server_address: String,
    },
    DeleteServer {
        server_index: usize,
    },
    UpdateLocalDedicated {
        instance_id: Uuid,
        display_name: String,
        server_address: String,
        connection_kind: crate::settings::LocalDedicatedConnectionKind,
        validate_tls: bool,
    },
    DeleteLocalDedicated {
        instance_id: Uuid,
    },
}

pub struct LoginInfo {
    pub username: String,
    pub password: String,
    pub server: String,
}

enum ConnectionState {
    InProgress,
    AuthTrustPrompt { auth_server: String, msg: String },
}

enum Screen {
    // Note: Keeping in case we re-add the disclaimer
    /*Disclaimer {
        screen: disclaimer::Screen,
    },*/
    Credits {
        screen: credits::Screen,
    },
    Login {
        screen: Box<login::Screen>, // boxed to avoid large variant
        // Error to display in a box
        error: Option<String>,
    },
    Servers {
        screen: servers::Screen,
    },
    LocalDedicatedEditor {
        screen: Box<local_dedicated::Screen>,
    },
    Connecting {
        screen: connecting::Screen,
        connection_state: ConnectionState,
        init_stage: DetailedInitializationStage,
        notice: Option<String>,
    },
    #[cfg(feature = "singleplayer")]
    WorldSelector {
        screen: world_selector::Screen,
    },
}

#[derive(PartialEq, Eq)]
enum Showing {
    Login,
    Languages,
}

impl Showing {
    fn toggle(&mut self, other: Showing) {
        if *self == other {
            *self = Showing::Login;
        } else {
            *self = other;
        }
    }
}

pub struct Controls {
    fonts: Fonts,
    imgs: Imgs,
    bg_img: widget::image::Handle,
    i18n: LocalizationHandle,
    // Voxygen version
    version: String,
    credits: Credits,

    // Public mode and CLI-pinned dev mode both lock the server field. Only the CLI-pinned dev
    // mode may unlock it back into an editable field.
    server_field_locked: bool,
    allow_server_unlock: bool,
    allow_server_list: bool,
    allow_singleplayer: bool,
    allow_multiplayer: bool,
    entry_policy: EntryPolicy,
    cli_server: Option<String>,
    dev_multiplayer_entries: Vec<DevMultiplayerEntry>,
    multiplayer_host_kind: HostKind,
    selected_local_dedicated_instance_id: Option<Uuid>,
    selected_server_index: Option<usize>,
    login_info: LoginInfo,

    show: Showing,
    selected_language_index: Option<usize>,

    time: f64,

    screen: Screen,
}

#[derive(Clone)]
enum Message {
    Quit,
    Back,
    ShowServers,
    ShowCredits,
    #[cfg(feature = "singleplayer")]
    Singleplayer,
    #[cfg(feature = "singleplayer")]
    SingleplayerPlay,
    #[cfg(feature = "singleplayer")]
    WorldChanged(WorldsChange),
    #[cfg(feature = "singleplayer")]
    WorldCancelConfirmation,
    #[cfg(feature = "singleplayer")]
    WorldConfirmation(world_selector::Confirmation),
    Multiplayer,
    UnlockServerField,
    LanguageChanged(usize),
    OpenLanguageMenu,
    Username(String),
    Password(String),
    Server(String),
    ServerChanged(usize),
    FocusPassword,
    CancelConnect,
    TrustPromptAdd,
    TrustPromptCancel,
    CloseError,
    EditLocalDedicated,
    AddLocalDedicated,
    DeleteServer,
    LocalDedicatedDisplayName(String),
    LocalDedicatedServerAddress(String),
    LocalDedicatedConnectionKind(crate::settings::LocalDedicatedConnectionKind),
    ToggleLocalDedicatedTls,
    SaveLocalDedicatedEditor,
    /* Note: Keeping in case we re-add the disclaimer
     *AcceptDisclaimer, */
}

impl Controls {
    fn new(
        fonts: Fonts,
        imgs: Imgs,
        bg_img: widget::image::Handle,
        i18n: LocalizationHandle,
        settings: &Settings,
        entry_policy: &EntryPolicy,
        server: Option<String>,
    ) -> Self {
        let version = format!(
            "Caldrayne Online (Veldr) {}",
            *common::util::DISPLAY_VERSION
        );

        let credits = Ron::<Credits>::load_expect_cloned("credits").into_inner();

        // Note: Keeping in case we re-add the disclaimer
        let screen = /* if settings.show_disclaimer {
            Screen::Disclaimer {
                screen: disclaimer::Screen::new(),
            }
        } else { */
            Screen::Login {
                screen: Box::default(),
                error: None,
            };
        //};

        let cli_server = server.as_deref();
        let server_field_locked = entry_policy.should_lock_server_field(cli_server);
        let allow_server_unlock = entry_policy.can_unlock_server_field(cli_server);
        let allow_server_list = entry_policy.can_show_server_list();
        let allow_singleplayer = entry_policy.can_use_singleplayer();
        let allow_multiplayer = entry_policy.can_attempt_multiplayer();
        let multiplayer_host_kind =
            entry_policy.initial_multiplayer_host_kind(settings, cli_server);
        let dev_multiplayer_entries = entry_policy.dev_multiplayer_entries(settings, cli_server);
        let login_info = LoginInfo {
            username: settings.networking.username.clone(),
            password: String::new(),
            server: entry_policy.initial_server_field_value(settings, cli_server),
        };
        let selected_local_dedicated_instance_id =
            if matches!(multiplayer_host_kind, HostKind::DevLocalDedicated) {
                settings.networking.default_local_dedicated_instance_id
            } else {
                None
            };
        let selected_server_index = if allow_server_list && !server_field_locked {
            dev_multiplayer_entries.iter().position(|entry| {
                Self::entry_matches_target(
                    entry,
                    multiplayer_host_kind,
                    selected_local_dedicated_instance_id,
                    &login_info.server,
                )
            })
        } else {
            None
        };

        let language_metadatas = i18n::list_localizations();
        let selected_language_index = language_metadatas
            .iter()
            .position(|f| f.language_identifier == settings.language.selected_language);

        Self {
            fonts,
            imgs,
            bg_img,
            i18n,
            version,
            credits,

            server_field_locked,
            allow_server_unlock,
            allow_server_list,
            allow_singleplayer,
            allow_multiplayer,
            entry_policy: entry_policy.clone(),
            cli_server: server,
            dev_multiplayer_entries,
            multiplayer_host_kind,
            selected_local_dedicated_instance_id,
            selected_server_index,
            login_info,

            show: Showing::Login,
            selected_language_index,

            time: 0.0,

            screen,
        }
    }

    fn view(
        &mut self,
        settings: &Settings,
        dt: f32,
        #[cfg(feature = "singleplayer")] worlds: &crate::singleplayer::SingleplayerWorlds,
    ) -> Element<'_, Message> {
        self.time += dt as f64;
        self.refresh_dev_multiplayer_entries(settings);

        // TODO: consider setting this as the default in the renderer
        let button_style = style::button::Style::new(self.imgs.button)
            .hover_image(self.imgs.button_hover)
            .press_image(self.imgs.button_press)
            .text_color(TEXT_COLOR)
            .disabled_text_color(DISABLED_TEXT_COLOR);

        let version = iced::Text::new(&self.version)
            .size(self.fonts.cyri.scale(12))
            .width(Length::Fill)
            .horizontal_alignment(HorizontalAlignment::Center);

        let top_text = Row::with_children(vec![
            Space::new(Length::Fill, Length::Shrink).into(),
            version.into(),
            Space::new(Length::Fill, Length::Shrink).into(),
        ])
        .padding(3)
        .width(Length::Fill);

        let bg_img = if matches!(&self.screen, Screen::Connecting { .. }) {
            self.bg_img
        } else {
            self.imgs.bg
        };

        let language_metadatas = i18n::list_localizations();

        // TODO: make any large text blocks scrollable so that if the area is to
        // small they can still be read
        let content = match &mut self.screen {
            // Note: Keeping in case we re-add the disclaimer
            //Screen::Disclaimer { screen } => screen.view(&self.fonts, &self.i18n, button_style),
            Screen::Credits { screen } => {
                screen.view(&self.fonts, &self.i18n.read(), &self.credits, button_style)
            },
            Screen::Login { screen, error } => screen.view(
                &self.fonts,
                &self.imgs,
                self.allow_server_list,
                self.allow_server_unlock,
                self.allow_singleplayer,
                self.allow_multiplayer,
                self.server_field_locked,
                &self.login_info,
                error.as_deref(),
                &self.i18n.read(),
                &self.show,
                self.selected_language_index,
                &language_metadatas,
                button_style,
            ),
            Screen::Servers { screen } => screen.view(
                &self.fonts,
                &self.imgs,
                &self.dev_multiplayer_entries,
                self.selected_server_index,
                self.selected_server_index
                    .and_then(|index| self.dev_multiplayer_entries.get(index))
                    .is_some_and(|entry| matches!(entry.host_kind, HostKind::DevLocalDedicated)),
                self.selected_server_index
                    .and_then(|index| self.dev_multiplayer_entries.get(index))
                    .is_some_and(|entry| entry.can_register_local_dedicated),
                self.selected_server_index
                    .and_then(|index| self.dev_multiplayer_entries.get(index))
                    .is_some_and(|entry| entry.can_delete),
                &self.i18n.read(),
                button_style,
            ),
            Screen::LocalDedicatedEditor { screen } => {
                screen.view(&self.fonts, &self.imgs, &self.i18n.read(), button_style)
            },
            Screen::Connecting {
                screen,
                connection_state,
                init_stage,
                notice,
            } => screen.view(
                &self.fonts,
                &self.imgs,
                connection_state,
                init_stage,
                notice.as_deref(),
                self.time,
                &self.i18n.read(),
                button_style,
                settings.interface.loading_tips,
                &settings.controls,
            ),
            #[cfg(feature = "singleplayer")]
            Screen::WorldSelector { screen } => screen.view(
                &self.fonts,
                &self.imgs,
                worlds,
                &self.i18n.read(),
                button_style,
            ),
        };

        Container::new(
            Column::with_children(vec![top_text.into(), content])
                .spacing(3)
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .style(style::container::Style::image(bg_img))
        .into()
    }

    fn update(
        &mut self,
        message: Message,
        events: &mut Vec<Event>,
        settings: &Settings,
        ui: &mut Ui,
    ) {
        self.refresh_dev_multiplayer_entries(settings);
        let mut language_metadatas = i18n::list_localizations();

        match message {
            Message::Quit => events.push(Event::Quit),
            Message::Back => {
                self.screen = if matches!(&self.screen, Screen::LocalDedicatedEditor { .. }) {
                    Screen::Servers {
                        screen: servers::Screen::new(),
                    }
                } else {
                    Screen::Login {
                        screen: Box::default(),
                        error: None,
                    }
                };
            },
            Message::ShowServers => {
                if self.allow_server_list && matches!(&self.screen, Screen::Login { .. }) {
                    self.selected_server_index =
                        self.dev_multiplayer_entries.iter().position(|entry| {
                            Self::entry_matches_target(
                                entry,
                                self.multiplayer_host_kind,
                                self.selected_local_dedicated_instance_id,
                                &self.login_info.server,
                            )
                        });
                    self.screen = Screen::Servers {
                        screen: servers::Screen::new(),
                    };
                }
            },
            Message::ShowCredits => {
                self.screen = Screen::Credits {
                    screen: credits::Screen::new(),
                };
            },
            #[cfg(feature = "singleplayer")]
            Message::Singleplayer => {
                if self.allow_singleplayer {
                    self.screen = Screen::WorldSelector {
                        screen: world_selector::Screen::default(),
                    };
                    events.push(Event::InitSingleplayer);
                }
            },
            #[cfg(feature = "singleplayer")]
            Message::SingleplayerPlay => {
                if self.allow_singleplayer {
                    self.screen = Screen::Connecting {
                        screen: connecting::Screen::new(ui),
                        connection_state: ConnectionState::InProgress,
                        init_stage: DetailedInitializationStage::PreparingHost(
                            HostKind::DevSingleplayer,
                        ),
                        notice: None,
                    };
                    events.push(Event::StartSingleplayer);
                }
            },
            #[cfg(feature = "singleplayer")]
            Message::WorldChanged(change) => {
                match change {
                    WorldsChange::Delete(_) | WorldsChange::Regenerate(_) => {
                        if let Screen::WorldSelector {
                            screen: world_selector::Screen { confirmation, .. },
                        } = &mut self.screen
                        {
                            *confirmation = None;
                        }
                    },
                    _ => {},
                }
                events.push(Event::SinglePlayerChange(change))
            },
            #[cfg(feature = "singleplayer")]
            Message::WorldCancelConfirmation => {
                if let Screen::WorldSelector {
                    screen: world_selector::Screen { confirmation, .. },
                } = &mut self.screen
                {
                    *confirmation = None;
                }
            },
            #[cfg(feature = "singleplayer")]
            Message::WorldConfirmation(new_confirmation) => {
                if let Screen::WorldSelector {
                    screen: world_selector::Screen { confirmation, .. },
                } = &mut self.screen
                {
                    *confirmation = Some(new_confirmation);
                }
            },
            Message::Multiplayer => {
                if self.allow_multiplayer {
                    self.screen = Screen::Connecting {
                        screen: connecting::Screen::new(ui),
                        connection_state: ConnectionState::InProgress,
                        init_stage: DetailedInitializationStage::PreparingHost(
                            self.multiplayer_host_kind,
                        ),
                        notice: None,
                    };

                    events.push(Event::LoginAttempt {
                        username: self.login_info.username.trim().to_string(),
                        password: self.login_info.password.clone(),
                        server_address: self.login_info.server.trim().to_string(),
                        local_dedicated_instance_id: matches!(
                            self.multiplayer_host_kind,
                            HostKind::DevLocalDedicated
                        )
                        .then_some(self.selected_local_dedicated_instance_id)
                        .flatten(),
                        host_kind: self.multiplayer_host_kind,
                    });
                }
            },
            Message::UnlockServerField => {
                if self.allow_server_unlock {
                    self.server_field_locked = false;
                    self.multiplayer_host_kind = HostKind::DevDirectConnect;
                    self.selected_local_dedicated_instance_id = None;
                }
            },
            Message::Username(new_value) => {
                self.login_info.username = sanitize_ascii_input(new_value);
            },
            Message::LanguageChanged(new_value) => {
                events.push(Event::ChangeLanguage(language_metadatas.remove(new_value)));
            },
            Message::OpenLanguageMenu => self.show.toggle(Showing::Languages),
            Message::Password(new_value) => {
                self.login_info.password = sanitize_ascii_input(new_value);
            },
            Message::Server(new_value) => {
                self.login_info.server = sanitize_ascii_input(new_value);
                if self.entry_policy.is_dev() {
                    self.multiplayer_host_kind = HostKind::DevDirectConnect;
                    self.selected_local_dedicated_instance_id = None;
                    self.selected_server_index = None;
                }
            },
            Message::ServerChanged(new_value) => {
                if self.allow_server_list && new_value < self.dev_multiplayer_entries.len() {
                    self.selected_server_index = Some(new_value);
                    self.login_info
                        .server
                        .clone_from(&self.dev_multiplayer_entries[new_value].server_address);
                    self.multiplayer_host_kind = self.dev_multiplayer_entries[new_value].host_kind;
                    self.selected_local_dedicated_instance_id =
                        self.dev_multiplayer_entries[new_value].local_dedicated_instance_id;
                }
            },
            Message::FocusPassword => {
                if let Screen::Login { screen, .. } = &mut self.screen {
                    screen.banner.password = text_input::State::focused();
                    screen.banner.username = text_input::State::new();
                }
            },
            Message::CancelConnect => {
                self.exit_connect_screen();
                events.push(Event::CancelLoginAttempt);
            },
            msg @ Message::TrustPromptAdd | msg @ Message::TrustPromptCancel => {
                if let Screen::Connecting {
                    connection_state, ..
                } = &mut self.screen
                    && let ConnectionState::AuthTrustPrompt { auth_server, .. } = connection_state
                {
                    let auth_server = std::mem::take(auth_server);
                    let added = matches!(msg, Message::TrustPromptAdd);

                    *connection_state = ConnectionState::InProgress;
                    events.push(Event::AuthServerTrust(auth_server, added));
                }
            },
            Message::CloseError => {
                if let Screen::Login { error, .. } = &mut self.screen {
                    *error = None;
                }
            },
            Message::DeleteServer => {
                if self.allow_server_list
                    && let Some(selected_index) = self.selected_server_index
                {
                    if let Some(server_index) =
                        self.direct_history_index_for_selection(selected_index)
                    {
                        events.push(Event::DeleteServer { server_index });
                        self.selected_server_index = None;
                    } else if let Some(instance_id) =
                        self.local_dedicated_instance_id_for_selection(selected_index)
                    {
                        events.push(Event::DeleteLocalDedicated { instance_id });
                        self.selected_server_index = None;
                        self.selected_local_dedicated_instance_id = None;
                    }
                }
            },
            Message::AddLocalDedicated => {
                if self.allow_server_list
                    && let Some(server_address) =
                        self.direct_connect_address_for_local_registration()
                {
                    events.push(Event::RegisterLocalDedicated { server_address });
                }
            },
            Message::EditLocalDedicated => {
                if self.allow_server_list
                    && let Some(instance_id) = self.local_dedicated_instance_id_for_editing()
                    && let Some(entry) = settings
                        .networking
                        .local_dedicated_server_by_instance_id(instance_id)
                {
                    self.screen = Screen::LocalDedicatedEditor {
                        screen: Box::new(local_dedicated::Screen::from_entry(entry)),
                    };
                }
            },
            Message::LocalDedicatedDisplayName(new_value) => {
                if let Screen::LocalDedicatedEditor { screen } = &mut self.screen {
                    screen.display_name = sanitize_open_text_input(new_value);
                }
            },
            Message::LocalDedicatedServerAddress(new_value) => {
                if let Screen::LocalDedicatedEditor { screen } = &mut self.screen {
                    screen.server_address = sanitize_ascii_input(new_value);
                }
            },
            Message::LocalDedicatedConnectionKind(new_kind) => {
                if let Screen::LocalDedicatedEditor { screen } = &mut self.screen {
                    screen.connection_kind = new_kind;
                    if matches!(
                        screen.connection_kind,
                        crate::settings::LocalDedicatedConnectionKind::Tcp
                    ) {
                        screen.validate_tls = false;
                    }
                }
            },
            Message::ToggleLocalDedicatedTls => {
                if let Screen::LocalDedicatedEditor { screen } = &mut self.screen
                    && matches!(
                        screen.connection_kind,
                        crate::settings::LocalDedicatedConnectionKind::Quic
                    )
                {
                    screen.validate_tls = !screen.validate_tls;
                }
            },
            Message::SaveLocalDedicatedEditor => {
                if let Screen::LocalDedicatedEditor { screen } = &mut self.screen {
                    let server_address = screen.server_address.trim().to_string();
                    if !server_address.is_empty() {
                        self.login_info.server.clone_from(&server_address);
                        self.multiplayer_host_kind = HostKind::DevLocalDedicated;
                        self.selected_local_dedicated_instance_id = Some(screen.instance_id);
                        events.push(Event::UpdateLocalDedicated {
                            instance_id: screen.instance_id,
                            display_name: screen.display_name.trim().to_string(),
                            server_address,
                            connection_kind: screen.connection_kind,
                            validate_tls: matches!(
                                screen.connection_kind,
                                crate::settings::LocalDedicatedConnectionKind::Quic
                            ) && screen.validate_tls,
                        });
                    }
                }
                self.screen = Screen::Servers {
                    screen: servers::Screen::new(),
                };
            },
            /* Note: Keeping in case we re-add the disclaimer */
            /*Message::AcceptDisclaimer => {
                if let Screen::Disclaimer { .. } = &self.screen {
                    events.push(Event::DisclaimerAccepted);
                    self.screen = Screen::Login {
                        screen: login::Screen::default(),
                        error: None,
                    };
                }
            },*/
        }
    }

    // Connection successful of failed
    fn exit_connect_screen(&mut self) {
        if matches!(&self.screen, Screen::Connecting { .. }) {
            self.screen = Screen::Login {
                screen: Box::default(),
                error: None,
            }
        }
    }

    fn refresh_dev_multiplayer_entries(&mut self, settings: &Settings) {
        self.dev_multiplayer_entries = self
            .entry_policy
            .dev_multiplayer_entries(settings, self.cli_server.as_deref());

        if self
            .selected_server_index
            .is_some_and(|index| index >= self.dev_multiplayer_entries.len())
        {
            self.selected_server_index = None;
            if matches!(self.multiplayer_host_kind, HostKind::DevLocalDedicated) {
                self.selected_local_dedicated_instance_id = None;
            }
        }
    }

    fn entry_matches_target(
        entry: &DevMultiplayerEntry,
        host_kind: HostKind,
        local_dedicated_instance_id: Option<Uuid>,
        server_address: &str,
    ) -> bool {
        if entry.host_kind != host_kind {
            return false;
        }

        match host_kind {
            HostKind::DevLocalDedicated => {
                if let Some(instance_id) = local_dedicated_instance_id {
                    entry.local_dedicated_instance_id == Some(instance_id)
                } else {
                    entry.server_address == server_address
                }
            },
            _ => entry.server_address == server_address,
        }
    }

    fn direct_history_index_for_selection(&self, selected_index: usize) -> Option<usize> {
        let selected = self.dev_multiplayer_entries.get(selected_index)?;
        if !selected.can_delete || !matches!(selected.host_kind, HostKind::DevDirectConnect) {
            return None;
        }

        self.dev_multiplayer_entries
            .iter()
            .take(selected_index + 1)
            .filter(|entry| {
                entry.can_delete
                    && matches!(entry.host_kind, HostKind::DevDirectConnect)
                    && entry.server_address == selected.server_address
            })
            .count()
            .checked_sub(1)
    }

    fn direct_connect_address_for_local_registration(&self) -> Option<String> {
        let selected = self
            .selected_server_index
            .and_then(|index| self.dev_multiplayer_entries.get(index))?;
        if !selected.can_register_local_dedicated
            || !matches!(selected.host_kind, HostKind::DevDirectConnect)
        {
            return None;
        }

        Some(selected.server_address.clone())
    }

    fn local_dedicated_instance_id_for_selection(&self, selected_index: usize) -> Option<Uuid> {
        let selected = self.dev_multiplayer_entries.get(selected_index)?;
        if !selected.can_delete || !matches!(selected.host_kind, HostKind::DevLocalDedicated) {
            return None;
        }

        selected.local_dedicated_instance_id
    }

    fn local_dedicated_instance_id_for_editing(&self) -> Option<Uuid> {
        let selected = self
            .selected_server_index
            .and_then(|index| self.dev_multiplayer_entries.get(index))?;
        if !matches!(selected.host_kind, HostKind::DevLocalDedicated) {
            return None;
        }

        selected.local_dedicated_instance_id
    }

    fn auth_trust_prompt(&mut self, auth_server: String) {
        if let Screen::Connecting {
            connection_state, ..
        } = &mut self.screen
        {
            let msg = format!(
                "Warning: The server you are trying to connect to has provided this \
                 authentication server address:\n\n{}\n\nbut it is not in your list of trusted \
                 authentication servers.\n\nMake sure that you trust this site and owner to not \
                 try and bruteforce your password!",
                &auth_server
            );

            *connection_state = ConnectionState::AuthTrustPrompt { auth_server, msg };
        }
    }

    fn connection_error(&mut self, error: String) {
        if matches!(&self.screen, Screen::Connecting { .. })
            || matches!(&self.screen, Screen::Login { .. })
        {
            self.screen = Screen::Login {
                screen: Box::default(),
                error: Some(error),
            }
        } else {
            warn!("connection_error invoked on unhandled screen!");
        }
    }

    fn connection_notice(&mut self, notice: String) {
        if let Screen::Connecting { notice: slot, .. } = &mut self.screen {
            *slot = Some(notice);
        } else {
            warn!("connection_notice invoked on unhandled screen!");
        }
    }

    fn update_init_stage(&mut self, stage: DetailedInitializationStage) {
        if let Screen::Connecting { init_stage, .. } = &mut self.screen {
            *init_stage = stage
        }
    }

    fn tab(&mut self) {
        if let Screen::Login { screen, .. } = &mut self.screen {
            // TODO: add select all function in iced
            if screen.banner.username.is_focused() {
                screen.banner.username = text_input::State::new();
                screen.banner.password = text_input::State::focused();
                screen.banner.password.move_cursor_to_end();
            } else if screen.banner.password.is_focused() {
                screen.banner.password = text_input::State::new();
                // Skip focusing server field if it isn't editable!
                if self.server_field_locked {
                    screen.banner.username = text_input::State::focused();
                } else {
                    screen.banner.server = text_input::State::focused();
                }
                screen.banner.server.move_cursor_to_end();
            } else if screen.banner.server.is_focused() {
                screen.banner.server = text_input::State::new();
                screen.banner.username = text_input::State::focused();
                screen.banner.username.move_cursor_to_end();
            } else {
                screen.banner.username = text_input::State::focused();
                screen.banner.username.move_cursor_to_end();
            }
        }
    }

    fn active_text_input_target(
        &self,
        ui: &Ui,
        #[cfg(feature = "singleplayer")] worlds: &crate::singleplayer::SingleplayerWorlds,
    ) -> Option<TextInputTarget> {
        match &self.screen {
            Screen::Login { screen, .. } => screen.banner.active_text_input_target(
                ui,
                &self.fonts,
                &self.login_info,
                self.server_field_locked,
            ),
            Screen::LocalDedicatedEditor { screen } => {
                screen.active_text_input_target(ui, &self.fonts)
            },
            #[cfg(feature = "singleplayer")]
            Screen::WorldSelector { screen } => {
                screen.active_text_input_target(worlds, ui, &self.fonts)
            },
            _ => None,
        }
    }
}

fn sanitize_ascii_input(value: String) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii() && !c.is_control())
        .collect()
}

fn sanitize_open_text_input(value: String) -> String {
    value.chars().filter(|c| !c.is_control()).collect()
}

pub struct MainMenuUi {
    ui: Ui,
    // TODO: re add this
    // tip_no: u16,
    controls: Controls,
    bg_img_spec: &'static str,
}

impl MainMenuUi {
    pub fn new(global_state: &mut GlobalState) -> Self {
        if global_state
            .settings
            .networking
            .sync_default_local_dedicated_source(&global_state.userdata_dir)
        {
            global_state
                .settings
                .save_to_file_warn(&global_state.config_dir);
        }

        // Load language
        let i18n = &global_state.i18n.read();
        // TODO: don't add default font twice
        let font = load_font(&i18n.fonts().get("cyri").unwrap().asset_key);

        let mut ui = Ui::new(
            &mut global_state.window,
            font,
            global_state.settings.interface.ui_scale,
        )
        .unwrap();

        let fonts = Fonts::load(i18n.fonts(), &mut ui).expect("Impossible to load fonts");

        let bg_img_spec = rand_bg_image_spec();

        let bg_img = Image::load_expect(bg_img_spec).read().to_image();
        let controls = Controls::new(
            fonts,
            Imgs::load(&mut ui).expect("Failed to load images"),
            ui.add_graphic(Graphic::Image(bg_img, None)),
            global_state.i18n,
            &global_state.settings,
            &global_state.entry_policy,
            global_state.args.server.clone(),
        );

        Self {
            ui,
            controls,
            bg_img_spec,
        }
    }

    pub fn bg_img_spec(&self) -> &'static str { self.bg_img_spec }

    pub fn update_language(&mut self, i18n: LocalizationHandle, settings: &Settings) {
        self.controls.i18n = i18n;
        let i18n = &i18n.read();
        let font = load_font(&i18n.fonts().get("cyri").unwrap().asset_key);
        self.ui.clear_fonts(font);
        self.controls.fonts =
            Fonts::load(i18n.fonts(), &mut self.ui).expect("Impossible to load fonts!");
        let language_metadatas = i18n::list_localizations();
        self.controls.selected_language_index = language_metadatas
            .iter()
            .position(|f| f.language_identifier == settings.language.selected_language);
    }

    pub fn auth_trust_prompt(&mut self, auth_server: String) {
        self.controls.auth_trust_prompt(auth_server);
    }

    pub fn show_info(&mut self, msg: String) { self.controls.connection_error(msg); }

    pub fn show_connection_notice(&mut self, msg: String) { self.controls.connection_notice(msg); }

    pub fn update_stage(&mut self, stage: DetailedInitializationStage) {
        tracing::trace!(?stage, "Updating stage");
        self.controls.update_init_stage(stage);
    }

    pub fn connected(&mut self) { self.controls.exit_connect_screen(); }

    pub fn cancel_connection(&mut self) { self.controls.exit_connect_screen(); }

    pub fn handle_event(&mut self, event: window::Event) -> bool {
        match event {
            // Pass events to ui.
            window::Event::IcedUi(event) => {
                self.handle_ui_event(event);
                true
            },
            window::Event::ScaleFactorChanged(s) => {
                self.ui.scale_factor_changed(s);
                false
            },
            _ => false,
        }
    }

    pub fn handle_ui_event(&mut self, event: ui::ice::Event) {
        // Tab for input fields
        use iced::keyboard;
        if matches!(
            &event,
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key_code: keyboard::KeyCode::Tab,
                ..
            })
        ) {
            self.controls.tab();
        }

        self.ui.handle_event(event);
    }

    pub fn set_scale_mode(&mut self, scale_mode: ui::ScaleMode) {
        self.ui.set_scaling_mode(scale_mode);
    }

    pub fn maintain(&mut self, global_state: &mut GlobalState, dt: Duration) -> Vec<Event> {
        let mut events = Vec::new();

        #[cfg(feature = "singleplayer")]
        let worlds_default = crate::singleplayer::SingleplayerWorlds::default();
        #[cfg(feature = "singleplayer")]
        let worlds = global_state
            .singleplayer
            .as_init()
            .unwrap_or(&worlds_default);

        global_state
            .window
            .set_text_input_target(self.controls.active_text_input_target(
                &self.ui,
                #[cfg(feature = "singleplayer")]
                worlds,
            ));

        let (messages, _) = self.ui.maintain(
            self.controls.view(
                &global_state.settings,
                dt.as_secs_f32(),
                #[cfg(feature = "singleplayer")]
                worlds,
            ),
            global_state.window.renderer_mut(),
            None,
            &mut global_state.clipboard,
        );

        messages.into_iter().for_each(|message| {
            self.controls
                .update(message, &mut events, &global_state.settings, &mut self.ui)
        });

        global_state
            .window
            .set_text_input_target(self.controls.active_text_input_target(
                &self.ui,
                #[cfg(feature = "singleplayer")]
                worlds,
            ));

        events
    }

    pub fn render<'a>(&'a self, drawer: &mut UiDrawer<'_, 'a>) { self.ui.render(drawer); }
}

pub fn rand_bg_image_spec() -> &'static str { BG_IMGS.choose(&mut rng()).unwrap() }
