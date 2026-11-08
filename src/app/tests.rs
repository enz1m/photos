use std::collections::VecDeque;
use std::sync::Arc;

use iced::futures::{StreamExt, executor::block_on};
use image::{Rgba, RgbaImage};

use super::*;

fn app() -> (Photos, tempfile::TempDir) {
    let sandbox = tempfile::tempdir().expect("sandbox");
    let paths = crate::platform::Paths {
        photos: sandbox.path().join("Photos"),
        catalog: sandbox.path().join("state/catalog.json"),
    };
    std::fs::create_dir_all(&paths.photos).expect("photos");
    RgbaImage::from_pixel(12, 8, Rgba([40, 80, 100, 255]))
        .save(paths.photos.join("first.png"))
        .expect("photo");
    RgbaImage::from_pixel(10, 6, Rgba([140, 80, 20, 255]))
        .save(paths.photos.join("second.png"))
        .expect("photo");
    let (app, _startup) = Photos::new_at(paths);
    (app, sandbox)
}

pub(super) fn dispatch(app: &mut Photos, message: Message) {
    let localizer = crate::localization::catalog();
    localizer.select(vec!["en-US".parse().expect("locale")]);
    localizer.scope(|| {
        let task = app.update(message);
        settle(app, task);
    });
}

thread_local! {
    static IMAGE_RENDERER: iced::Renderer = block_on(<iced::Renderer as iced::advanced::renderer::Headless>::new(
        iced::advanced::renderer::Settings::default(), Some("tiny-skia"),
    )).expect("image renderer");
}

pub(super) fn settle(app: &mut Photos, task: iced::Task<Message>) {
    let mut tasks = VecDeque::from([task]);
    let mut outputs = 0;
    while let Some(task) = tasks.pop_front() {
        if let Some(mut stream) = iced_runtime::task::into_stream(task) {
            while let Some(action) = block_on(stream.next()) {
                if let iced_runtime::Action::Image(iced_runtime::image::Action::Allocate(
                    handle,
                    sender,
                )) = action
                {
                    IMAGE_RENDERER.with(|renderer| {
                        let _ = sender.send(iced::advanced::image::Renderer::load_image(
                            renderer, &handle,
                        ));
                    });
                } else if let iced_runtime::Action::Output(message) = action {
                    outputs += 1;
                    assert!(outputs < 200, "dispatcher did not settle");
                    tasks.push_back(app.update(message));
                }
            }
        }
    }
}

pub(super) fn loaded() -> (Photos, tempfile::TempDir) {
    let (mut app, sandbox) = app();
    let session = crate::services::persistence::open(&app.paths.catalog).expect("catalog session");
    dispatch(&mut app, Message::CatalogLoaded(Ok(session)));
    assert_eq!(app.library.photos.len(), 2);
    (app, sandbox)
}

#[test]
fn routing_favorite_album_and_search_survive_restart() {
    let (mut app, _sandbox) = loaded();
    let id = app.library.photos[0].id.clone();
    dispatch(&mut app, Message::Favorite(id.clone()));
    dispatch(&mut app, Message::Navigate(Destination::Favorites));
    assert_eq!(app.library.visible.len(), 1);
    dispatch(&mut app, Message::Open(id.clone()));
    dispatch(&mut app, Message::DialogText("A collection".into()));
    dispatch(&mut app, Message::CreateAlbum);
    dispatch(&mut app, Message::AddToAlbum(1));
    dispatch(&mut app, Message::Navigate(Destination::Album(1)));
    assert_eq!(app.library.visible.len(), 1);
    dispatch(&mut app, Message::Search("no-such-photo".into()));
    assert!(app.library.visible.is_empty());
    dispatch(&mut app, Message::Search(String::new()));
    assert_eq!(app.library.visible.len(), 1);
    let saved = crate::services::persistence::load(&app.paths.catalog).expect("catalog");
    assert!(saved.favorites.contains(&id));
    assert!(saved.albums[0].photos.contains(&id));
    let originals: Vec<_> = app
        .library
        .photos
        .iter()
        .map(|p| (p.path.clone(), std::fs::read(&p.path).expect("original")))
        .collect();
    dispatch(&mut app, Message::Navigate(Destination::Library));
    dispatch(&mut app, Message::SelectAll);
    dispatch(&mut app, Message::AddToAlbum(1));
    dispatch(&mut app, Message::Navigate(Destination::Album(1)));
    assert_eq!(app.library.visible.len(), 2);
    dispatch(&mut app, Message::SelectAll);
    dispatch(&mut app, Message::RemoveFromAlbum);
    let saved = crate::services::persistence::load(&app.paths.catalog).expect("catalog");
    assert!(saved.albums[0].photos.is_empty());
    for (path, original) in originals {
        assert_eq!(std::fs::read(path).expect("retained source"), original);
    }
}

