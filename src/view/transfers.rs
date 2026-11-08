use enzui_iced_widgets::button::{ButtonKind, ButtonSize, enz_button};
use enzui_iced_widgets::preferences_group::enz_preferences_group;
use enzui_iced_widgets::preferences_row::enz_preferences_row;
use enzui_iced_widgets::switch::{SwitchGeometry, switch};
use enzui_iced_widgets::text::{TextSize, TextTone, enz_text};
use enzui_iced_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::{Space, column, row};
use iced::{Element, Length};
use photos_library::catalog::{SyncDirection, TransferSource};

use crate::app::{
    Message, Photos, cloud,
    cloud_sync::{Draft, Event},
};

fn message(event: Event) -> Message {
    Message::Cloud(cloud::Event::Sync(event))
}
fn action<'a>(label: &str, id: &str, event: Event) -> Element<'a, Message> {
    enz_button(label.to_owned(), message(event))
        .kind(ButtonKind::Quiet)
        .size(ButtonSize::Sm)
        .id(iced::widget::Id::from(format!("photos-transfer-{id}")))
        .into_element()
}
fn icon<'a>(
    name: &str,
    label: &str,
    id: &str,
    event: Event,
    enabled: bool,
) -> Element<'a, Message> {
    enz_toolbar_icon(name.to_owned(), message(event))
        .aria_label(label.to_owned())
        .tooltip(label.to_owned())
        .id(iced::widget::Id::from(format!("photos-transfer-{id}")))
        .enabled(enabled)
        .into_element()
}
fn folder(app: &Photos, path: &std::path::Path) -> String {
    if path == app.paths.photos {
        "Photos library".into()
    } else {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned()
    }
}
fn local_label(app: &Photos, source: &TransferSource, path: &std::path::Path) -> String {
    match source {
        TransferSource::Library => "Entire local library".into(),
        TransferSource::Album(id) => app
            .catalog
            .albums
            .iter()
            .find(|a| a.id == *id)
            .map_or_else(|| "Missing local album".into(), |a| a.name.clone()),
        TransferSource::Folder => folder(app, path),
    }
}
fn server_label(app: &Photos, id: u64) -> String {
    app.catalog
        .services
        .iter()
        .find(|a| a.id == id)
        .map_or_else(
            || "Missing server".into(),
            |a| {
                if app
                    .catalog
                    .services
                    .iter()
                    .filter(|b| b.name == a.name)
                    .count()
                    > 1
                {
                    format!("{} · {} ({id})", a.name, a.url)
                } else {
                    a.name.clone()
                }
            },
        )
}
pub(super) fn view(app: &Photos, direction: SyncDirection) -> Element<'_, Message> {
    let upload = direction == SyncDirection::Upload;
    let title = if upload {
        "Automatic uploads"
    } else {
        "Automatic downloads"
    };
    let mut body = column![super::settings::heading(title, "automatic-transfers",
        "Each rule is off until enabled. Enabled rules check for new originals every minute while Photos is open. Files are copied, never overwritten or deleted; removals and edit recipes are not mirrored. The play button runs a rule once. Removing a rule keeps all existing copies.",
        Some(enz_button("Add", message(Event::New(direction))).icon("plus").kind(ButtonKind::Quiet).size(ButtonSize::Sm)
            .aria_label(format!("Add automatic {} rule", if upload { "upload" } else { "download" }))
            .id(iced::widget::Id::new("photos-transfer-new")).enabled(!app.catalog.services.is_empty() && app.cloud.sync.draft.is_none()).into_element()))].spacing(8.0);
    let mut rows = column![].spacing(4.0);
    let mut count = 0;
    for (id, rule) in app
        .catalog
        .transfer_rules
        .iter()
        .filter(|(_, rule)| rule.plan.direction == direction)
    {
        count += 1;
        let local = local_label(app, &rule.source, &rule.plan.folder);
        let remote = &rule.remote_label;
        let title = if upload { local } else { remote.clone() };
        let target = if upload {
            remote.clone()
        } else {
            folder(app, &rule.plan.folder)
        };
        let summary = format!("{} → {target}", server_label(app, rule.account));
        let label = format!(
            "Automatic {}: {title} on {}",
            if upload { "upload" } else { "download" },
            server_label(app, rule.account)
        );
        let id = *id;
        let running = app
            .cloud
            .sync
            .running
            .is_some_and(|(active, _)| active == id);
        let mut controls = row![
            switch(rule.enabled, move |enabled| message(Event::Toggle(
                id, enabled
            )))
            .geometry(SwitchGeometry::SM)
            .label(label)
            .id(iced::widget::Id::from(format!(
                "photos-transfer-enabled-{id}"
            )))
        ]
        .spacing(4.0)
        .align_y(iced::Alignment::Center);
        if let Some(status) = app.cloud.sync.status.get(&id) {
            controls = controls.push(super::settings::info(
                &format!("transfer-status-{id}"),
                "Transfer result",
                status,
            ));
        }
        controls = controls.push(icon(
            if running { "square" } else { "play" },
            if running {
                "Stop transfer"
            } else {
                "Run rule now"
            },
            &format!("run-{id}"),
            if running {
                Event::Cancel
            } else {
                Event::Now(id)
            },
            running || app.cloud.sync.running.is_none(),
        ));
        controls = controls.push(icon(
            "trash-2",
            "Remove transfer rule",
            &format!("remove-{id}"),
            Event::Remove(id),
            true,
        ));
        rows = rows.push(
            enz_preferences_row(title)
                .subtitle(summary)
                .subtitle_wrap(true)
                .control(controls)
                .accessory_width(if app.cloud.sync.status.contains_key(&id) {
                    132.0
                } else {
                    100.0
                })
                .label_min_width(100.0)
                .id(iced::widget::Id::from(format!("photos-transfer-rule-{id}"))),
        );
    }
    if let Some(draft) = &app.cloud.sync.draft
        && draft.direction == direction
    {
        body = body.push(form(app, draft));
    }
    if count > 0 {
        body = body.push(enz_preferences_group(rows).aria_label(title));
    } else if app.cloud.sync.draft.is_none() {
        body = body.push(
            enz_text("No automatic rules")
                .size(TextSize::Footnote)
                .tone(TextTone::Muted),
        );
    }
    body.width(Length::Fill).into()
}
fn form<'a>(app: &'a Photos, draft: &'a Draft) -> Element<'a, Message> {
    let upload = draft.direction == SyncDirection::Upload;
    let server = super::settings::choice(
        app,
        "transfer-server",
        "Server",
        &server_label(app, draft.account),
        app.catalog
            .services
            .iter()
            .map(|a| (server_label(app, a.id), message(Event::Server(a.id))))
            .collect(),
    );
    let mut body = column![server].spacing(8.0);
    if upload {
        let mut options = vec![("Entire local library".into(), message(Event::Local(None)))];
        options.extend(app.catalog.albums.iter().map(|a| {
            let label = if app
                .catalog
                .albums
                .iter()
                .filter(|b| b.name == a.name)
                .count()
                > 1
            {
                format!("{} ({})", a.name, a.id)
            } else {
                a.name.clone()
            };
            (label, message(Event::Local(Some(a.id))))
        }));
        let selected = options.iter().find(|(_, msg)| matches!(msg, Message::Cloud(cloud::Event::Sync(Event::Local(id))) if *id == draft.local)).map(|(label, _)| label.as_str()).unwrap_or("Entire local library").to_owned();
        body = body.push(super::settings::choice(
            app,
            "transfer-local",
            "From",
            &selected,
            options,
        ));
    }
    let remote_label = |album: &crate::services::remote::RemoteAlbum| {
        if draft
            .albums
            .iter()
            .filter(|other| other.name == album.name)
            .count()
            > 1
        {
            format!("{} ({})", album.name, album.id)
        } else {
            album.name.clone()
        }
    };
    let mut remotes = vec![("Entire server library".into(), message(Event::Remote(None)))];
    remotes.extend(
        draft
            .albums
            .iter()
            .map(|a| (remote_label(a), message(Event::Remote(Some(a.clone()))))),
    );
    let selected = draft
        .remote
        .as_ref()
        .map_or_else(|| "Entire server library".into(), remote_label);
    body = body.push(super::settings::choice(
        app,
        "transfer-remote",
        if upload { "To" } else { "From" },
        &selected,
        remotes,
    ));
    if !upload {
        let mut roots = vec![app.paths.photos.clone()];
        for root in &app.catalog.sources {
            if !roots.contains(root) {
                roots.push(root.clone());
            }
        }
        if !roots.iter().any(|root| root == app.import_destination()) {
            roots.push(app.import_destination().to_owned());
        }
        let label = |path: &std::path::Path| {
            let label = folder(app, path);
            if roots
                .iter()
                .filter(|root| folder(app, root) == label)
                .count()
                > 1
            {
                path.display().to_string()
            } else {
                label
            }
        };
        body = body.push(super::settings::choice(
            app,
            "transfer-folder",
            "To",
            &label(&draft.folder),
            roots
                .iter()
                .map(|p| (label(p), message(Event::Folder(p.clone()))))
                .collect(),
        ));
    }
    let mut footer = row![
        enz_text("Starts off")
            .size(TextSize::Footnote)
            .tone(TextTone::Muted),
        Space::new().width(Length::Fill)
    ]
    .spacing(8.0)
    .align_y(iced::Alignment::Center);
    if draft.loading {
        footer = footer.push(enz_text("Loading albums…").size(TextSize::Footnote));
    }
    if let Some(error) = &draft.error {
        footer = footer.push(super::settings::info(
            "transfer-albums-error",
            "Server albums unavailable",
            error,
        ));
    }
    footer = footer
        .push(action("Cancel", "cancel-form", Event::CancelForm))
        .push(action("Add rule", "add", Event::Add));
    body.push(footer).into()
}
