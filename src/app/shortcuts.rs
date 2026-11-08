use super::{Dialog, Message, Photos};
use enzui_iced_core::localization::text;
use enzui_iced_core::{Modifier, NamedKey, Shortcut, ShortcutBinding};

pub(crate) fn catalog(app: &Photos) -> Vec<ShortcutBinding<Message>> {
    if app.cloud.open {
        use super::cloud::Event;
        let back = if app.cloud.busy && app.cloud.cancellable() {
            Event::Cancel
        } else {
            Event::Back
        };
        let mut bindings = vec![
            binding(app, "photos-help", Message::Dialog(Dialog::Help)),
            binding(app, "photos-back", Message::Cloud(back)),
        ];
        if app.cloud.preview.is_some() && app.cloud.connected() && !app.cloud.search_open {
            bindings.extend([
                binding(app, "photos-previous", Message::Next(-1)),
                binding(app, "photos-next", Message::Next(1)),
            ]);
            if !app.cloud.busy {
                bindings.extend([
                    binding(app, "photos-copy-image", Message::CopyImage),
                    binding(app, "photos-save-copy", Message::SaveCopy),
                ]);
            }
        }
        if app.cloud.connected() && !app.cloud.busy {
            bindings.push(binding(
                app,
                "photos-select-all",
                Message::Cloud(Event::SelectAll),
            ));
            bindings.push(binding(
                app,
                "photos-search",
                Message::Cloud(Event::FocusSearch),
            ));
        }
        return bindings;
    }

    if app.search_open {
        return vec![
            binding(app, "photos-search", Message::FocusSearch),
            binding(app, "photos-close-search", Message::CloseSearch),
        ];
    }
    let mut bindings = vec![
        binding(app, "photos-search", Message::FocusSearch),
        binding(app, "photos-import", Message::Dialog(Dialog::Import)),
        binding(app, "photos-help", Message::Dialog(Dialog::Help)),
    ];
    if app
        .viewer
        .as_ref()
        .is_some_and(|viewer| viewer.video_editor.is_some())
    {
        bindings.extend([
            binding(app, "photos-video-mark-in", Message::VideoMark(true)),
            binding(app, "photos-video-mark-out", Message::VideoMark(false)),
            binding(app, "photos-video-play", Message::Playback),
            binding(app, "photos-video-previous", Message::VideoPreviousFrame),
            binding(app, "photos-video-next", Message::VideoStep),
            binding(app, "photos-video-close", Message::VideoClose(true)),
            binding(app, "photos-video-undo", Message::VideoUndo(true)),
            binding(app, "photos-video-redo", Message::VideoUndo(false)),
        ]);
        return bindings;
    }
    if let Some(viewer) = &app.viewer {
        bindings.extend([
            binding(app, "photos-copy-image", Message::CopyImage),
            binding(app, "photos-save-copy", Message::SaveCopy),
            binding(app, "photos-edit", Message::BeginEdit),
            binding(app, "photos-favorite", Message::Favorite(viewer.id.clone())),
            binding(app, "photos-previous", Message::Next(-1)),
            binding(app, "photos-next", Message::Next(1)),
        ]);
        if viewer.editing {
            bindings.extend([
                binding(app, "photos-save-edit", Message::SaveEdit),
                binding(app, "photos-undo", Message::Undo),
                binding(app, "photos-redo", Message::Redo),
            ]);
        }
    } else {
        bindings.push(binding(app, "photos-select-all", Message::SelectAll));
    }
    let back = if app.viewer.is_none()
        && app.selection.mode() == enzui_iced_core::list_selection::SelectionMode::Selection
    {
        Message::SelectionMode(false)
    } else if app.viewer.as_ref().is_some_and(|viewer| viewer.editing) {
        Message::CancelEdit
    } else {
        Message::Back
    };
    bindings.push(binding(app, "photos-back", back));
    bindings
}

fn binding(app: &Photos, id: &'static str, command: Message) -> ShortcutBinding<Message> {
    let definition = app
        .shortcuts
        .resolved()
        .iter()
        .find(|row| row.id() == id)
        .expect("known shortcut action");
    ShortcutBinding::new(id, command, definition.primary().clone())
        .with_description(definition.description())
        .with_section(definition.section())
}