#[test]
fn routing_edit_cancel_undo_save_and_export_preserve_original() {
    let (mut app, _sandbox) = loaded();
    let photo = app.library.photos[0].clone();
    let original = std::fs::read(&photo.path).expect("original");
    dispatch(&mut app, Message::Open(photo.id.clone()));
    dispatch(&mut app, Message::BeginEdit);
    dispatch(&mut app, Message::Adjust(Adjustment::Exposure, 1.0));
    assert_eq!(app.viewer.as_ref().expect("viewer").recipe.exposure, 1.0);
    dispatch(&mut app, Message::Undo);
    assert_eq!(app.viewer.as_ref().expect("viewer").recipe.exposure, 0.0);
    dispatch(&mut app, Message::Redo);
    dispatch(&mut app, Message::CancelEdit);
    assert_eq!(app.viewer.as_ref().expect("viewer").recipe.exposure, 0.0);
    dispatch(&mut app, Message::BeginEdit);
    dispatch(&mut app, Message::Rotate);
    dispatch(&mut app, Message::SaveEdit);
    let output = app.paths.photos.join("exported.png");
    dispatch(
        &mut app,
        Message::FolderChosen(PickPurpose::Export, output.clone()),
    );
    let exported = image::open(&output).expect("export");
    assert_eq!(exported.width(), photo.height);
    assert_eq!(exported.height(), photo.width);
    assert_eq!(std::fs::read(&photo.path).expect("source"), original);
    assert_eq!(
        crate::services::persistence::load(&app.paths.catalog)
            .expect("catalog")
            .edits[&photo.id]
            .quarter_turns,
        1
    );
}

#[test]
fn routing_source_changes_during_scan_retire_the_old_projection() {
    let (mut app, sandbox) = loaded();
    let source = sandbox.path().join("external");
    std::fs::create_dir(&source).expect("source");
    RgbaImage::new(5, 3)
        .save(source.join("external.png"))
        .expect("image");
    app.scanning = true;
    let generation = app.scan_generation;
    dispatch(
        &mut app,
        Message::FolderChosen(PickPurpose::Source, source.clone()),
    );
    assert!(app.scan_requested);
    dispatch(
        &mut app,
        Message::Scanned(generation, Arc::new(Default::default()), None),
    );
    assert_eq!(app.library.photos.len(), 3);
    assert!(!app.scanning);
    dispatch(&mut app, Message::RemoveSource(source));
    assert_eq!(app.library.photos.len(), 2);
    assert!(app.catalog.sources.is_empty());
}

#[test]
fn routing_navigation_retires_pending_library_and_direct_loads() {
    let (mut app, _sandbox) = loaded();
    let id = app.library.photos[0].id.clone();
    let task = app.update(Message::Open(id));
    let generation = app.load_generation;
    dispatch(&mut app, Message::Back);
    settle(&mut app, task);
    assert!(app.viewer.is_none());
    assert!(app.load_generation > generation);

    let (mut app, _sandbox, photo, media) = direct_fixture();
    dispatch(&mut app, Message::Back);
    dispatch(&mut app, Message::FileOpened(0, Ok((photo, media))));
    assert!(app.viewer.is_none());
    assert!(!app.opening_file);
    assert!(app.library.photos.is_empty());
}

