#![forbid(unsafe_code)]

mod app;
mod localization;
mod platform;
mod services;
mod view;

fn main() -> iced::Result {
    let launch = platform::Launch::parse(std::env::args_os().skip(1)).unwrap_or_else(|error| {
        eprintln!("{error}\nUsage: enz-photos [--open] FILE | --add-folder FOLDER | --measure-thumbnails REPORT");
        std::process::exit(2);
    });
    if launch.help {
        println!(
            "Usage: enz-photos [--open] FILE\n       enz-photos --add-folder FOLDER\n       enz-photos --measure-thumbnails REPORT\nLocal file:// URIs and -- before filenames are supported."
        );
        return Ok(());
    }
    if let Some(output) = launch.measure.clone() {
        let paths = platform::Paths::discover();
        let worker = enzui_iced_core::blocking_task::BlockingTaskOwner::new(
            "thumbnail-measurement",
            std::num::NonZeroUsize::MIN,
            std::num::NonZeroUsize::MIN,
        )
        .expect("measurement worker");
        let task = worker.submit(0, move |_| {
            services::thumbnail_cache::measure(&paths, &output).map_err(|e| e.to_string())
        });
        match iced::futures::executor::block_on(task).into_result() {
            Ok(Ok(())) => return Ok(()),
            result => {
                eprintln!("Thumbnail measurement failed: {result:?}");
                std::process::exit(1);
            }
        }
    }

    let instance = if launch.add_folder.is_some()
        || std::env::var_os("ENZ_PHOTOS_STATE").is_some()
        || std::env::var_os("ENZ_PHOTOS_LIBRARY").is_some()
    {
        enzui_iced_widgets::single_instance::SingleInstance::standalone()
    } else {
        match enzui_iced_widgets::single_instance::SingleInstance::start("org.enzimos.Photos") {
            enzui_iced_widgets::single_instance::InstanceStart::Forwarded => return Ok(()),
            enzui_iced_widgets::single_instance::InstanceStart::Owner(instance, _) => instance,
        }
    };
    let instance = std::sync::Mutex::new(Some(instance));
    let identity =
        enzui_iced_widgets::app_shell::DesktopApplicationIdentity::new("org.enzimos.Photos")
            .expect("static desktop identity is valid");
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    enzui_iced_core::crash_report::CrashReporter::for_app("enz-photos", env!("CARGO_PKG_VERSION"));
    enzui_iced_widgets::app_setup::run_with_services(
        iced::application(
            move || {
                let (mut app, task) =
                    app::Photos::new_launch(platform::Paths::discover(), launch.clone());
                app.instance = instance
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
                    .unwrap_or_else(
                        enzui_iced_widgets::single_instance::SingleInstance::standalone,
                    );
                (app, task)
            },
            app::Photos::update,
            app::Photos::view,
        )
        .title(app::Photos::title)
        .subscription(app::Photos::subscription)
        .theme(app::Photos::theme)
        .exit_on_close_request(false)
        .window(
            identity.window_settings(
                enzui_iced_widgets::app_shell::ShellLayout::FloatingTitlebarSidebar,
            ),
        ),
        enzui_iced_widgets::app_setup::AppServices::default().localizer(localization::catalog()),
    )
}
