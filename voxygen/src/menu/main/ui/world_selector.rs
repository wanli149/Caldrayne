use common::resources::MapKind;
use i18n::Localization;
use iced::{
    Align, Button, Column, Container, Length, Row, Scrollable, Slider, Space, Text, TextInput,
    button, scrollable, slider, text_input,
};
use rand::RngExt;
use std::{fmt::Write, path::Path};
use vek::Rgba;

use crate::{
    menu::main::ui::{FILL_FRAC_TWO, WorldsChange},
    singleplayer::{
        SingleplayerLegacyInventory, SingleplayerLegacyOrigin, SingleplayerWorld,
        SingleplayerWorldSource,
    },
    ui::{
        fonts::IcedFonts,
        ice::{
            Element,
            component::neat_button,
            style,
            widget::{
                self, BackgroundContainer, Image, Overlay, Padding,
                compound_graphic::{CompoundGraphic, Graphic},
            },
        },
    },
    window::{TextInputPolicy, TextInputSource, TextInputTarget},
};

use super::{Imgs, Message};

const INPUT_TEXT_SIZE: u16 = 20;

#[derive(Clone)]
pub enum Confirmation {
    Regenerate(usize),
    Delete(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LegacyMetadataGapState {
    MissingTypedOrigin,
    MissingCompatAudit,
    MissingTypedOriginAndCompatAudit,
}

#[derive(Default)]
pub struct Screen {
    back_button: button::State,
    play_button: button::State,
    new_button: button::State,
    yes_button: button::State,
    no_button: button::State,

    worlds_buttons: Vec<button::State>,

    selection_list: scrollable::State,

    world_name: text_input::State,
    world_name_bounds: widget::BoundsState,
    map_seed: text_input::State,
    map_seed_bounds: widget::BoundsState,
    day_length: slider::State,
    random_seed_button: button::State,
    world_size_x: slider::State,
    world_size_y: slider::State,

    map_vertical_scale: slider::State,
    shape_buttons: enum_map::EnumMap<MapKind, button::State>,
    map_erosion_quality: slider::State,

    delete_world: button::State,
    regenerate_map: button::State,
    generate_map: button::State,

    pub confirmation: Option<Confirmation>,
}

impl Screen {
    pub(super) fn active_text_input_target(
        &self,
        worlds: &crate::singleplayer::SingleplayerWorlds,
        ui: &crate::ui::ice::IcedUi,
        fonts: &IcedFonts,
    ) -> Option<TextInputTarget> {
        let world = worlds.current.and_then(|i| worlds.worlds.get(i))?;
        let can_edit = !world.is_generated;
        let input_text_size = fonts.cyri.scale(INPUT_TEXT_SIZE);

        if self.world_name.is_focused() {
            return Some(TextInputTarget {
                source: TextInputSource::Iced,
                policy: TextInputPolicy::OpenText,
                cursor_rect: ui
                    .text_input_cursor_rect(
                        &self.world_name_bounds,
                        &self.world_name,
                        &world.name,
                        fonts.cyri.id,
                        input_text_size,
                        false,
                    )
                    .or_else(|| ui.tracked_bounds_cursor_rect(&self.world_name_bounds)),
            });
        }

        if can_edit && self.map_seed.is_focused() {
            return Some(TextInputTarget {
                source: TextInputSource::Iced,
                policy: TextInputPolicy::NumericOnly,
                cursor_rect: ui
                    .text_input_cursor_rect(
                        &self.map_seed_bounds,
                        &self.map_seed,
                        &world.seed.to_string(),
                        fonts.cyri.id,
                        input_text_size,
                        false,
                    )
                    .or_else(|| ui.tracked_bounds_cursor_rect(&self.map_seed_bounds)),
            });
        }

        None
    }

    pub(super) fn view(
        &mut self,
        fonts: &IcedFonts,
        imgs: &Imgs,
        worlds: &crate::singleplayer::SingleplayerWorlds,
        i18n: &Localization,
        button_style: style::button::Style,
    ) -> Element<'_, Message> {
        let input_text_size = fonts.cyri.scale(INPUT_TEXT_SIZE);

        let worlds_count = worlds.worlds.len();
        if self.worlds_buttons.len() != worlds_count {
            self.worlds_buttons = vec![Default::default(); worlds_count];
        }

        let title = Text::new(i18n.get_msg("gameinput-map"))
            .size(fonts.cyri.scale(35))
            .horizontal_alignment(iced::HorizontalAlignment::Center);

        let mut list = Scrollable::new(&mut self.selection_list)
            .spacing(8)
            .height(Length::Fill)
            .align_items(Align::Start);

        let list_items = self
            .worlds_buttons
            .iter_mut()
            .zip(
                worlds
                    .worlds
                    .iter()
                    .enumerate()
                    .map(|(i, w)| (Some(i), &w.name)),
            )
            .map(|(state, (i, map))| {
                let world = &worlds.worlds[i.expect("list item index should exist")];
                let color = if i == worlds.current {
                    (97, 255, 18)
                } else {
                    (97, 97, 25)
                };
                let mut label_children = vec![
                    Text::new(map)
                        .width(Length::FillPortion(95))
                        .size(fonts.cyri.scale(25))
                        .vertical_alignment(iced::VerticalAlignment::Center)
                        .into(),
                ];
                if Self::should_show_legacy_gap_badge(i, worlds.current) {
                    if let Some(legacy_gap_badge_text) = Self::legacy_gap_badge_text(world, i18n) {
                        label_children.push(
                            Text::new(legacy_gap_badge_text)
                                .width(Length::Shrink)
                                .size(fonts.cyri.scale(16))
                                .color([0.84, 0.74, 0.50])
                                .vertical_alignment(iced::VerticalAlignment::Center)
                                .into(),
                        );
                    }
                    if let Some(managed_recipe_sidecar_missing_badge_text) =
                        Self::managed_recipe_sidecar_missing_badge_text(world, i18n)
                    {
                        label_children.push(
                            Text::new(managed_recipe_sidecar_missing_badge_text)
                                .width(Length::Shrink)
                                .size(fonts.cyri.scale(16))
                                .color([0.84, 0.74, 0.50])
                                .vertical_alignment(iced::VerticalAlignment::Center)
                                .into(),
                        );
                    }
                }
                let button = Button::new(
                    state,
                    Row::with_children(vec![
                        Space::new(Length::FillPortion(5), Length::Units(0)).into(),
                        Row::with_children(label_children)
                            .spacing(8)
                            .width(Length::FillPortion(95))
                            .into(),
                    ]),
                )
                .style(
                    style::button::Style::new(imgs.selection)
                        .hover_image(imgs.selection_hover)
                        .press_image(imgs.selection_press)
                        .image_color(Rgba::new(color.0, color.1, color.2, 192)),
                )
                .min_height(56)
                .on_press(Message::WorldChanged(super::WorldsChange::SetActive(i)));
                Row::with_children(vec![
                    Space::new(Length::FillPortion(3), Length::Units(0)).into(),
                    button.width(Length::FillPortion(92)).into(),
                    Space::new(Length::FillPortion(5), Length::Units(0)).into(),
                ])
            });

        for item in list_items {
            list = list.push(item);
        }

        let new_button = Container::new(neat_button(
            &mut self.new_button,
            i18n.get_msg("main-singleplayer-new"),
            FILL_FRAC_TWO,
            button_style,
            Some(Message::WorldChanged(super::WorldsChange::AddNew)),
        ))
        .center_x()
        .max_width(200);

        let back_button = Container::new(neat_button(
            &mut self.back_button,
            i18n.get_msg("common-back"),
            FILL_FRAC_TWO,
            button_style,
            Some(Message::Back),
        ))
        .center_x()
        .max_width(200);

        let mut selection_menu_content = vec![title.into(), list.into()];

        if let Some(legacy_inventory_text) = Self::legacy_inventory_summary_text(worlds, i18n) {
            selection_menu_content.push(
                Text::new(legacy_inventory_text)
                    .size(fonts.cyri.scale(16))
                    .width(Length::Fill)
                    .color([0.72, 0.69, 0.58])
                    .horizontal_alignment(iced::HorizontalAlignment::Center)
                    .into(),
            );
        }

        selection_menu_content.push(new_button.into());
        selection_menu_content.push(back_button.into());

        let content = Column::with_children(selection_menu_content)
            .spacing(8)
            .width(Length::Fill)
            .height(Length::FillPortion(38))
            .align_items(Align::Center)
            .padding(iced::Padding {
                bottom: 25,
                ..iced::Padding::new(0)
            });

        let selection_menu = BackgroundContainer::new(
            CompoundGraphic::from_graphics(vec![
                Graphic::image(imgs.banner_top, [138, 17], [0, 0]),
                Graphic::rect(Rgba::new(0, 0, 0, 230), [130, 300], [4, 17]),
                // TODO: use non image gradient
                Graphic::gradient(Rgba::new(0, 0, 0, 230), Rgba::zero(), [130, 50], [4, 182]),
            ])
            .fix_aspect_ratio()
            .height(Length::Fill)
            .width(Length::Fill),
            content,
        )
        .padding(Padding::new().horizontal(5).top(15));
        let mut items = vec![selection_menu.into()];

        if let Some(i) = worlds.current {
            let world = &worlds.worlds[i];
            let can_edit = !world.is_generated;
            let message = |m| Message::WorldChanged(super::WorldsChange::CurrentWorldChange(m));

            use super::WorldChange;

            const SLIDER_TEXT_SIZE: u16 = 20;
            const SLIDER_CURSOR_SIZE: (u16, u16) = (9, 21);
            const SLIDER_BAR_HEIGHT: u16 = 9;
            const SLIDER_BAR_PAD: u16 = 0;
            // Height of interactable area
            const SLIDER_HEIGHT: u16 = 30;
            // Day length slider values
            pub const DAY_LENGTH_MIN: f64 = 10.0;
            pub const DAY_LENGTH_MAX: f64 = 60.0;

            let mut gen_content = vec![
                BackgroundContainer::new(
                    Image::new(imgs.input_bg)
                        .width(Length::Units(230))
                        .fix_aspect_ratio(),
                    Element::from(widget::TrackBounds::new(
                        &self.world_name_bounds,
                        TextInput::new(
                            &mut self.world_name,
                            &i18n.get_msg("main-singleplayer-world_name"),
                            &world.name,
                            move |s| message(WorldChange::Name(s)),
                        )
                        .size(input_text_size),
                    )),
                )
                .padding(Padding::new().horizontal(7).top(5))
                .into(),
            ];

            let seed = world.seed;
            let seed_str = i18n.get_msg("main-singleplayer-seed");
            let mut seed_content = vec![
                Column::with_children(vec![
                    Text::new(seed_str.to_string())
                        .size(SLIDER_TEXT_SIZE)
                        .horizontal_alignment(iced::HorizontalAlignment::Center)
                        .into(),
                ])
                .padding(iced::Padding::new(5))
                .into(),
                BackgroundContainer::new(
                    Image::new(imgs.input_bg)
                        .width(Length::Units(190))
                        .fix_aspect_ratio(),
                    if can_edit {
                        Element::from(widget::TrackBounds::new(
                            &self.map_seed_bounds,
                            TextInput::new(
                                &mut self.map_seed,
                                &seed_str,
                                &seed.to_string(),
                                move |s| {
                                    if let Ok(seed) = if s.is_empty() {
                                        Ok(0)
                                    } else {
                                        s.parse::<u32>()
                                    } {
                                        message(WorldChange::Seed(seed))
                                    } else {
                                        message(WorldChange::Seed(seed))
                                    }
                                },
                            )
                            .size(input_text_size),
                        ))
                    } else {
                        Text::new(world.seed.to_string())
                            .size(input_text_size)
                            .width(Length::Fill)
                            .height(Length::Shrink)
                            .into()
                    },
                )
                .padding(Padding::new().horizontal(7).top(5))
                .into(),
            ];

            if can_edit {
                seed_content.push(
                    Container::new(neat_button(
                        &mut self.random_seed_button,
                        i18n.get_msg("main-singleplayer-random_seed"),
                        FILL_FRAC_TWO,
                        button_style,
                        Some(message(WorldChange::Seed(rand::rng().random()))),
                    ))
                    .max_width(200)
                    .into(),
                )
            }

            gen_content.push(Row::with_children(seed_content).into());

            if let Some(provenance_text) = Self::provenance_text(world, i18n) {
                gen_content.push(
                    Text::new(provenance_text)
                        .size(fonts.cyri.scale(18))
                        .width(Length::Fill)
                        .color([0.78, 0.75, 0.62])
                        .horizontal_alignment(iced::HorizontalAlignment::Center)
                        .into(),
                );
            }

            if let Some(legacy_gap_text) = Self::legacy_metadata_gap_text(world, i18n) {
                gen_content.push(
                    Text::new(legacy_gap_text)
                        .size(fonts.cyri.scale(16))
                        .width(Length::Fill)
                        .color([0.81, 0.73, 0.57])
                        .horizontal_alignment(iced::HorizontalAlignment::Center)
                        .into(),
                );
            }

            if let Some(managed_residual_text) =
                Self::managed_recipe_sidecar_missing_text(world, i18n)
            {
                gen_content.push(
                    Text::new(managed_residual_text)
                        .size(fonts.cyri.scale(16))
                        .width(Length::Fill)
                        .color([0.81, 0.73, 0.57])
                        .horizontal_alignment(iced::HorizontalAlignment::Center)
                        .into(),
                );
            }

            if let Some(gen_opts) = world.gen_opts.as_ref() {
                // Day length setting label
                gen_content.push(
                    Text::new(format!(
                        "{}: {}",
                        i18n.get_msg("main-singleplayer-day_length"),
                        world.day_length
                    ))
                    .size(SLIDER_TEXT_SIZE)
                    .horizontal_alignment(iced::HorizontalAlignment::Center)
                    .into(),
                );

                // Day length setting slider
                if can_edit {
                    gen_content.push(
                        Row::with_children(vec![
                            Slider::new(
                                &mut self.day_length,
                                DAY_LENGTH_MIN..=DAY_LENGTH_MAX,
                                world.day_length,
                                move |d| message(WorldChange::DayLength(d)),
                            )
                            .height(SLIDER_HEIGHT)
                            .style(style::slider::Style::images(
                                imgs.slider_indicator,
                                imgs.slider_range,
                                SLIDER_BAR_PAD,
                                SLIDER_CURSOR_SIZE,
                                SLIDER_BAR_HEIGHT,
                            ))
                            .into(),
                        ])
                        .into(),
                    )
                }

                gen_content.push(
                    Text::new(format!(
                        "{}: x: {}, y: {}",
                        i18n.get_msg("main-singleplayer-size_lg"),
                        gen_opts.x_lg,
                        gen_opts.y_lg
                    ))
                    .size(SLIDER_TEXT_SIZE)
                    .horizontal_alignment(iced::HorizontalAlignment::Center)
                    .into(),
                );

                if can_edit {
                    gen_content.push(
                        Row::with_children(vec![
                            Slider::new(&mut self.world_size_x, 4..=13, gen_opts.x_lg, move |s| {
                                message(WorldChange::SizeX(s))
                            })
                            .height(SLIDER_HEIGHT)
                            .style(style::slider::Style::images(
                                imgs.slider_indicator,
                                imgs.slider_range,
                                SLIDER_BAR_PAD,
                                SLIDER_CURSOR_SIZE,
                                SLIDER_BAR_HEIGHT,
                            ))
                            .into(),
                            Slider::new(&mut self.world_size_y, 4..=13, gen_opts.y_lg, move |s| {
                                message(WorldChange::SizeY(s))
                            })
                            .height(SLIDER_HEIGHT)
                            .style(style::slider::Style::images(
                                imgs.slider_indicator,
                                imgs.slider_range,
                                SLIDER_BAR_PAD,
                                SLIDER_CURSOR_SIZE,
                                SLIDER_BAR_HEIGHT,
                            ))
                            .into(),
                        ])
                        .into(),
                    );
                    let height = Length::Units(86);
                    if gen_opts.x_lg + gen_opts.y_lg >= 19 {
                        let mut msg = i18n
                            .get_msg("main-singleplayer-map_large_warning")
                            .into_owned();
                        let default_ops = server::GenOpts::default();
                        if let Some(s) = (gen_opts.x_lg + gen_opts.y_lg)
                            .checked_sub(default_ops.x_lg + default_ops.y_lg)
                        {
                            // Memory usages would still be more even if `erosion_quality`
                            // is less than 1.0 so only multiply by that if it's greater
                            // than 1.
                            let count = ((1 << s) as f32 * gen_opts.erosion_quality.max(1.0))
                                .round() as u32;
                            if count > 1 {
                                msg.push(' ');
                                msg.push_str(&i18n.get_msg_ctx(
                                    "main-singleplayer-map_large_extra_warning",
                                    &i18n::fluent_args! {
                                        "count" => count,
                                    },
                                ));
                            }
                        }
                        gen_content.push(
                            Text::new(msg)
                                .size(SLIDER_TEXT_SIZE)
                                .height(height)
                                .color([0.914, 0.835, 0.008])
                                .horizontal_alignment(iced::HorizontalAlignment::Center)
                                .into(),
                        );
                    } else {
                        gen_content.push(Space::new(Length::Units(0), height).into());
                    }
                }

                gen_content.push(
                    Text::new(format!(
                        "{}: {}",
                        i18n.get_msg("main-singleplayer-map_scale"),
                        gen_opts.scale
                    ))
                    .size(SLIDER_TEXT_SIZE)
                    .horizontal_alignment(iced::HorizontalAlignment::Center)
                    .into(),
                );

                if can_edit {
                    gen_content.push(
                        Slider::new(
                            &mut self.map_vertical_scale,
                            0.1..=160.0,
                            gen_opts.scale * 10.0,
                            move |s| message(WorldChange::Scale(s / 10.0)),
                        )
                        .height(SLIDER_HEIGHT)
                        .style(style::slider::Style::images(
                            imgs.slider_indicator,
                            imgs.slider_range,
                            SLIDER_BAR_PAD,
                            SLIDER_CURSOR_SIZE,
                            SLIDER_BAR_HEIGHT,
                        ))
                        .into(),
                    );
                }

                if can_edit {
                    gen_content.extend([
                        Text::new(i18n.get_msg("main-singleplayer-map_shape"))
                            .size(SLIDER_TEXT_SIZE)
                            .horizontal_alignment(iced::HorizontalAlignment::Center)
                            .into(),
                        Row::with_children(
                            self.shape_buttons
                                .iter_mut()
                                .map(|(shape, state)| {
                                    let color = if gen_opts.map_kind == shape {
                                        (97, 255, 18)
                                    } else {
                                        (97, 97, 25)
                                    };
                                    Button::new(
                                        state,
                                        Row::with_children(vec![
                                            Space::new(Length::FillPortion(5), Length::Units(0))
                                                .into(),
                                            Text::new(i18n.get_msg(Self::map_kind_key(shape)))
                                                .width(Length::FillPortion(95))
                                                .size(fonts.cyri.scale(14))
                                                .vertical_alignment(iced::VerticalAlignment::Center)
                                                .into(),
                                        ])
                                        .align_items(Align::Center),
                                    )
                                    .style(
                                        style::button::Style::new(imgs.selection)
                                            .hover_image(imgs.selection_hover)
                                            .press_image(imgs.selection_press)
                                            .image_color(Rgba::new(color.0, color.1, color.2, 192)),
                                    )
                                    .width(Length::FillPortion(1))
                                    .min_height(18)
                                    .on_press(Message::WorldChanged(
                                        super::WorldsChange::CurrentWorldChange(
                                            WorldChange::MapKind(shape),
                                        ),
                                    ))
                                    .into()
                                })
                                .collect(),
                        )
                        .into(),
                    ]);
                } else {
                    gen_content.push(
                        Text::new(format!(
                            "{}: {}",
                            i18n.get_msg("main-singleplayer-map_shape"),
                            gen_opts.map_kind,
                        ))
                        .size(SLIDER_TEXT_SIZE)
                        .horizontal_alignment(iced::HorizontalAlignment::Center)
                        .into(),
                    );
                }

                gen_content.push(
                    Text::new(format!(
                        "{}: {}",
                        i18n.get_msg("main-singleplayer-map_erosion_quality"),
                        gen_opts.erosion_quality
                    ))
                    .size(SLIDER_TEXT_SIZE)
                    .horizontal_alignment(iced::HorizontalAlignment::Center)
                    .into(),
                );

                if can_edit {
                    gen_content.push(
                        Slider::new(
                            &mut self.map_erosion_quality,
                            0.0..=20.0,
                            gen_opts.erosion_quality * 10.0,
                            move |s| message(WorldChange::ErosionQuality(s / 10.0)),
                        )
                        .height(SLIDER_HEIGHT)
                        .style(style::slider::Style::images(
                            imgs.slider_indicator,
                            imgs.slider_range,
                            SLIDER_BAR_PAD,
                            SLIDER_CURSOR_SIZE,
                            SLIDER_BAR_HEIGHT,
                        ))
                        .into(),
                    );
                }
            }

            let mut world_buttons = vec![];

            if world.gen_opts.is_none() && can_edit {
                let create_custom = Container::new(neat_button(
                    &mut self.regenerate_map,
                    i18n.get_msg("main-singleplayer-create_custom"),
                    FILL_FRAC_TWO,
                    button_style,
                    Some(Message::WorldChanged(
                        super::WorldsChange::CurrentWorldChange(WorldChange::DefaultGenOps),
                    )),
                ))
                .center_x()
                .width(Length::FillPortion(1))
                .max_width(200);
                world_buttons.push(create_custom.into());
            }

            if world.is_generated {
                let regenerate = Container::new(neat_button(
                    &mut self.generate_map,
                    i18n.get_msg("main-singleplayer-regenerate"),
                    FILL_FRAC_TWO,
                    button_style,
                    Some(Message::WorldConfirmation(Confirmation::Regenerate(i))),
                ))
                .center_x()
                .width(Length::FillPortion(1))
                .max_width(200);
                world_buttons.push(regenerate.into())
            }
            let delete = Container::new(neat_button(
                &mut self.delete_world,
                i18n.get_msg("main-singleplayer-delete"),
                FILL_FRAC_TWO,
                button_style,
                Some(Message::WorldConfirmation(Confirmation::Delete(i))),
            ))
            .center_x()
            .width(Length::FillPortion(1))
            .max_width(200);

            world_buttons.push(delete.into());

            gen_content.push(Row::with_children(world_buttons).into());

            let play_button = Container::new(neat_button(
                &mut self.play_button,
                i18n.get_msg(if world.is_generated || world.gen_opts.is_none() {
                    "main-singleplayer-play"
                } else {
                    "main-singleplayer-generate_and_play"
                }),
                FILL_FRAC_TWO,
                button_style,
                Some(Message::SingleplayerPlay),
            ))
            .center_x()
            .max_width(200);

            gen_content.push(play_button.into());

            let gen_opts = Column::with_children(gen_content).align_items(Align::Center);

            let opts_menu = BackgroundContainer::new(
                CompoundGraphic::from_graphics(vec![
                    Graphic::image(imgs.banner_top, [138, 17], [0, 0]),
                    Graphic::rect(Rgba::new(0, 0, 0, 230), [130, 300], [4, 17]),
                    // TODO: use non image gradient
                    Graphic::gradient(Rgba::new(0, 0, 0, 230), Rgba::zero(), [130, 50], [4, 182]),
                ])
                .fix_aspect_ratio()
                .height(Length::Fill)
                .width(Length::Fill),
                gen_opts,
            )
            .padding(Padding::new().horizontal(5).top(15));

            items.push(opts_menu.into());
        }

        let all = Row::with_children(items)
            .height(Length::Fill)
            .width(Length::Fill);

        if let Some(confirmation) = self.confirmation.as_ref() {
            const FILL_FRAC_ONE: f32 = 0.77;

            let (text, yes_msg, index) = match confirmation {
                Confirmation::Regenerate(i) => (
                    "menu-singleplayer-confirm_regenerate",
                    Message::WorldChanged(WorldsChange::Regenerate(*i)),
                    i,
                ),
                Confirmation::Delete(i) => (
                    "menu-singleplayer-confirm_delete",
                    Message::WorldChanged(WorldsChange::Delete(*i)),
                    i,
                ),
            };

            if let Some(name) = worlds.worlds.get(*index).map(|world| &world.name) {
                let over_content = Column::with_children(vec![
                    Text::new(i18n.get_msg_ctx(text, &i18n::fluent_args! { "world_name" => name }))
                        .size(fonts.cyri.scale(24))
                        .into(),
                    Row::with_children(vec![
                        neat_button(
                            &mut self.no_button,
                            i18n.get_msg("common-no").into_owned(),
                            FILL_FRAC_ONE,
                            button_style,
                            Some(Message::WorldCancelConfirmation),
                        ),
                        neat_button(
                            &mut self.yes_button,
                            i18n.get_msg("common-yes").into_owned(),
                            FILL_FRAC_ONE,
                            button_style,
                            Some(yes_msg),
                        ),
                    ])
                    .height(Length::Units(28))
                    .spacing(30)
                    .into(),
                ])
                .align_items(Align::Center)
                .spacing(10);

                let over = Container::new(over_content)
                    .style(
                        style::container::Style::color_with_double_cornerless_border(
                            (0, 0, 0, 200).into(),
                            (3, 4, 4, 255).into(),
                            (28, 28, 22, 255).into(),
                        ),
                    )
                    .width(Length::Shrink)
                    .height(Length::Shrink)
                    .max_width(400)
                    .max_height(500)
                    .padding(24)
                    .center_x()
                    .center_y();

                Overlay::new(over, all)
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x()
                    .center_y()
                    .into()
            } else {
                self.confirmation = None;
                all.into()
            }
        } else {
            all.into()
        }
    }

    fn map_kind_key(map_kind: MapKind) -> &'static str {
        match map_kind {
            MapKind::Circle => "main-singleplayer-map_shape-circle",
            MapKind::Square => "main-singleplayer-map_shape-square",
        }
    }