fn exits(task: iced::Task<Message>) -> bool {
    let Some(mut stream) = iced_runtime::task::into_stream(task) else {
        return false;
    };
    while let Some(action) = block_on(stream.next()) {
        if matches!(action, iced_runtime::Action::Exit) {
            return true;
        }
    }
    false
}

#[test]
fn routing_close_preserves_edits_during_persistence_success_and_failure() {
    let (mut app, _sandbox) = loaded();
    let id = app.library.photos[0].id.clone();
    dispatch(&mut app, Message::Open(id.clone()));
    dispatch(&mut app, Message::BeginEdit);
    dispatch(&mut app, Message::Rotate);
    dispatch(&mut app, Message::CloseRequested);
    assert!(matches!(app.dialog, Some(Dialog::CloseEdits)));
    assert!(!app.close_requested);
    dispatch(&mut app, Message::DismissDialog);
    assert!(app.viewer.as_ref().expect("viewer").editing);
    let save = app.update(Message::SaveEdit);
    assert!(!exits(app.update(Message::CloseRequested)));
    assert!(app.close_requested);
    settle(&mut app, save);
    assert_eq!(app.saved_revision, app.revision);
    let saved = crate::services::persistence::load(&app.paths.catalog).expect("saved");
    assert_eq!(saved.edits[&id].quarter_turns, 1);
    assert!(exits(app.update(Message::CloseRequested)));

    let (mut app, _sandbox) = loaded();
    app.revision = 1;
    app.save_inflight = true;
    assert!(!exits(app.update(Message::CloseRequested)));
    assert!(!exits(app.update(Message::PersistenceFinished(
        1,
        Err("disk full".into())
    ))));
    assert!(!app.close_requested);
    assert_ne!(app.saved_revision, app.revision);
    assert!(app.notice.as_ref().expect("notice").1);
}

#[test]
fn routing_navigation_requires_edit_decision_and_cancel_keeps_the_session() {
    let (mut app, _sandbox) = loaded();
    let id = app.library.photos[0].id.clone();
    dispatch(&mut app, Message::Open(id.clone()));
    dispatch(&mut app, Message::BeginEdit);
    dispatch(&mut app, Message::Rotate);
    dispatch(&mut app, Message::BeginEdit);
    assert_eq!(
        app.viewer
            .as_ref()
            .expect("viewer")
            .before_edit
            .quarter_turns,
        0
    );
    assert_eq!(app.viewer.as_ref().expect("viewer").history.len(), 1);
    let other = app.library.photos[1].path.clone();
    dispatch(&mut app, Message::OpenExternal(other));
    assert!(matches!(app.dialog, Some(Dialog::LeaveEdits)));
    assert_eq!(app.viewer.as_ref().expect("viewer").id, id);
    dispatch(&mut app, Message::DismissDialog);
    dispatch(&mut app, Message::Navigate(Destination::Favorites));
    assert!(matches!(app.dialog, Some(Dialog::LeaveEdits)));
    assert_eq!(app.destination, Destination::Library);
    assert_eq!(app.viewer.as_ref().expect("viewer").recipe.quarter_turns, 1);
    dispatch(&mut app, Message::DismissDialog);
    assert!(app.pending_navigation.is_none());
    assert!(app.viewer.as_ref().expect("viewer").editing);
    dispatch(&mut app, Message::Back);
    dispatch(&mut app, Message::SaveAndContinue);
    assert!(app.viewer.is_none());
    assert_eq!(
        crate::services::persistence::load(&app.paths.catalog)
            .expect("saved")
            .edits[&id]
            .quarter_turns,
        1
    );
    dispatch(&mut app, Message::Open(id.clone()));
    dispatch(&mut app, Message::BeginEdit);
    dispatch(&mut app, Message::Rotate);
    dispatch(&mut app, Message::Back);
    dispatch(&mut app, Message::DiscardAndContinue);
    assert!(app.viewer.is_none());
    assert_eq!(app.catalog.edits[&id].quarter_turns, 1);
    dispatch(&mut app, Message::Open(id));
    dispatch(&mut app, Message::Undo);
    dispatch(&mut app, Message::CancelEdit);
    assert_eq!(app.viewer.as_ref().expect("viewer").recipe.quarter_turns, 1);
}

