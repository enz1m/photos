use enzui_iced_core::localization::text;
use enzui_iced_widgets::preferences_group::enz_preferences_group;
use enzui_iced_widgets::preferences_row::enz_preferences_row;
use enzui_iced_widgets::settings_dialog::{DialogSize, SettingsPage, enz_settings_dialog};
use enzui_iced_widgets::text::{TextSize, enz_text};
use enzui_iced_widgets::toolbar_icon::enz_toolbar_icon;
use iced::widget::{Space, column, container, responsive, row};
use iced::{Element, Length};
use photos_library::catalog::{GallerySections, ImportLayout};

use crate::app::{Message, Photos};

pub(super) fn info(id: &str, title: &str, explanation: &str) -> Element<'static, Message> {
    enz_toolbar_icon("info", Message::Ignore)
        .id(iced::widget::Id::from(format!("photos-settings-info-{id}")))
        .aria_label(format!("About {title}"))
        .tooltip(explanation.to_owned())
        .into_element()
}

pub(super) fn heading<'a>(
    title: &str,
    id: &str,
    explanation: &str,
    action: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let mut header = row![
        enz_text(title.to_owned())
            .size(TextSize::Body)
            .weight_rung(enzui_iced_theme::WeightRung::Semibold),
        info(id, title, explanation),
        Space::new().width(Length::Fill),
    ]
    .spacing(4.0)
    .align_y(iced::Alignment::Center);
    if let Some(action) = action {
        header = header.push(action);
    }
    header.into()
}

pub(super) fn choice<'a>(
    app: &Photos,
    key: &'static str,
    title: &'static str,
    current: &str,
    choices: Vec<(String, Message)>,
) -> Element<'a, Message> {
    enz_preferences_row(title)
        .id(iced::widget::Id::from(format!("photos-setting-{key}")))
        .accessory_width(150.0)
        .label_min_width(90.0)
        .value_menu(
            current.to_owned(),
            app.settings_menu.as_deref() == Some(key),
            Message::SettingsMenu(Some(key.into())),
            Message::SettingsMenu(None),
            choices,
        )
        .into_element()
}

fn section<'a>(title: &str, explanation: &str, body: Element<'a, Message>) -> Element<'a, Message> {
    enz_preferences_group(
        column![
            // On the rows' label edge, as the framework's group titles are.
            container(heading(
                title,
                &title.to_lowercase().replace(' ', "-"),
                explanation,
                None
            ))
            .padding(iced::Padding::ZERO.left(
                enzui_iced_widgets::preferences_row::PreferencesRowTokens::default().padding_x
            )),
            body
        ]
        .spacing(4.0),
    )
    .aria_label(title.to_owned())
    .into_element()
}