    fn provenance_text(world: &SingleplayerWorld, i18n: &Localization) -> Option<String> {
        let source = if let Some(origin) = world.legacy_origin.as_ref() {
            Self::provenance_origin_text(origin, i18n)
        } else {
            let fallback_key = Self::provenance_world_source_msg_key(&world.world_source)?;
            i18n.get_msg(fallback_key).into_owned()
        };

        Some(
            i18n.get_msg_ctx("main-singleplayer-provenance", &i18n::fluent_args! {
                "source" => source,
            })
            .into_owned(),
        )
    }

    fn provenance_origin_text(origin: &SingleplayerLegacyOrigin, i18n: &Localization) -> String {
        match origin {
            SingleplayerLegacyOrigin::LoadPath(path) => i18n
                .get_msg_ctx(
                    Self::provenance_origin_msg_key(origin),
                    &i18n::fluent_args! {
                        "name" => Self::provenance_path_display_name(path),
                    },
                )
                .into_owned(),
            SingleplayerLegacyOrigin::LoadLegacyPath(path) => i18n
                .get_msg_ctx(
                    Self::provenance_origin_msg_key(origin),
                    &i18n::fluent_args! {
                        "name" => Self::provenance_path_display_name(path),
                    },
                )
                .into_owned(),
            SingleplayerLegacyOrigin::LoadAsset(asset) => i18n
                .get_msg_ctx(
                    Self::provenance_origin_msg_key(origin),
                    &i18n::fluent_args! {
                        "asset" => asset.as_str(),
                    },
                )
                .into_owned(),
            SingleplayerLegacyOrigin::LoadOrGenerate { name, overwrite } => i18n
                .get_msg_ctx(
                    Self::provenance_origin_msg_key(origin),
                    &i18n::fluent_args! {
                        "name" => name.as_str(),
                        "overwrite" => i18n.get_msg(Self::provenance_overwrite_msg_key(*overwrite)),
                    },
                )
                .into_owned(),
        }
    }