#[cfg(unix)]
#[test]
fn routing_catalog_loading_or_damage_cannot_silently_discard_user_choices() {
    let (mut app, _sandbox) = app();
    dispatch(&mut app, Message::DialogText("too soon".into()));
    dispatch(&mut app, Message::CreateAlbum);
    assert!(app.catalog.albums.is_empty());
    assert!(app.notice.as_ref().expect("loading notice").1);
    dispatch(
        &mut app,
        Message::CatalogLoaded(Err("damaged catalog".into())),
    );
    dispatch(&mut app, Message::CreateAlbum);
    assert!(app.catalog.albums.is_empty());
    let photo = app.library.photos[0].id.clone();
    dispatch(&mut app, Message::Favorite(photo));
    assert!(app.catalog.favorites.is_empty());
    assert_eq!(app.revision, 0);
}

#[test]
fn routing_crop_draft_requires_confirmation_and_supports_cancel_undo_redo() {
    let (mut app, _sandbox) = loaded();
    let id = app.library.photos[0].id.clone();
    dispatch(&mut app, Message::Open(id));
    dispatch(&mut app, Message::BeginEdit);
    dispatch(&mut app, Message::CropPreset(1));
    let viewer = app.viewer.as_ref().expect("viewer");
    assert!(viewer.drawing_crop && viewer.crop_draft.is_some());
    assert!(viewer.recipe.crop.is_none() && viewer.history.is_empty());
    let draft = photos_library::edit::Crop {
        x: 0.1,
        y: 0.2,
        width: 0.5,
        height: 0.6,
    };
    dispatch(&mut app, Message::CropRegion(draft));
    assert_eq!(app.viewer.as_ref().expect("viewer").crop_draft, Some(draft));
    dispatch(&mut app, Message::CancelCrop);
    assert!(app.viewer.as_ref().expect("viewer").recipe.crop.is_none());
    dispatch(&mut app, Message::DrawCrop);
    dispatch(&mut app, Message::CropRegion(draft));
    dispatch(&mut app, Message::ApplyCrop);
    assert_eq!(
        app.viewer.as_ref().expect("viewer").recipe.crop,
        Some(draft)
    );
    assert_eq!(app.viewer.as_ref().expect("viewer").history.len(), 1);
    dispatch(&mut app, Message::Undo);
    assert!(app.viewer.as_ref().expect("viewer").recipe.crop.is_none());
    dispatch(&mut app, Message::Redo);
    assert_eq!(
        app.viewer.as_ref().expect("viewer").recipe.crop,
        Some(draft)
    );
    dispatch(&mut app, Message::Undo);
    dispatch(&mut app, Message::Rotate);
    assert!(app.viewer.as_ref().expect("viewer").redo.is_empty());
}

