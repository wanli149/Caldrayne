use iced::{
    Clipboard, Element, Event, Hasher, Layout, Length, Point, Rectangle, Widget, layout,
};
use std::{cell::Cell, hash::Hash};

#[derive(Debug, Default)]
pub struct BoundsState {
    bounds: Cell<Option<Rectangle>>,
}

impl BoundsState {
    pub fn bounds(&self) -> Option<Rectangle> { self.bounds.get() }

    pub fn clear(&self) { self.bounds.set(None) }

    fn update(&self, bounds: Rectangle) { self.bounds.set(Some(bounds)) }
}

pub struct TrackBounds<'a, M, R> {
    state: &'a BoundsState,
    content: Element<'a, M, R>,
}

impl<'a, M, R> TrackBounds<'a, M, R>
where
    R: iced::Renderer,
{
    pub fn new(state: &'a BoundsState, content: impl Into<Element<'a, M, R>>) -> Self {
        Self {
            state,
            content: content.into(),
        }
    }
}

impl<M, R> Widget<M, R> for TrackBounds<'_, M, R>
where
    R: iced::Renderer,
{
    fn width(&self) -> Length { self.content.width() }

    fn height(&self) -> Length { self.content.height() }

    fn layout(&self, renderer: &R, limits: &layout::Limits) -> layout::Node {
        self.content.layout(renderer, limits)
    }

    fn on_event(
        &mut self,
        event: Event,
        layout: Layout<'_>,
        cursor_position: Point,
        renderer: &R,
        clipboard: &mut dyn Clipboard,
        messages: &mut Vec<M>,
    ) -> iced::event::Status {
        self.state.update(layout.bounds());
        self.content.on_event(
            event,
            layout,
            cursor_position,
            renderer,
            clipboard,
            messages,
        )
    }

    fn draw(
        &self,
        renderer: &mut R,
        defaults: &R::Defaults,
        layout: Layout<'_>,
        cursor_position: Point,
        viewport: &Rectangle,
    ) -> R::Output {
        self.state.update(layout.bounds());
        self.content
            .draw(renderer, defaults, layout, cursor_position, viewport)
    }

    fn hash_layout(&self, state: &mut Hasher) {
        struct Marker;
        std::any::TypeId::of::<Marker>().hash(state);
        self.content.hash_layout(state);
    }

    fn overlay(&mut self, layout: Layout<'_>) -> Option<iced::overlay::Element<'_, M, R>> {
        self.state.update(layout.bounds());
        self.content.overlay(layout)
    }
}

impl<'a, M, R> From<TrackBounds<'a, M, R>> for Element<'a, M, R>
where
    R: 'a + iced::Renderer,
    M: 'a,
{
    fn from(track_bounds: TrackBounds<'a, M, R>) -> Element<'a, M, R> {
        Element::new(track_bounds)
    }
}
