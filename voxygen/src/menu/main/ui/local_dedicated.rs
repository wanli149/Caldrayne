use super::{FILL_FRAC_ONE, Imgs, Message};
use crate::{
    settings::{LocalDedicatedConnectionKind, LocalDedicatedServer},
    ui::{
        fonts::IcedFonts as Fonts,
        ice::{
            Element,
            component::neat_button,
            style,
            widget::{self, BackgroundContainer, Image, Padding},
        },
    },
    window::{TextInputPolicy, TextInputSource, TextInputTarget},
};
use i18n::Localization;
use iced::{Align, Column, Container, Length, Row, Text, TextInput, button, text_input};

const INPUT_WIDTH: u16 = 260;
const INPUT_TEXT_SIZE: u16 = 20;
const TRANSPORT_BUTTON_MAX_WIDTH: u32 = 150;
const ACTION_BUTTON_MAX_WIDTH: u32 = 180;

pub struct Screen {
    back_button: button::State,
    save_button: button::State,
    tcp_button: button::State,
    quic_button: button::State,
    tls_button: button::State,

    pub display_name_input: text_input::State,
    pub display_name_bounds: widget::BoundsState,
    pub server_input: text_input::State,
    pub server_bounds: widget::BoundsState,

    pub instance_id: common::uuid::Uuid,
    pub display_name: String,
    pub server_address: String,
    pub connection_kind: LocalDedicatedConnectionKind,
    pub validate_tls: bool,
    source_summary: String,
    data_dir_summary: String,
}

impl Screen {
    pub fn from_entry(entry: &LocalDedicatedServer) -> Self {
        let source_summary = format!(
            "Source: {:?}{}",
            entry.source_kind,
            if entry.manual_registration {
                " + manual override"
            } else {
                ""
            }
        );
        let data_dir_summary = format!(
            "Data Dir: {}",
            entry
                .data_dir
                .as_ref()
                .map(|path| path.to_string_lossy().to_string())
                .filter(|path| !path.is_empty())
                .unwrap_or_else(|| "None".to_string())
        );

        Self {
            back_button: Default::default(),
            save_button: Default::default(),
            tcp_button: Default::default(),
            quic_button: Default::default(),
            tls_button: Default::default(),
            display_name_input: Default::default(),
            display_name_bounds: Default::default(),
            server_input: Default::default(),
            server_bounds: Default::default(),
            instance_id: entry.instance_id,
            display_name: entry.display_name.clone(),
            server_address: entry.server_address.clone(),
            connection_kind: entry.connection_kind,
            validate_tls: entry.validate_tls,
            source_summary,
            data_dir_summary,
        }
    }

    pub(super) fn active_text_input_target(
        &self,
        ui: &crate::ui::ice::IcedUi,
        fonts: &Fonts,
    ) -> Option<TextInputTarget> {
        let input_text_size = fonts.cyri.scale(INPUT_TEXT_SIZE);

        if self.display_name_input.is_focused() {
            return Some(TextInputTarget {
                source: TextInputSource::Iced,
                policy: TextInputPolicy::OpenText,
                cursor_rect: ui
                    .text_input_cursor_rect(
                        &self.display_name_bounds,
                        &self.display_name_input,
                        &self.display_name,
                        fonts.cyri.id,
                        input_text_size,
                        false,
                    )
                    .or_else(|| ui.tracked_bounds_cursor_rect(&self.display_name_bounds)),
            });
        }

        if self.server_input.is_focused() {
            return Some(TextInputTarget {
                source: TextInputSource::Iced,
                policy: TextInputPolicy::StructuredAscii,
                cursor_rect: ui
                    .text_input_cursor_rect(
                        &self.server_bounds,
                        &self.server_input,
                        &self.server_address,
                        fonts.cyri.id,
                        input_text_size,
                        false,
                    )
                    .or_else(|| ui.tracked_bounds_cursor_rect(&self.server_bounds)),
            });
        }

        None
    }

    pub(super) fn view(
        &mut self,
        fonts: &Fonts,
        imgs: &Imgs,
        i18n: &Localization,
        button_style: style::button::Style,
    ) -> Element<'_, Message> {
        let input_text_size = fonts.cyri.scale(INPUT_TEXT_SIZE);

        let title = Text::new("Local Dedicated Override")
            .size(fonts.cyri.scale(32))
            .horizontal_alignment(iced::HorizontalAlignment::Center);

        let source_summary = Text::new(&self.source_summary).size(fonts.cyri.scale(16));
        let data_dir_summary = Text::new(&self.data_dir_summary).size(fonts.cyri.scale(16));