#[test]
fn closing_window_retires_chooser_parent_reply() {
    use enzui_iced_widgets::file_chooser::{ChooserOptions, PortalOperation};
    let (mut app, _sandbox) = loaded();
    let window = iced::window::Id::unique();
    app.window = Some(window);
    let request = super::state::PickRequest {
        purpose: PickPurpose::Source,
        generation: app.chooser_generation,
        load_generation: app.load_generation,
        service_generation: app.cloud.generation,
    };
    let task = app.prepare_chooser(
        request.clone(),
        PortalOperation::OpenDirectory,
        ChooserOptions::new(),
    );
    drop(task);
    let cancel = app
        .chooser_cancel
        .as_ref()
        .expect("export cancellation")
        .clone();
    dispatch(
        &mut app,
        Message::Event(
            iced::Event::Window(iced::window::Event::Closed),
            iced::event::Status::Ignored,
            window,
        ),
    );
    assert!(cancel.is_terminal());
    assert!(app.chooser_request.is_none());
    dispatch(
        &mut app,
        Message::ChooserWindow(
            request,
            window,
            Some(enzui_iced_widgets::portal_parent::ParentExport::Ready(
                enzui_iced_widgets::file_chooser::ParentHandle::x11(1),
            )),
        ),
    );
    assert!(!app.chooser.is_pending());
    assert!(app.window.is_none());
}

#[test]
fn preview_upload_cancel_and_photo_replacement_preserve_the_current_owner() {
    let (mut app, _sandbox) = loaded();
    let id = app.library.photos[0].id.clone();
    dispatch(&mut app, Message::Open(id));
    dispatch(&mut app, Message::BeginEdit);
    let before = app.viewer.as_ref().expect("viewer").preview.clone();
    let _work = app.update(Message::Adjust(Adjustment::Brightness, 25.0));
    let generation = app
        .viewer
        .as_ref()
        .expect("viewer")
        .preview_inflight
        .expect("worker");
    let owner = app.load_generation;
    let next = iced::widget::image::Handle::from_rgba(2, 2, vec![180u8; 16]);
    let upload = app.update(Message::Preview(owner, generation, Ok(next.clone())));
    assert_eq!(app.viewer.as_ref().expect("viewer").preview, before);
    assert!(
        app.viewer
            .as_ref()
            .expect("viewer")
            .preview_allocation
            .is_some()
    );
    assert!(app.viewer.as_ref().expect("viewer").preview_busy);
    settle(&mut app, upload);
    let ready = app.viewer.as_ref().expect("viewer");
    assert_eq!(ready.preview.as_ref(), Some(&next));
    assert_eq!(
        ready
            .preview_allocation
            .as_ref()
            .expect("allocation")
            .handle(),
        &next
    );
    dispatch(&mut app, Message::Compare(true));
    assert!(
        app.viewer
            .as_ref()
            .expect("viewer")
            .original_allocation
            .is_some()
    );
    dispatch(&mut app, Message::CancelEdit);
    let restored = app.viewer.as_ref().expect("viewer");
    assert_eq!(restored.preview, before);
    assert_eq!(
        restored
            .preview_allocation
            .as_ref()
            .expect("allocation")
            .handle(),
        before.as_ref().expect("frame")
    );
    let allocation = IMAGE_RENDERER
        .with(|renderer| iced::advanced::image::Renderer::load_image(renderer, &next))
        .expect("allocation");
    dispatch(&mut app, Message::BeginEdit);
    let _cancelled_work = app.update(Message::Adjust(Adjustment::Brightness, 35.0));
    let active = app.viewer.as_ref().expect("viewer").preview_inflight;
    assert_ne!(active, Some(generation));
    dispatch(
        &mut app,
        Message::PreviewReady(owner, generation, Ok(allocation.clone())),
    );
    assert_eq!(app.viewer.as_ref().expect("viewer").preview, before);
    assert_eq!(
        app.viewer.as_ref().expect("viewer").preview_inflight,
        active
    );
    dispatch(&mut app, Message::CancelEdit);
    dispatch(&mut app, Message::Next(1));
    dispatch(&mut app, Message::BeginEdit);
    let current = app.viewer.as_ref().expect("viewer").preview.clone();
    let original = app
        .viewer
        .as_ref()
        .expect("viewer")
        .original_allocation
        .as_ref()
        .map(|a| a.handle().clone());
    let work = app.update(Message::Adjust(Adjustment::Brightness, 45.0));
    let active = app
        .viewer
        .as_ref()
        .expect("viewer")
        .preview_inflight
        .expect("current worker");
    dispatch(
        &mut app,
        Message::PreviewReady(owner, active, Ok(allocation.clone())),
    );
    dispatch(&mut app, Message::OriginalReady(owner, Ok(allocation)));
    assert_eq!(app.viewer.as_ref().expect("viewer").preview, current);
    assert_eq!(
        app.viewer.as_ref().expect("viewer").preview_inflight,
        Some(active)
    );
    assert_eq!(
        app.viewer
            .as_ref()
            .expect("viewer")
            .original_allocation
            .as_ref()
            .map(|a| a.handle().clone()),
        original
    );
    settle(&mut app, work);
    assert_ne!(app.viewer.as_ref().expect("viewer").preview, current);
}

