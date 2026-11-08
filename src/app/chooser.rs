use enzui_iced_core::portal::cancel::PortalCancel;
use enzui_iced_widgets::file_chooser::{ChooserOptions, ChooserParent, PortalOperation};
use enzui_iced_widgets::portal_parent::{ParentExport, export_parent, export_parent_task};
use iced::Task;

use super::state::{ParentHandoff, PickRequest};
use super::{Message, Photos};

impl Photos {
    pub(super) fn prepare_chooser(
        &mut self,
        request: PickRequest,
        operation: PortalOperation,
        options: ChooserOptions,
    ) -> Task<Message> {
        self.chooser_request = Some((request.clone(), operation, options));
        self.chooser_cancel = Some(PortalCancel::new());
        iced::window::oldest().then(move |window| {
            let Some(window) = window else {
                return Task::done(Message::ChooserUnavailable(request.clone()));
            };
            let request = request.clone();
            iced::window::run(window, |native| {
                native
                    .window_handle()
                    .ok()
                    .map(|handle| export_parent(&handle.as_raw()))
            })
            .map(move |parent| Message::ChooserWindow(request.clone(), window, parent))
        })
    }

    fn owns_chooser(&self, request: &PickRequest) -> bool {
        self.chooser_request
            .as_ref()
            .is_some_and(|(pending, _, _)| pending == request)
            && request.generation == self.chooser_generation
            && request.load_generation == self.load_generation
            && request.service_generation == self.cloud.generation
            && !self.close_requested
    }

    pub(super) fn chooser_window(
        &mut self,
        request: PickRequest,
        window: iced::window::Id,
        parent: Option<ParentExport>,
    ) -> Task<Message> {
        if !self.owns_chooser(&request) {
            return Task::none();
        }
        if self.window != Some(window) {
            self.chooser_unavailable(request);
            return Task::none();
        }
        match parent {
            Some(ParentExport::Ready(handle)) => {
                self.start_parented_chooser(request, ChooserParent::X11 { window, handle })
            }
            Some(ParentExport::NeedsWaylandExport) => {
                let cancel = self
                    .chooser_cancel
                    .as_ref()
                    .expect("owned chooser cancellation")
                    .clone();
                return export_parent_task(
                    &self.parent_exports.dispatcher(),
                    request.generation,
                    window,
                    cancel,
                    std::time::Duration::from_secs(3),
                )
                .map(move |handoff| Message::ChooserParent(request.clone(), window, handoff));
            }
            _ => self.chooser_unavailable(request),
        }
        Task::none()
    }

    pub(super) fn chooser_parent(
        &mut self,
        request: PickRequest,
        window: iced::window::Id,
        handoff: ParentHandoff,
    ) {
        let Some(settlement) = handoff.take() else {
            return;
        };
        if !self.owns_chooser(&request) || self.window != Some(window) {
            return;
        }
        match settlement.into_result() {
            Ok(enzui_iced_wayland::ParentAdoption::Parented { export, .. }) => {
                self.start_parented_chooser(request, ChooserParent::Wayland { window, export })
            }
            _ => self.chooser_unavailable(request),
        }
    }

    fn start_parented_chooser(&mut self, request: PickRequest, parent: ChooserParent) {
        if !self.owns_chooser(&request) {
            return;
        }
        let Some((_, operation, options)) = self.chooser_request.take() else {
            return;
        };
        self.chooser_cancel = None;
        let live = self.window;
        if self
            .chooser
            .start_parented(request, operation, options, parent, |window| {
                live == Some(window)
            })
            .is_err()
        {
            self.notice(
                "The parented file chooser is unavailable. Enter a local path instead.",
                true,
            );
        }
    }

    pub(super) fn chooser_unavailable(&mut self, request: PickRequest) {
        if !self.owns_chooser(&request) {
            return;
        }
        self.chooser_request = None;
        if let Some(cancel) = self.chooser_cancel.take() {
            cancel.cancel(enzui_iced_widgets::file_chooser::CancelReason::Application);
        }
        self.notice(
            "The parented file chooser is unavailable. Enter a local path instead.",
            true,
        );
    }
}