        let display_name_input = BackgroundContainer::new(
            Image::new(imgs.input_bg)
                .width(Length::Units(INPUT_WIDTH))
                .fix_aspect_ratio(),
            widget::TrackBounds::new(
                &self.display_name_bounds,
                TextInput::new(
                    &mut self.display_name_input,
                    "Display Name",
                    &self.display_name,
                    Message::LocalDedicatedDisplayName,
                )
                .size(input_text_size),
            ),
        )
        .padding(Padding::new().horizontal(7).top(5));

        let server_input = BackgroundContainer::new(
            Image::new(imgs.input_bg)
                .width(Length::Units(INPUT_WIDTH))
                .fix_aspect_ratio(),
            widget::TrackBounds::new(
                &self.server_bounds,
                TextInput::new(
                    &mut self.server_input,
                    &i18n.get_msg("main-server"),
                    &self.server_address,
                    Message::LocalDedicatedServerAddress,
                )
                .size(input_text_size),
            ),
        )
        .padding(Padding::new().horizontal(7).top(5));

        let tcp_button = neat_button(
            &mut self.tcp_button,
            "TCP",
            FILL_FRAC_ONE,
            button_style,
            Some(Message::LocalDedicatedConnectionKind(
                LocalDedicatedConnectionKind::Tcp,
            )),
        );
        let quic_button = neat_button(
            &mut self.quic_button,
            "QUIC",
            FILL_FRAC_ONE,
            button_style,
            Some(Message::LocalDedicatedConnectionKind(
                LocalDedicatedConnectionKind::Quic,
            )),
        );
        let tls_button = neat_button(
            &mut self.tls_button,
            if self.validate_tls {
                "TLS On"
            } else {
                "TLS Off"
            },
            FILL_FRAC_ONE,
            button_style,
            Some(Message::ToggleLocalDedicatedTls),
        );

        let back_button = neat_button(
            &mut self.back_button,
            i18n.get_msg("common-cancel"),
            FILL_FRAC_ONE,
            button_style,
            Some(Message::Back),
        );
        let save_button = neat_button(
            &mut self.save_button,
            i18n.get_msg("common-okay"),
            FILL_FRAC_ONE,
            button_style,
            Some(Message::SaveLocalDedicatedEditor),
        );

        let transport_buttons = Row::with_children(vec![
            Container::new(tcp_button)
                .center_x()
                .width(Length::FillPortion(1))
                .max_width(TRANSPORT_BUTTON_MAX_WIDTH)
                .into(),
            Container::new(quic_button)
                .center_x()
                .width(Length::FillPortion(1))
                .max_width(TRANSPORT_BUTTON_MAX_WIDTH)
                .into(),
            Container::new(tls_button)
                .center_x()
                .width(Length::FillPortion(1))
                .max_width(TRANSPORT_BUTTON_MAX_WIDTH)
                .into(),
        ])
        .spacing(8)
        .width(Length::Fill)
        .align_items(Align::Center);

        let action_buttons = Row::with_children(vec![
            Container::new(back_button)
                .center_x()
                .width(Length::FillPortion(1))
                .max_width(ACTION_BUTTON_MAX_WIDTH)
                .into(),
            Container::new(save_button)
                .center_x()
                .width(Length::FillPortion(1))
                .max_width(ACTION_BUTTON_MAX_WIDTH)
                .into(),
        ])
        .spacing(8)
        .width(Length::Fill)
        .align_items(Align::Center);

        Container::new(
            Container::new(
                Column::with_children(vec![
                    title.into(),
                    source_summary.into(),
                    data_dir_summary.into(),
                    Text::new("Display Name").size(fonts.cyri.scale(18)).into(),
                    display_name_input.into(),
                    Text::new(i18n.get_msg("main-server"))
                        .size(fonts.cyri.scale(18))
                        .into(),
                    server_input.into(),
                    Text::new("Transport").size(fonts.cyri.scale(18)).into(),
                    transport_buttons.into(),
                    action_buttons.into(),
                ])
                .spacing(10)
                .width(Length::Fill)
                .height(Length::Fill)
                .padding(20),
            )
            .style(
                style::container::Style::color_with_double_cornerless_border(
                    (22, 18, 16, 255).into(),
                    (11, 11, 11, 255).into(),
                    (54, 46, 38, 255).into(),
                ),
            )
            .max_width(520),
        )
        .width(Length::Fill)
        .align_x(Align::Center)
        .padding(80)
        .into()
    }
}