pub(crate) fn defaults() -> Vec<ShortcutBinding<()>> {
    vec![
        ShortcutBinding::new("photos-search", (), Shortcut::new(Modifier::CTRL, 'f'))
            .with_description(text("photos-search"))
            .with_section("Application"),
        ShortcutBinding::new(
            "photos-close-search",
            (),
            Shortcut::new(Modifier::NONE, NamedKey::Escape),
        )
        .with_description(text("photos-close-search"))
        .with_section("Application"),
        ShortcutBinding::new("photos-import", (), Shortcut::new(Modifier::CTRL, 'i'))
            .with_description(text("photos-import"))
            .with_section("Application"),
        ShortcutBinding::new(
            "photos-help",
            (),
            Shortcut::new(Modifier::NONE, NamedKey::F1),
        )
        .with_description(text("photos-help"))
        .with_section("Application"),
        ShortcutBinding::new(
            "photos-video-mark-in",
            (),
            Shortcut::new(Modifier::NONE, 'i'),
        )
        .with_description(text("photos-mark-trim-start"))
        .with_section("Video editing"),
        ShortcutBinding::new(
            "photos-video-mark-out",
            (),
            Shortcut::new(Modifier::NONE, 'o'),
        )
        .with_description(text("photos-mark-trim-end"))
        .with_section("Video editing"),
        ShortcutBinding::new(
            "photos-video-play",
            (),
            Shortcut::new(Modifier::NONE, NamedKey::Space),
        )
        .with_description(text("photos-play-or-pause"))
        .with_section("Video editing"),
        ShortcutBinding::new(
            "photos-video-previous",
            (),
            Shortcut::new(Modifier::NONE, NamedKey::ArrowLeft),
        )
        .with_description(text("photos-previous-frame"))
        .with_section("Video editing"),
        ShortcutBinding::new(
            "photos-video-next",
            (),
            Shortcut::new(Modifier::NONE, NamedKey::ArrowRight),
        )
        .with_description(text("photos-next-frame"))
        .with_section("Video editing"),
        ShortcutBinding::new(
            "photos-video-close",
            (),
            Shortcut::new(Modifier::NONE, NamedKey::Escape),
        )
        .with_description(text("photos-cancel-video-edits"))
        .with_section("Video editing"),
        ShortcutBinding::new("photos-video-undo", (), Shortcut::new(Modifier::CTRL, 'z'))
            .with_description(text("photos-undo-video-edit"))
            .with_section("Video editing"),
        ShortcutBinding::new(
            "photos-video-redo",
            (),
            Shortcut::new(Modifier::CTRL | Modifier::SHIFT, 'z'),
        )
        .with_description(text("photos-redo-video-edit"))
        .with_section("Video editing"),
        ShortcutBinding::new("photos-copy-image", (), Shortcut::new(Modifier::CTRL, 'c'))
            .with_description("Copy image")
            .with_section("Photo viewer"),
        ShortcutBinding::new(
            "photos-save-copy",
            (),
            Shortcut::new(Modifier::CTRL | Modifier::SHIFT, 's'),
        )
        .with_description("Save a copy")
        .with_section("Photo viewer"),
        ShortcutBinding::new("photos-edit", (), Shortcut::new(Modifier::NONE, 'e'))
            .with_description(text("photos-edit-photo"))
            .with_section("Photo viewer"),
        ShortcutBinding::new("photos-favorite", (), Shortcut::new(Modifier::NONE, 'f'))
            .with_description(text("photos-toggle-favorite"))
            .with_section("Photo viewer"),
        ShortcutBinding::new(
            "photos-previous",
            (),
            Shortcut::new(Modifier::NONE, NamedKey::ArrowLeft),
        )
        .with_description(text("photos-previous-photo"))
        .with_section("Photo viewer"),
        ShortcutBinding::new(
            "photos-next",
            (),
            Shortcut::new(Modifier::NONE, NamedKey::ArrowRight),
        )
        .with_description(text("photos-next-photo"))
        .with_section("Photo viewer"),
        ShortcutBinding::new("photos-save-edit", (), Shortcut::new(Modifier::CTRL, 's'))
            .with_description(text("photos-save-edits"))
            .with_section("Photo editing"),
        ShortcutBinding::new("photos-undo", (), Shortcut::new(Modifier::CTRL, 'z'))
            .with_description(text("photos-undo-edit"))
            .with_section("Photo editing"),
        ShortcutBinding::new(
            "photos-redo",
            (),
            Shortcut::new(Modifier::CTRL | Modifier::SHIFT, 'z'),
        )
        .with_description(text("photos-redo-edit"))
        .with_section("Photo editing"),
        ShortcutBinding::new("photos-select-all", (), Shortcut::new(Modifier::CTRL, 'a'))
            .with_description(text("photos-select-all-photos"))
            .with_section("Application"),
        ShortcutBinding::new(
            "photos-back",
            (),
            Shortcut::new(Modifier::NONE, NamedKey::Escape),
        )
        .with_description(text("photos-cancel-or-back"))
        .with_section("Application"),
    ]
}

