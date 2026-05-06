// SPDX-License-Identifier: MPL-2.0

use cosmic::iced::advanced::layout::{self, Layout};
use cosmic::iced::advanced::renderer;
use cosmic::iced::advanced::widget::{self, Tree, Widget};
use cosmic::iced::advanced::{Clipboard, Shell};
use cosmic::iced::mouse;
use cosmic::iced::{Element, Event, Length, Rectangle, Size};

pub(super) struct Measure<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    width: f32,
    on_resize: Box<dyn Fn(Size) -> Message + 'a>,
}

impl<'a, Message, Theme, Renderer> Measure<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    pub(super) fn new(
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
        width: f32,
        on_resize: impl Fn(Size) -> Message + 'a,
    ) -> Self {
        Self {
            content: content.into(),
            width,
            on_resize: Box::new(on_resize),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct State {
    last_size: Option<Size>,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Measure<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<State>()
    }

    fn state(&self) -> widget::tree::State {
        widget::tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(0.0), Length::Fixed(0.0))
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        _limits: &layout::Limits,
    ) -> layout::Node {
        let width = self.width.max(1.0);
        let limits = layout::Limits::new(Size::ZERO, Size::new(width, f32::INFINITY))
            .width(Length::Fixed(width));
        let child = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &limits);

        layout::Node::with_children(Size::ZERO, vec![child])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        if !matches!(
            event,
            Event::Window(cosmic::iced::window::Event::RedrawRequested(_))
        ) {
            return;
        }

        let Some(child) = layout.children().next() else {
            return;
        };
        let size = child.bounds().size();
        let state = tree.state.downcast_mut::<State>();
        if Some(size) != state.last_size {
            state.last_size = Some(size);
            shell.publish((self.on_resize)(size));
        }
    }

    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
    }
}

impl<'a, Message, Theme, Renderer> From<Measure<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(measure: Measure<'a, Message, Theme, Renderer>) -> Self {
        Self::new(measure)
    }
}