fn direct_fixture() -> (
    Photos,
    tempfile::TempDir,
    photos_library::catalog::Photo,
    Arc<crate::services::LoadedPhoto>,
) {
    let (base, sandbox) = app();
    let path = sandbox.path().join("outside-library.png");
    RgbaImage::from_pixel(18, 12, Rgba([30, 160, 90, 255]))
        .save(&path)
        .expect("outside photo");
    let photo = photos_library::scan::inspect_file(&path).expect("inspect direct file");
    let media = Arc::new(crate::services::load_photo(&path, || false).expect("load direct file"));
    let (app, _startup) = Photos::new_launch(
        base.paths.clone(),
        crate::platform::Launch {
            file: Some(path),
            ..Default::default()
        },
    );
    (app, sandbox, photo, media)
}

#[test]
fn direct_open_displays_without_waiting_for_catalog_and_then_preserves_external_photo() {
    let (mut app, _sandbox, photo, media) = direct_fixture();
    let original = std::fs::read(&photo.path).expect("original");
    assert_eq!(
        app.nav.preference,
        enzui_iced_widgets::nav_split::SidebarPreference::Hidden
    );
    dispatch(&mut app, Message::FileOpened(0, Ok((photo.clone(), media))));
    assert!(
        app.viewer
            .as_ref()
            .expect("valid test fixture")
            .preview_allocation
            .is_some()
    );
    assert!(app.catalog_authority.is_none());
    assert!(!app.opening_file);
    assert_eq!(app.scan_generation, 0);
    assert_eq!(
        crate::localization::catalog().scope(|| app.title()),
        "outside-library.png — Photos"
    );
    app.nav.preference = enzui_iced_widgets::nav_split::SidebarPreference::Shown;
    dispatch(&mut app, Message::OpenExternal(photo.path.clone()));
    assert_eq!(
        app.nav.preference,
        enzui_iced_widgets::nav_split::SidebarPreference::Hidden
    );
    let session = crate::services::persistence::open(&app.paths.catalog).expect("session");
    dispatch(&mut app, Message::CatalogLoaded(Ok(session)));
    assert!(app.scan_generation > 0);
    assert_eq!(
        app.selected_photo().expect("valid test fixture").id,
        photo.id
    );
    dispatch(&mut app, Message::Back);
    dispatch(&mut app, Message::Refresh);
    assert!(app.library.photos.iter().any(|item| item.id == photo.id));
    assert_eq!(
        std::fs::read(&photo.path).expect("valid test fixture"),
        original
    );
    assert!(
        !app.catalog
            .sources
            .contains(&photo.path.parent().expect("valid test fixture").to_owned())
    );
}

