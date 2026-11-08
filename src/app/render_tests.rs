use super::*;

pub(super) fn simulator(
    app: &Photos,
    width: f32,
    height: f32,
) -> iced_test::Simulator<'_, Message> {
    app.viewport_width.set(width);
    let localizer = crate::localization::catalog();
    localizer.select(vec!["en-US".parse().expect("locale")]);
    iced_test::Simulator::with_size(
        iced::Settings::default(),
        iced::Size::new(width, height),
        localizer.view(|| app.view()),
    )
}

#[test]
fn viewer_focused_arrows_and_image_menu_keep_navigation_and_copy_ownership() {
    use super::tests::dispatch;
    use iced::keyboard::key::Named;
    let (mut app, _sandbox) = super::tests::loaded();
    let first = app.library.photos[app.library.visible[0]].id.clone();
    let second = app.library.photos[app.library.visible[1]].id.clone();
    dispatch(&mut app, Message::Open(first.clone()));
    let owner = app.viewer_owner().expect("viewer owner");
    let mut ui = simulator(&app, 960.0, 720.0);
    ui.click(iced::widget::Id::new("photos-viewer"))
        .expect("focus image");
    assert_eq!(ui.tap_key(Named::ArrowRight), iced::event::Status::Captured);
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(messages.iter().any(|m| matches!(m, Message::Next(1))));
    assert!(!messages.iter().any(|m| matches!(m, Message::Transform(_))));
    for message in messages {
        dispatch(&mut app, message);
    }
    assert_eq!(app.viewer.as_ref().expect("next photo").id, second);
    let generation = app.load_generation;
    dispatch(&mut app, Message::Next(1));
    assert_eq!(
        app.load_generation, generation,
        "the last photo must keep its zoom and load"
    );
    let stale = app.update(Message::ViewerCommand(owner, Box::new(Message::SaveCopy)));
    drop(stale);
    assert!(app.dialog.is_none());
    assert!(app.chooser_request.is_none());
    let selection = app.selection.mode();
    let mut ui = simulator(&app, 960.0, 720.0);
    let bounds = ui
        .find(iced::widget::Id::new("photos-viewer"))
        .expect("image")
        .visible_bounds()
        .expect("visible image");
    ui.point_at(bounds.center());
    ui.simulate([
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Right,
        )),
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Right,
        )),
    ]);
    for message in ui.into_messages() {
        dispatch(&mut app, message);
    }
    assert!(app.context.is_open());
    assert_eq!(app.selection.mode(), selection);
    let mut ui = simulator(&app, 960.0, 720.0);
    ui.draw(&app.theme());
    assert!(ui.find("Copy image").is_ok());
    assert!(ui.find("Save copy…").is_ok());
    assert!(ui.find("Photo information").is_ok());
    ui.click("Save copy…").expect("save copy menu action");
    for message in ui.into_messages() {
        let task = app.update(message);
        drop(task);
    }
    assert!(app.dialog.is_none());
    assert_eq!(
        app.chooser_request
            .as_ref()
            .expect("file chooser")
            .0
            .purpose,
        super::PickPurpose::Export
    );
    assert_eq!(app.viewer.as_ref().expect("same source").id, second);
}

#[test]
fn gallery_marquee_starts_on_empty_space_and_preserves_tile_gestures() {
    let (app, _sandbox) = super::tests::loaded();
    let id = &app.library.photos[app.library.visible[0]].id;
    for starts_on_tile in [false, true] {
        let mut ui = simulator(&app, 960.0, 720.0);
        let tile = ui
            .find(iced::widget::Id::from(format!("photos-tile-{}", id.0)))
            .expect("photo tile")
            .visible_bounds()
            .expect("visible tile");
        let start = if starts_on_tile {
            tile.center()
        } else {
            iced::Point::new(tile.x + 8.0, tile.y + tile.height + 6.0)
        };
        ui.point_at(start);
        ui.simulate([iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
            iced::mouse::Button::Left,
        ))]);
        let end = iced::Point::new(tile.x + tile.width - 8.0, tile.y + 8.0);
        ui.point_at(end);
        ui.simulate([iced::Event::Mouse(iced::mouse::Event::CursorMoved {
            position: end,
        })]);
        ui.simulate([iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        ))]);
        let selections: Vec<_> = ui
            .into_messages()
            .filter_map(|message| match message {
                Message::DragSelection(_, ids) => Some(ids),
                _ => None,
            })
            .collect();
        if starts_on_tile {
            assert!(
                selections.is_empty(),
                "tile drag must reach its native source"
            );
        } else {
            assert!(
                selections.iter().any(|ids| ids.contains(id)),
                "empty-space marquee selects covered photos"
            );
        }
    }
}
