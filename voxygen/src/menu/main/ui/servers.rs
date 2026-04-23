use super::{FILL_FRAC_ONE, Imgs, Message};
use crate::{
    entry::DevMultiplayerEntry,
    ui::{
        fonts::IcedFonts as Fonts,
        ice::{Element, component::neat_button, style},
    },
};
use i18n::Localization;
use iced::{
    Align, Button, Column, Container, Length, Row, Scrollable, Space, Text, button, scrollable,
};

pub struct Screen {
    back_button: button::State,
    edit_button: button::State,
    add_button: button::State,
    delete_button: button::State,
    server_buttons: Vec<button::State>,
    servers_list: scrollable::State,
}

impl Screen {
    pub fn new() -> Self {
        Self {
            back_button: Default::default(),
            edit_button: Default::default(),
            add_button: Default::default(),
            delete_button: Default::default(),
            server_buttons: vec![],
            servers_list: Default::default(),
        }
    }

    pub(super) fn view(
        &mut self,
        fonts: &Fonts,
        imgs: &Imgs,
        servers: &[DevMultiplayerEntry],
        selected_server_index: Option<usize>,
        can_edit_selected: bool,
        can_add_selected: bool,
        can_delete_selected: bool,
        i18n: &Localization,
        button_style: style::button::Style,
    ) -> Element<'_, Message> {
        let title = Text::new(i18n.get_msg("main-servers-select_server"))
            .size(fonts.cyri.scale(35))
            .width(Length::Fill)
            .horizontal_alignment(iced::HorizontalAlignment::Center);
        let helper = Text::new(
            "Developer host inventory: direct-connect history can be promoted into Local \
             Dedicated, and Local Dedicated entries can be edited here.",
        )
        .size(fonts.cyri.scale(16))
        .width(Length::Fill)
        .horizontal_alignment(iced::HorizontalAlignment::Center);

        let back_button = Container::new(
            Container::new(neat_button(
                &mut self.back_button,
                i18n.get_msg("common-back"),
                FILL_FRAC_ONE,
                button_style,
                Some(Message::Back),
            ))
            .max_width(200),
        )
        .width(Length::Fill)
        .align_x(Align::Center);

        let delete_button = Container::new(
            Container::new(neat_button(
                &mut self.delete_button,
                i18n.get_msg("common-delete_server"),
                FILL_FRAC_ONE,
                button_style,
                can_delete_selected.then_some(Message::DeleteServer),
            ))
            .max_width(200),
        )
        .width(Length::Fill)
        .align_x(Align::Center);

        let edit_button = Container::new(
            Container::new(neat_button(
                &mut self.edit_button,
                "Edit Local",
                FILL_FRAC_ONE,
                button_style,
                can_edit_selected.then_some(Message::EditLocalDedicated),
            ))
            .max_width(200),
        )
        .width(Length::Fill)
        .align_x(Align::Center);

        let add_button = Container::new(
            Container::new(neat_button(
                &mut self.add_button,
                "Register Local",
                FILL_FRAC_ONE,
                button_style,
                can_add_selected.then_some(Message::AddLocalDedicated),
            ))
            .max_width(200),
        )
        .width(Length::Fill)
        .align_x(Align::Center);

        let mut list = Scrollable::new(&mut self.servers_list)
            .spacing(8)
            .align_items(Align::Start)
            .width(Length::Fill)
            .height(Length::Fill);

        // Reset button states if servers were added / removed
        if self.server_buttons.len() != servers.len() {
            self.server_buttons = vec![Default::default(); servers.len()];
        }

        let list_items =
            self.server_buttons
                .iter_mut()
                .zip(servers)
                .enumerate()
                .map(|(i, (state, server))| {
                    let color = if Some(i) == selected_server_index {
                        (97, 255, 18)
                    } else {
                        (97, 97, 25)
                    };
                    let button = Button::new(
                        state,
                        Row::with_children(vec![
                            Space::new(Length::FillPortion(5), Length::Units(0)).into(),
                            Column::with_children(vec![
                                Text::new(&server.label)
                                    .size(fonts.cyri.scale(28))
                                    .width(Length::Fill)
                                    .vertical_alignment(iced::VerticalAlignment::Center)
                                    .into(),
                                Text::new(&server.kind_label)
                                    .size(fonts.cyri.scale(18))
                                    .width(Length::Fill)
                                    .into(),
                                Text::new(&server.detail)
                                    .size(fonts.cyri.scale(15))
                                    .width(Length::Fill)
                                    .into(),
                            ])
                            .spacing(4)
                            .width(Length::FillPortion(95))
                            .into(),
                        ]),
                    )
                    .style(
                        style::button::Style::new(imgs.selection)
                            .hover_image(imgs.selection_hover)
                            .press_image(imgs.selection_press)
                            .image_color(vek::Rgba::new(color.0, color.1, color.2, 255)),
                    )
                    .min_height(120)
                    .on_press(Message::ServerChanged(i));
                    Row::with_children(vec![
                        Space::new(Length::FillPortion(3), Length::Units(0)).into(),
                        button.width(Length::FillPortion(92)).into(),
                        Space::new(Length::FillPortion(5), Length::Units(0)).into(),
                    ])
                });

        for item in list_items {
            list = list.push(item);
        }

        Container::new(
            Container::new(
                Column::with_children(vec![
                    title.into(),
                    helper.into(),
                    list.into(),
                    Row::with_children(vec![
                        edit_button.into(),
                        add_button.into(),
                        delete_button.into(),
                        back_button.into(),
                    ])
                    .width(Length::Fill)
                    .into(),
                ])
                .width(Length::Fill)
                .height(Length::Fill)
                .spacing(10)
                .padding(20),
            )
            .style(
                style::container::Style::color_with_double_cornerless_border(
                    (22, 18, 16, 255).into(),
                    (11, 11, 11, 255).into(),
                    (54, 46, 38, 255).into(),
                ),
            )
            .max_width(500),
        )
        .width(Length::Fill)
        .align_x(Align::Center)
        .padding(80)
        .into()
    }
}