#[test]
fn routing_shortcut_rebinding_persists_dispatches_and_rejects_conflicts_and_stale_capture() {
    use enzui_iced_core::{Modifier, Shortcut};
    use enzui_iced_widgets::editable_shortcuts::EditableShortcutsEvent as Edit;
    use enzui_iced_widgets::key_capture_chip::KeyCapture;
    let (mut app, _sandbox) = loaded();
    dispatch(&mut app, Message::Dialog(Dialog::Help));
    let capture = |action: &str, event| {
        Message::Shortcut(Edit::Capture {
            action: action.into(),
            event,
        })
    };
    dispatch(&mut app, capture("photos-search", KeyCapture::Started));
    dispatch(
        &mut app,
        capture(
            "photos-search",
            KeyCapture::Candidate(Shortcut::new(Modifier::CTRL, 'k')),
        ),
    );
    assert!(app.shortcut_feedback.is_none());
    let saved = crate::services::persistence::load(&app.paths.catalog).expect("saved preferences");
    let restored =
        super::shortcuts::restore(&saved.settings.shortcut_overrides).expect("restored shortcuts");
    assert_eq!(
        restored
            .rows()
            .find(|row| row.id == "photos-search")
            .expect("search")
            .primary,
        &Shortcut::new(Modifier::CTRL, 'k')
    );
    let event = iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
        key: iced::keyboard::Key::Character("k".into()),
        modified_key: iced::keyboard::Key::Character("k".into()),
        physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyK),
        location: iced::keyboard::Location::Standard,
        modifiers: iced::keyboard::Modifiers::CTRL,
        text: None,
        repeat: false,
    });
    let window = iced::window::Id::unique();
    dispatch(
        &mut app,
        Message::Event(event.clone(), iced::event::Status::Ignored, window),
    );
    assert!(!app.search_open, "modal owns the keyboard");
    dispatch(&mut app, capture("photos-import", KeyCapture::Started));
    dispatch(
        &mut app,
        capture(
            "photos-import",
            KeyCapture::Candidate(Shortcut::new(Modifier::CTRL, 'k')),
        ),
    );
    assert!(app.shortcut_feedback.is_some());
    dispatch(&mut app, capture("photos-import", KeyCapture::Started));
    dispatch(
        &mut app,
        capture(
            "photos-import",
            KeyCapture::Candidate(Shortcut::new(Modifier::SUPER, 'k')),
        ),
    );
    assert!(app.shortcut_feedback.is_some());
    dispatch(&mut app, capture("photos-search", KeyCapture::Started));
    dispatch(&mut app, Message::DismissDialog);
    dispatch(
        &mut app,
        capture(
            "photos-search",
            KeyCapture::Candidate(Shortcut::new(Modifier::CTRL, 'l')),
        ),
    );
    dispatch(
        &mut app,
        Message::Event(event.clone(), iced::event::Status::Captured, window),
    );
    assert!(!app.search_open, "text widgets keep consumed chords");
    dispatch(
        &mut app,
        Message::Event(event, iced::event::Status::Ignored, window),
    );
    assert!(
        app.search_open,
        "the rebound chord dispatches after dismissal"
    );
    dispatch(&mut app, Message::CloseSearch);
    dispatch(&mut app, Message::Dialog(Dialog::Help));
    dispatch(
        &mut app,
        Message::Shortcut(Edit::Reset {
            action: "photos-search".into(),
        }),
    );
    assert!(app.shortcuts.overrides().is_empty());
    let saved = crate::services::persistence::load(&app.paths.catalog).expect("reset persisted");
    assert!(
        super::shortcuts::restore(&saved.settings.shortcut_overrides)
            .expect("defaults")
            .overrides()
            .is_empty()
    );
    for (action, key) in [("photos-search", 'k'), ("photos-import", 'f')] {
        dispatch(&mut app, capture(action, KeyCapture::Started));
        dispatch(
            &mut app,
            capture(
                action,
                KeyCapture::Candidate(Shortcut::new(Modifier::CTRL, key)),
            ),
        );
    }
    dispatch(
        &mut app,
        Message::Shortcut(Edit::Reset {
            action: "photos-search".into(),
        }),
    );
    assert!(
        app.shortcut_feedback.is_some(),
        "reset cannot steal another custom chord"
    );
    assert_eq!(app.shortcuts.overrides().len(), 2);
    dispatch(&mut app, Message::Shortcut(Edit::ResetAll));
    assert!(app.shortcuts.overrides().is_empty());
    let saved =
        crate::services::persistence::load(&app.paths.catalog).expect("reset all persisted");
    assert!(
        super::shortcuts::restore(&saved.settings.shortcut_overrides)
            .expect("all defaults")
            .overrides()
            .is_empty()
    );
}