    fn provenance_world_source_msg_key(
        world_source: &SingleplayerWorldSource,
    ) -> Option<&'static str> {
        match world_source {
            SingleplayerWorldSource::LegacyUnknown => {
                Some("main-singleplayer-provenance-legacy_unknown")
            },
            SingleplayerWorldSource::LegacyMigrated => {
                Some("main-singleplayer-provenance-legacy_migrated")
            },
            SingleplayerWorldSource::Generated | SingleplayerWorldSource::DefaultAsset => None,
        }
    }

    fn provenance_origin_msg_key(origin: &SingleplayerLegacyOrigin) -> &'static str {
        match origin {
            SingleplayerLegacyOrigin::LoadPath(_) => "main-singleplayer-provenance-load_path",
            SingleplayerLegacyOrigin::LoadLegacyPath(_) => {
                "main-singleplayer-provenance-load_legacy_path"
            },
            SingleplayerLegacyOrigin::LoadAsset(_) => "main-singleplayer-provenance-load_asset",
            SingleplayerLegacyOrigin::LoadOrGenerate { .. } => {
                "main-singleplayer-provenance-load_or_generate"
            },
        }
    }

    fn provenance_overwrite_msg_key(overwrite: bool) -> &'static str {
        if overwrite {
            "main-singleplayer-provenance-overwrite-true"
        } else {
            "main-singleplayer-provenance-overwrite-false"
        }
    }

    fn provenance_path_display_name(path: &str) -> String {
        Path::new(path)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or(path)
            .to_owned()
    }

    fn legacy_metadata_gap_text(world: &SingleplayerWorld, i18n: &Localization) -> Option<String> {
        let gap_state = Self::legacy_metadata_gap_state(
            &world.world_source,
            world.legacy_origin.is_some(),
            world.compat_audit.is_some(),
        )?;

        if world.legacy_origin.is_none()
            && Self::provenance_world_source_msg_key(&world.world_source).is_some()
        {
            return match gap_state {
                LegacyMetadataGapState::MissingTypedOrigin => None,
                LegacyMetadataGapState::MissingTypedOriginAndCompatAudit => Some(
                    i18n.get_msg("main-singleplayer-legacy_gap-missing_compat_audit")
                        .into_owned(),
                ),
                LegacyMetadataGapState::MissingCompatAudit => Some(
                    i18n.get_msg(Self::legacy_metadata_gap_msg_key(gap_state))
                        .into_owned(),
                ),
            };
        }

        Some(
            i18n.get_msg(Self::legacy_metadata_gap_msg_key(gap_state))
                .into_owned(),
        )
    }

    fn legacy_gap_badge_text(world: &SingleplayerWorld, i18n: &Localization) -> Option<String> {
        let gap_state = Self::legacy_metadata_gap_state(
            &world.world_source,
            world.legacy_origin.is_some(),
            world.compat_audit.is_some(),
        )?;

        Some(
            i18n.get_msg(Self::legacy_gap_badge_msg_key(gap_state))
                .into_owned(),
        )
    }

    fn managed_recipe_sidecar_missing_text(
        world: &SingleplayerWorld,
        i18n: &Localization,
    ) -> Option<String> {
        Self::managed_recipe_sidecar_missing_msg_key(world)
            .map(|key| i18n.get_msg(key).into_owned())
    }

    fn managed_recipe_sidecar_missing_badge_text(
        world: &SingleplayerWorld,
        i18n: &Localization,
    ) -> Option<String> {
        Self::managed_recipe_sidecar_missing_msg_key(world)
            .map(|_| i18n.get_msg("main-singleplayer-managed_recipe_sidecar_missing-badge"))
            .map(|message| message.into_owned())
    }

    fn managed_recipe_sidecar_missing_msg_key(world: &SingleplayerWorld) -> Option<&'static str> {
        world
            .managed_recipe_sidecar_missing
            .then_some("main-singleplayer-managed_recipe_sidecar_missing")
    }

    fn should_show_legacy_gap_badge(
        item_index: Option<usize>,
        current_index: Option<usize>,
    ) -> bool {
        item_index != current_index
    }

    fn legacy_metadata_gap_state(
        world_source: &SingleplayerWorldSource,
        has_typed_origin: bool,
        has_compat_audit: bool,
    ) -> Option<LegacyMetadataGapState> {
        if !Self::is_legacy_world_source(world_source) {
            return None;
        }

        match (has_typed_origin, has_compat_audit) {
            (false, false) => Some(LegacyMetadataGapState::MissingTypedOriginAndCompatAudit),
            (false, true) => Some(LegacyMetadataGapState::MissingTypedOrigin),
            (true, false) => Some(LegacyMetadataGapState::MissingCompatAudit),
            (true, true) => None,
        }
    }

    const fn legacy_metadata_gap_msg_key(gap_state: LegacyMetadataGapState) -> &'static str {
        match gap_state {
            LegacyMetadataGapState::MissingTypedOrigin => {
                "main-singleplayer-legacy_gap-missing_typed_origin"
            },
            LegacyMetadataGapState::MissingCompatAudit => {
                "main-singleplayer-legacy_gap-missing_compat_audit"
            },
            LegacyMetadataGapState::MissingTypedOriginAndCompatAudit => {
                "main-singleplayer-legacy_gap-missing_typed_origin_and_compat_audit"
            },
        }
    }

    const fn legacy_gap_badge_msg_key(gap_state: LegacyMetadataGapState) -> &'static str {
        match gap_state {
            LegacyMetadataGapState::MissingTypedOrigin => {
                "main-singleplayer-legacy_gap-badge-missing_typed_origin"
            },
            LegacyMetadataGapState::MissingCompatAudit => {
                "main-singleplayer-legacy_gap-badge-missing_compat_audit"
            },
            LegacyMetadataGapState::MissingTypedOriginAndCompatAudit => {
                "main-singleplayer-legacy_gap-badge-missing_typed_origin_and_compat_audit"
            },
        }
    }

    const fn is_legacy_world_source(world_source: &SingleplayerWorldSource) -> bool {
        matches!(
            world_source,
            SingleplayerWorldSource::LegacyUnknown | SingleplayerWorldSource::LegacyMigrated
        )
    }

    fn legacy_inventory_summary_text(
        worlds: &crate::singleplayer::SingleplayerWorlds,
        i18n: &Localization,
    ) -> Option<String> {
        let summary = Self::legacy_inventory_summary(&worlds.legacy_inventory())?;
        let mut text = i18n
            .get_msg_ctx(
                "main-singleplayer-legacy_stock-total",
                &i18n::fluent_args! {
                    "legacy" => summary.legacy_worlds,
                },
            )
            .into_owned();

        for detail in summary.residual_details {
            let detail_text = i18n
                .get_msg_ctx(detail.message_key, &i18n::fluent_args! {
                    "count" => detail.count,
                })
                .into_owned();
            let _ = write!(text, "; {detail_text}");
        }

        Some(text)
    }

    fn legacy_inventory_summary(
        inventory: &SingleplayerLegacyInventory,
    ) -> Option<LegacyInventorySummary<'static>> {
        (inventory.legacy_worlds > 0).then_some(LegacyInventorySummary {
            legacy_worlds: inventory.legacy_worlds,
            residual_details: Self::legacy_inventory_summary_details(inventory),
        })
    }

    fn legacy_inventory_summary_details(
        inventory: &SingleplayerLegacyInventory,
    ) -> Vec<LegacyInventorySummaryDetail<'static>> {
        let mut details = Vec::new();
        let residual_missing_typed_origin = inventory
            .legacy_worlds_without_typed_origin
            .saturating_sub(inventory.legacy_unknown_worlds);
        Self::push_legacy_inventory_summary_detail(
            &mut details,
            inventory.legacy_unknown_worlds,
            "main-singleplayer-legacy_stock-unknown",
        );
        Self::push_legacy_inventory_summary_detail(
            &mut details,
            residual_missing_typed_origin,
            "main-singleplayer-legacy_stock-missing_typed_origin",
        );
        Self::push_legacy_inventory_summary_detail(
            &mut details,
            inventory.legacy_worlds_without_compat_audit,
            "main-singleplayer-legacy_stock-missing_compat_audit",
        );
        Self::push_legacy_inventory_summary_detail(
            &mut details,
            inventory.legacy_worlds_with_sidecarless_managed_residual,
            "main-singleplayer-legacy_stock-sidecarless_managed_residual",
        );
        details
    }

    fn push_legacy_inventory_summary_detail(
        details: &mut Vec<LegacyInventorySummaryDetail<'static>>,
        count: usize,
        message_key: &'static str,
    ) {
        if count > 0 {
            details.push(LegacyInventorySummaryDetail { message_key, count });
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
struct LegacyInventorySummary<'a> {
    legacy_worlds: usize,
    residual_details: Vec<LegacyInventorySummaryDetail<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LegacyInventorySummaryDetail<'a> {
    message_key: &'a str,
    count: usize,
}

#[cfg(test)]
mod tests {
    use super::{LegacyInventorySummaryDetail, LegacyMetadataGapState, Screen};
    use crate::singleplayer::{
        SingleplayerLegacyInventory, SingleplayerLegacyOrigin, SingleplayerWorld,
        SingleplayerWorldSource,
    };
    use common::uuid::Uuid;
    use server::{CompatAuditV1, CompatEntryKindV1};

    fn test_world(
        world_source: SingleplayerWorldSource,
        legacy_origin: Option<SingleplayerLegacyOrigin>,
        compat_audit: bool,
        managed_recipe_sidecar_missing: bool,
    ) -> SingleplayerWorld {
        SingleplayerWorld {
            world_id: Uuid::nil(),
            realm_id: Uuid::nil(),
            name: "test".to_string(),
            gen_opts: None,
            day_length: 0.0,
            seed: 0,
            world_source,
            source_ref: None,
            legacy_origin,
            compat_audit: compat_audit
                .then(|| CompatAuditV1::loaded_existing(CompatEntryKindV1::LoadLegacy)),
            managed_recipe_sidecar_missing,
            world_recipe_hash: None,
            topology_id: None,
            is_generated: false,
            path: std::path::PathBuf::from("test-world"),
            map_path: std::path::PathBuf::from("test-world/map.bin"),
        }
    }

    #[test]
    fn provenance_origin_msg_keys_match_expected_variants() {
        assert_eq!(
            Screen::provenance_origin_msg_key(&SingleplayerLegacyOrigin::LoadPath(
                "C:/maps/world.bin".to_string()
            )),
            "main-singleplayer-provenance-load_path"
        );
        assert_eq!(
            Screen::provenance_origin_msg_key(&SingleplayerLegacyOrigin::LoadLegacyPath(
                "C:/maps/legacy.bin".to_string()
            )),
            "main-singleplayer-provenance-load_legacy_path"
        );
        assert_eq!(
            Screen::provenance_origin_msg_key(&SingleplayerLegacyOrigin::LoadAsset(
                "world.test.asset".to_string()
            )),
            "main-singleplayer-provenance-load_asset"
        );
        assert_eq!(
            Screen::provenance_origin_msg_key(&SingleplayerLegacyOrigin::LoadOrGenerate {
                name: "managed".to_string(),
                overwrite: true,
            }),
            "main-singleplayer-provenance-load_or_generate"
        );
    }

    #[test]
    fn provenance_world_source_msg_keys_cover_legacy_worlds_only() {
        assert_eq!(
            Screen::provenance_world_source_msg_key(&SingleplayerWorldSource::LegacyUnknown),
            Some("main-singleplayer-provenance-legacy_unknown")
        );
        assert_eq!(
            Screen::provenance_world_source_msg_key(&SingleplayerWorldSource::LegacyMigrated),
            Some("main-singleplayer-provenance-legacy_migrated")
        );
        assert_eq!(
            Screen::provenance_world_source_msg_key(&SingleplayerWorldSource::Generated),
            None
        );
        assert_eq!(
            Screen::provenance_world_source_msg_key(&SingleplayerWorldSource::DefaultAsset),
            None
        );
    }

    #[test]
    fn provenance_path_display_name_prefers_file_name() {
        assert_eq!(
            Screen::provenance_path_display_name("C:/maps/world.bin"),
            "world.bin"
        );
        assert_eq!(
            Screen::provenance_path_display_name("legacy-world.bin"),
            "legacy-world.bin"
        );
    }

    #[test]
    fn provenance_overwrite_msg_keys_match_expected_variants() {
        assert_eq!(
            Screen::provenance_overwrite_msg_key(true),
            "main-singleplayer-provenance-overwrite-true"
        );
        assert_eq!(
            Screen::provenance_overwrite_msg_key(false),
            "main-singleplayer-provenance-overwrite-false"
        );
    }

    #[test]
    fn legacy_metadata_gap_msg_keys_match_expected_variants() {
        assert_eq!(
            Screen::legacy_metadata_gap_msg_key(LegacyMetadataGapState::MissingTypedOrigin),
            "main-singleplayer-legacy_gap-missing_typed_origin"
        );
        assert_eq!(
            Screen::legacy_metadata_gap_msg_key(LegacyMetadataGapState::MissingCompatAudit),
            "main-singleplayer-legacy_gap-missing_compat_audit"
        );
        assert_eq!(
            Screen::legacy_metadata_gap_msg_key(
                LegacyMetadataGapState::MissingTypedOriginAndCompatAudit
            ),
            "main-singleplayer-legacy_gap-missing_typed_origin_and_compat_audit"
        );
    }

    #[test]
    fn legacy_gap_badge_msg_keys_match_expected_variants() {
        assert_eq!(
            Screen::legacy_gap_badge_msg_key(LegacyMetadataGapState::MissingTypedOrigin),
            "main-singleplayer-legacy_gap-badge-missing_typed_origin"
        );
        assert_eq!(
            Screen::legacy_gap_badge_msg_key(LegacyMetadataGapState::MissingCompatAudit),
            "main-singleplayer-legacy_gap-badge-missing_compat_audit"
        );
        assert_eq!(
            Screen::legacy_gap_badge_msg_key(
                LegacyMetadataGapState::MissingTypedOriginAndCompatAudit
            ),
            "main-singleplayer-legacy_gap-badge-missing_typed_origin_and_compat_audit"
        );
    }

    #[test]
    fn legacy_metadata_gap_state_only_surfaces_real_legacy_gaps() {
        assert_eq!(
            Screen::legacy_metadata_gap_state(
                &SingleplayerWorldSource::LegacyUnknown,
                false,
                false
            ),
            Some(LegacyMetadataGapState::MissingTypedOriginAndCompatAudit)
        );
        assert_eq!(
            Screen::legacy_metadata_gap_state(
                &SingleplayerWorldSource::LegacyMigrated,
                false,
                true
            ),
            Some(LegacyMetadataGapState::MissingTypedOrigin)
        );
        assert_eq!(
            Screen::legacy_metadata_gap_state(
                &SingleplayerWorldSource::LegacyMigrated,
                true,
                false
            ),
            Some(LegacyMetadataGapState::MissingCompatAudit)
        );
        assert_eq!(
            Screen::legacy_metadata_gap_state(&SingleplayerWorldSource::LegacyMigrated, true, true),
            None
        );
        assert_eq!(
            Screen::legacy_metadata_gap_state(&SingleplayerWorldSource::Generated, false, false),
            None
        );
    }

    #[test]
    fn legacy_gap_badge_text_only_surfaces_real_legacy_gaps() {
        let missing_both = test_world(SingleplayerWorldSource::LegacyUnknown, None, false, false);
        assert_eq!(
            Screen::legacy_metadata_gap_state(
                &missing_both.world_source,
                missing_both.legacy_origin.is_some(),
                missing_both.compat_audit.is_some()
            ),
            Some(LegacyMetadataGapState::MissingTypedOriginAndCompatAudit)
        );

        let missing_audit = test_world(
            SingleplayerWorldSource::LegacyMigrated,
            Some(SingleplayerLegacyOrigin::LoadLegacyPath(
                "C:/maps/legacy.bin".to_string(),
            )),
            false,
            false,
        );
        assert_eq!(
            Screen::legacy_metadata_gap_state(
                &missing_audit.world_source,
                missing_audit.legacy_origin.is_some(),
                missing_audit.compat_audit.is_some()
            ),
            Some(LegacyMetadataGapState::MissingCompatAudit)
        );

        let generated = test_world(SingleplayerWorldSource::Generated, None, false, false);
        assert_eq!(
            Screen::legacy_metadata_gap_state(
                &generated.world_source,
                generated.legacy_origin.is_some(),
                generated.compat_audit.is_some()
            ),
            None
        );
    }

    #[test]
    fn legacy_inventory_summary_hides_when_no_legacy_worlds_remain() {
        assert_eq!(
            Screen::legacy_inventory_summary(&SingleplayerLegacyInventory::default()),
            None
        );
    }

    #[test]
    fn legacy_inventory_summary_preserves_non_zero_residual_truth() {
        let summary = Screen::legacy_inventory_summary(&SingleplayerLegacyInventory {
            total_worlds: 6,
            legacy_worlds: 3,
            legacy_unknown_worlds: 1,
            legacy_worlds_without_typed_origin: 2,
            legacy_worlds_without_compat_audit: 2,
            legacy_worlds_with_sidecarless_managed_residual: 1,
            ..SingleplayerLegacyInventory::default()
        })
        .expect("legacy summary should exist");

        assert_eq!(summary.legacy_worlds, 3);
        assert_eq!(summary.residual_details, vec![
            LegacyInventorySummaryDetail {
                message_key: "main-singleplayer-legacy_stock-unknown",
                count: 1,
            },
            LegacyInventorySummaryDetail {
                message_key: "main-singleplayer-legacy_stock-missing_typed_origin",
                count: 1,
            },
            LegacyInventorySummaryDetail {
                message_key: "main-singleplayer-legacy_stock-missing_compat_audit",
                count: 2,
            },
            LegacyInventorySummaryDetail {
                message_key: "main-singleplayer-legacy_stock-sidecarless_managed_residual",
                count: 1,
            },
        ]);
    }

    #[test]
    fn legacy_inventory_summary_hides_missing_typed_origin_when_fully_explained_by_unknown() {
        let summary = Screen::legacy_inventory_summary(&SingleplayerLegacyInventory {
            total_worlds: 4,
            legacy_worlds: 2,
            legacy_unknown_worlds: 1,
            legacy_worlds_without_typed_origin: 1,
            legacy_worlds_without_compat_audit: 0,
            ..SingleplayerLegacyInventory::default()
        })
        .expect("legacy summary should exist");

        assert_eq!(summary.legacy_worlds, 2);
        assert_eq!(summary.residual_details, vec![
            LegacyInventorySummaryDetail {
                message_key: "main-singleplayer-legacy_stock-unknown",
                count: 1,
            }
        ]);
    }

    #[test]
    fn legacy_inventory_summary_omits_zero_residual_buckets() {
        let summary = Screen::legacy_inventory_summary(&SingleplayerLegacyInventory {
            total_worlds: 6,
            legacy_worlds: 3,
            legacy_unknown_worlds: 0,
            legacy_worlds_without_typed_origin: 0,
            legacy_worlds_without_compat_audit: 2,
            legacy_worlds_with_sidecarless_managed_residual: 0,
            ..SingleplayerLegacyInventory::default()
        })
        .expect("legacy summary should exist");

        assert_eq!(summary.legacy_worlds, 3);
        assert_eq!(summary.residual_details, vec![
            LegacyInventorySummaryDetail {
                message_key: "main-singleplayer-legacy_stock-missing_compat_audit",
                count: 2,
            }
        ]);
    }

    #[test]
    fn current_selection_suppresses_list_badge_duplication() {
        assert!(!Screen::should_show_legacy_gap_badge(Some(2), Some(2)));
        assert!(Screen::should_show_legacy_gap_badge(Some(1), Some(2)));
        assert!(Screen::should_show_legacy_gap_badge(Some(1), None));
    }

    #[test]
    fn legacy_unknown_without_typed_origin_still_maps_to_real_gap_states() {
        let missing_both = Screen::legacy_metadata_gap_state(
            &SingleplayerWorldSource::LegacyUnknown,
            false,
            false,
        );
        assert_eq!(
            missing_both,
            Some(LegacyMetadataGapState::MissingTypedOriginAndCompatAudit)
        );

        let missing_origin_only =
            Screen::legacy_metadata_gap_state(&SingleplayerWorldSource::LegacyUnknown, false, true);
        assert_eq!(
            missing_origin_only,
            Some(LegacyMetadataGapState::MissingTypedOrigin)
        );

        assert_eq!(
            Screen::provenance_world_source_msg_key(&SingleplayerWorldSource::LegacyUnknown),
            Some("main-singleplayer-provenance-legacy_unknown")
        );
    }

    #[test]
    fn managed_recipe_sidecar_missing_text_only_surfaces_runtime_residual_truth() {
        let sidecarless_managed_world = test_world(
            SingleplayerWorldSource::Generated,
            Some(SingleplayerLegacyOrigin::LoadOrGenerate {
                name: "managed".to_string(),
                overwrite: false,
            }),
            true,
            true,
        );
        assert_eq!(
            Screen::managed_recipe_sidecar_missing_msg_key(&sidecarless_managed_world),
            Some("main-singleplayer-managed_recipe_sidecar_missing")
        );

        let strict_world = test_world(
            SingleplayerWorldSource::Generated,
            Some(SingleplayerLegacyOrigin::LoadOrGenerate {
                name: "managed".to_string(),
                overwrite: false,
            }),
            true,
            false,
        );
        assert_eq!(
            Screen::managed_recipe_sidecar_missing_msg_key(&strict_world),
            None
        );
    }

    #[test]
    fn managed_recipe_sidecar_missing_badge_tracks_same_runtime_truth() {
        let sidecarless_managed_world = test_world(
            SingleplayerWorldSource::Generated,
            Some(SingleplayerLegacyOrigin::LoadOrGenerate {
                name: "managed".to_string(),
                overwrite: false,
            }),
            true,
            true,
        );
        assert!(
            Screen::managed_recipe_sidecar_missing_msg_key(&sidecarless_managed_world).is_some()
        );

        let strict_world = test_world(
            SingleplayerWorldSource::Generated,
            Some(SingleplayerLegacyOrigin::LoadOrGenerate {
                name: "managed".to_string(),
                overwrite: false,
            }),
            true,
            false,
        );
        assert!(Screen::managed_recipe_sidecar_missing_msg_key(&strict_world).is_none());
    }
}