pub(crate) fn restore(body: &str) -> Result<enzui_iced_core::EditableShortcutCatalog<()>, String> {
    use enzui_iced_core::persist::Persistable;
    let overrides = enzui_iced_core::ShortcutOverrides::persist_deserialize(body)?;
    Ok(enzui_iced_core::EditableShortcutCatalog::from_overrides(
        defaults(),
        overrides,
    ))
}

impl Photos {
    pub(crate) fn edit_shortcut(
        &mut self,
        event: enzui_iced_widgets::editable_shortcuts::EditableShortcutsEvent,
    ) -> iced::Task<Message> {
        use enzui_iced_core::editable_shortcuts::ShortcutValidity;
        use enzui_iced_core::persist::Persistable;
        use enzui_iced_widgets::editable_shortcuts::EditableShortcutsEvent;
        use enzui_iced_widgets::key_capture_chip::KeyCapture;

        if !matches!(self.dialog, Some(Dialog::Settings)) || self.settings_page != "shortcuts" {
            return iced::Task::none();
        }
        let result = match event {
            EditableShortcutsEvent::Capture {
                action,
                event: KeyCapture::Started,
            } => {
                if self.shortcuts.rows().any(|row| row.id == action) {
                    self.shortcut_capture = Some(action);
                    self.shortcut_feedback = None;
                }
                return iced::Task::none();
            }
            EditableShortcutsEvent::Capture { action, event } => {
                if self.shortcut_capture.as_deref() != Some(&action) {
                    return iced::Task::none();
                }
                self.shortcut_capture = None;
                self.shortcut_feedback = None;
                match event {
                    KeyCapture::Candidate(candidate) => {
                        let result = self.shortcuts.try_rebind(&action, candidate, |chord| {
                            if chord.modifiers().contains(Modifier::SUPER)
                                || *chord == Shortcut::new(Modifier::ALT, NamedKey::F4)
                                || *chord
                                    == Shortcut::new(
                                        Modifier::CTRL | Modifier::ALT,
                                        NamedKey::Delete,
                                    )
                            {
                                ShortcutValidity::Reserved(
                                    "This shortcut is reserved by the desktop.".into(),
                                )
                            } else {
                                ShortcutValidity::Valid
                            }
                        });
                        result.map_err(|reason| (action, reason.message()))
                    }
                    KeyCapture::Cleared => self.reset_shortcut(&action),
                    KeyCapture::Cancelled => return iced::Task::none(),
                    KeyCapture::Started => unreachable!(),
                }
            }
            EditableShortcutsEvent::Reset { action } => {
                self.shortcut_capture = None;
                self.shortcut_feedback = None;
                self.reset_shortcut(&action)
            }
            EditableShortcutsEvent::ResetAll => {
                self.shortcut_capture = None;
                self.shortcut_feedback = None;
                self.shortcuts.reset_all();
                Ok(())
            }
        };
        if let Err(feedback) = result {
            self.shortcut_feedback = Some(feedback);
            return iced::Task::none();
        }
        match self.shortcuts.overrides().persist_serialize() {
            Ok(body) => {
                self.catalog.settings.shortcut_overrides = body;
                self.changed()
            }
            Err(error) => {
                self.notice(format!("Cannot save shortcuts: {error}"), true);
                iced::Task::none()
            }
        }
    }

    fn reset_shortcut(&mut self, action: &str) -> Result<(), (String, String)> {
        use enzui_iced_core::editable_shortcuts::RebindRefusal;
        let Some(default) = self
            .shortcuts
            .defaults()
            .iter()
            .find(|row| row.id() == action)
        else {
            return Err((
                action.into(),
                RebindRefusal::UnknownAction(action.into()).message(),
            ));
        };
        if let Some(owner) = self
            .shortcuts
            .rows()
            .find(|row| row.id != action && row.overridden && row.primary == default.primary())
        {
            return Err((
                action.into(),
                RebindRefusal::Conflict {
                    action_id: owner.id.into(),
                    action_name: owner.name.into(),
                }
                .message(),
            ));
        }
        self.shortcuts.reset(action);
        Ok(())
    }
}