#[test]
fn routing_unavailable_portal_falls_back_and_retires_location_events_on_close() {
    use enzui_iced_core::portal::backend::{BackendError, MockBackend};
    use enzui_iced_widgets::file_chooser::fallback::FallbackEvent;
    use enzui_iced_widgets::file_chooser::{ChooserOptions, ChooserSession, PortalOperation};
    let (mut app, sandbox) = loaded();
    app.chooser =
        ChooserSession::with_backend(Box::new(MockBackend::failing(BackendError::Unavailable)))
            .with_fallback();
    let window = iced::window::Id::unique();
    app.window = Some(window);
    let request = super::state::PickRequest {
        purpose: PickPurpose::Source,
        generation: app.chooser_generation,
        load_generation: app.load_generation,
        service_generation: app.cloud.generation,
    };
    app.chooser
        .start(
            request,
            PortalOperation::OpenDirectory,
            ChooserOptions::new().current_folder(sandbox.path()),
        )
        .expect("fallback starts");
    assert!(app.chooser.is_fallback_open());
    let key = iced::keyboard::Key::Character("a".into());
    dispatch(
        &mut app,
        Message::Event(
            iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: key.clone(),
                modified_key: key,
                physical_key: iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::KeyA),
                location: iced::keyboard::Location::Standard,
                modifiers: iced::keyboard::Modifiers::CTRL,
                text: None,
                repeat: false,
            }),
            iced::event::Status::Ignored,
            window,
        ),
    );
    assert!(
        app.selection.is_empty(),
        "the fallback owns keyboard commands"
    );
    let generation = app.chooser_generation;
    let mut ui = super::render_tests::simulator(&app, 375.0, 800.0);
    ui.draw(&app.theme());
    assert!(ui.find("Cancel").is_ok());
    drop(ui);
    dispatch(
        &mut app,
        Message::ChooserFallback(generation, FallbackEvent::EditLocation),
    );
    let location = app.paths.photos.to_string_lossy().into_owned();
    dispatch(
        &mut app,
        Message::ChooserFallback(generation, FallbackEvent::LocationChanged(location)),
    );
    dispatch(
        &mut app,
        Message::ChooserFallback(generation, FallbackEvent::GoToLocation),
    );
    dispatch(
        &mut app,
        Message::Event(
            iced::Event::Window(iced::window::Event::Closed),
            iced::event::Status::Ignored,
            window,
        ),
    );
    assert!(!app.chooser.is_pending());
    dispatch(
        &mut app,
        Message::ChooserFallback(generation, FallbackEvent::Accept),
    );
    assert!(app.catalog.sources.is_empty());
    assert!(!app.busy);
    let request = super::state::PickRequest {
        purpose: PickPurpose::Source,
        generation: app.chooser_generation,
        load_generation: app.load_generation,
        service_generation: app.cloud.generation,
    };
    app.chooser
        .start(
            request,
            PortalOperation::OpenDirectory,
            ChooserOptions::new().current_folder(sandbox.path()),
        )
        .expect("new fallback starts");
    dispatch(
        &mut app,
        Message::ChooserFallback(generation, FallbackEvent::Cancel),
    );
    assert!(
        app.chooser.is_fallback_open(),
        "old control events cannot close a newer chooser"
    );
    let current = app.chooser_generation;
    dispatch(
        &mut app,
        Message::ChooserFallback(current, FallbackEvent::Cancel),
    );
    assert!(!app.chooser.is_pending());
}