pub(crate) fn view(app: &Photos) -> Element<'_, Message> {
    responsive(move |size| {
        let prefs = &app.catalog.settings;
        let sections = choice(app, "sections", "Date separators", match prefs.sections { GallerySections::None => "None", GallerySections::Month => "Months", GallerySections::Day => "Days" }, vec![
            ("None".into(), Message::GallerySections(GallerySections::None)),
            ("Months".into(), Message::GallerySections(GallerySections::Month)),
            ("Days".into(), Message::GallerySections(GallerySections::Day)),
        ]);
        let sorting = choice(app, "sort", "Photo order", if prefs.oldest_first { "Oldest first" } else { "Newest first" }, vec![
            ("Newest first".into(), Message::OldestFirst(false)), ("Oldest first".into(), Message::OldestFirst(true)),
        ]);
        let cache_label = if prefs.thumbnail_cache_mb == 0 { "Off".into() } else { format!("{} MB", prefs.thumbnail_cache_mb) };
        let cache = choice(app, "thumbnail-cache", "Cache limit", &cache_label, [0, 10, 30, 50, 100, 250, 500].into_iter().map(|mb| (if mb == 0 { "Off".into() } else { format!("{mb} MB") }, Message::ThumbnailCacheLimit(mb))).collect());
        let usage = enz_preferences_row(format!("{:.1} MB · {} previews", app.thumbnail_usage.bytes as f64 / 1_000_000.0, app.thumbnail_usage.entries))
            .control(enz_toolbar_icon("trash-2", Message::ClearThumbnailCache).aria_label(text("photos-clear-cache")).tooltip("Clear thumbnail cache").id(iced::widget::Id::new("photos-clear-thumbnail-cache")))
            .accessory_width(32.0).label_min_width(100.0).id(iced::widget::Id::new("photos-cache-usage"));
        let general = column![
            section("Library", "Date separators group photo rows. Sorting changes their chronological order.", column![sections, sorting].into()),
            section("Previews", "Local thumbnail previews are saved on disk between launches. Clearing this cache never removes photos. Recent remote thumbnails are kept separately in memory.", column![cache, usage].into()),
            section("Display", "Full thumbnails fit the whole photo inside each tile. The information panel setting is remembered when reopening Photos.", column![
                enz_preferences_row("Show full thumbnails").switch(prefs.contain_thumbnails, Message::ContainThumbnails).id(iced::widget::Id::new("photos-setting-contain")),
                enz_preferences_row("Show names on hover").switch(prefs.show_names, Message::ShowNames).id(iced::widget::Id::new("photos-setting-names")),
                enz_preferences_row("Show photo information").switch(app.catalog.viewer_info, |_| Message::ToggleInfo).id(iced::widget::Id::new("photos-setting-info")),
            ].into()),
        ].spacing(8.0);
        let mut folders = vec![app.paths.photos.clone()];
        for folder in &app.catalog.sources { if !folders.contains(folder) { folders.push(folder.clone()); } }
        if !folders.iter().any(|path| path == app.import_destination()) { folders.push(app.import_destination().to_owned()); }
        let label = |path: &std::path::Path| {
            if path == app.paths.photos { return "Home / Photos".to_owned(); }
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            if folders.iter().filter(|folder| *folder != &app.paths.photos && folder.file_name() == path.file_name()).count() > 1 { format!("{name} · {}", path.parent().unwrap_or(path).display()) } else { name.into_owned() }
        };
        let destination = choice(app, "destination", "Import into", &label(app.import_destination()), folders.iter().map(|folder| (label(folder), Message::ImportDestination(folder.clone()))).collect());
        let layout = choice(app, "layout", "Folder layout", match prefs.import_layout { ImportLayout::Flat => "One folder", ImportLayout::PerImport => "Per import", ImportLayout::YearMonth => "Year / month" }, vec![
            ("One folder".into(), Message::ImportLayout(ImportLayout::Flat)), ("Per import".into(), Message::ImportLayout(ImportLayout::PerImport)), ("Year / month".into(), Message::ImportLayout(ImportLayout::YearMonth)),
        ]);
        let example = match prefs.import_layout { ImportLayout::Flat => "Photos / IMG_0001.jpg", ImportLayout::PerImport => "Photos / 2026-10-09 143000 Camera / IMG_0001.jpg", ImportLayout::YearMonth => "Photos / 2026 / 10 / IMG_0001.jpg" };
        let import_help = format!("Imports create new copies and never replace originals. Add destination folders from the library sidebar. Current destination: {}. Example layout: {example}. Year/month uses the capture date, falling back to the file date.", app.import_destination().display());
        let import = section("Import copies", &import_help, column![destination, layout].into());
        let shortcuts = column![
            container(heading("Keyboard shortcuts", "shortcuts", "Click a shortcut to record a replacement. Escape cancels recording; Backspace restores its default. Reset all shortcuts restores every default.", None))
                .padding(iced::Padding::ZERO.left(enzui_iced_widgets::preferences_row::PreferencesRowTokens::default().padding_x)),
            enzui_iced_widgets::editable_shortcuts::enz_editable_shortcuts(&app.shortcuts, app.shortcut_capture.as_deref(), Message::Shortcut)
                // The heading above already titles the page, with its reset-all control.
                .heading(false)
                .feedback(app.shortcut_feedback.as_ref().map(|(action, reason)| (action.as_str(), reason.as_str()))).into_element(),
        ].spacing(8.0);
        // Four framework pages: tabs on a desktop window, a page list with
        // Back on a narrow one, and a sheet from the bottom edge when small.
        let selected = match app.settings_page.as_str() {
            "imports" | "shortcuts" | "services" => app.settings_page.as_str(),
            _ => "gallery",
        };
        let builder = enz_settings_dialog(true, size)
            .title(text("photos-photos-settings"))
            .page_id_prefix("photos-settings-dialog")
            .size(DialogSize::Small)
            .page(SettingsPage::new("gallery", "General"), general)
            .page(SettingsPage::new("imports", "Imports"), import)
            .page(SettingsPage::new("shortcuts", "Shortcuts"), shortcuts)
            .page_lazy(SettingsPage::new("services", "Services"), move || super::cloud::settings(app))
            .selected(selected)
            .on_select(Message::SettingsPage)
            .stacked_navigation(app.settings_page_open, Message::SettingsBack)
            .on_done(Message::DismissDialog);
        // The shortcuts page's reset sits in the dialog's footer, under a
        // divider, as in Mail, Messages and Terminal, while the page is shown.
        let shortcuts_shown = selected == "shortcuts"
            && (app.settings_page_open
                || builder.resolved_nav() != enzui_iced_widgets::settings_dialog::SettingsNav::Stacked);
        let builder = if shortcuts_shown {
            builder.action(enzui_iced_widgets::editable_shortcuts::editable_shortcuts_reset_all_action(|| {
                Message::Shortcut(enzui_iced_widgets::editable_shortcuts::EditableShortcutsEvent::ResetAll)
            }))
        } else {
            builder
        };
        let sheet = builder.host_placement() == enzui_iced_widgets::modal_host::SurfacePlacement::BottomEdge;
        let surface = builder.into_presented_surface(Some(Box::new(|| Message::DismissDialog)));
        if sheet {
            return container(surface).center_x(Length::Fill).align_bottom(Length::Fill);
        }
        container(surface).center_x(Length::Fill).center_y(Length::Fill)
    }).into()
}
