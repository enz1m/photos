use std::path::PathBuf;
use std::sync::Arc;

use enzui_services::media::{LoadOptions, MediaSource, VideoService, VideoServiceConfig};
use iced::Task;

use super::state::Streaming;
use super::{Message, Photos};

impl Photos {
    pub(crate) fn preview_control(
        &mut self,
        generation: u64,
        intent: enzui_iced_widgets::media_controls::MediaControlsMessage,
    ) -> Task<Message> {
        use enzui_iced_widgets::media_controls::MediaControlsMessage as Intent;
        if generation != self.load_generation {
            return Task::none();
        }
        match intent {
            Intent::PlayPause => return self.update(Message::Playback),
            Intent::Seek(position) => {
                if let Some(stream) = self.viewer.as_ref().and_then(|v| v.stream.as_ref()) {
                    if stream.status.seekable
                        && let Some(duration) = stream.status.duration
                    {
                        return self
                            .update(Message::VideoSeek(position.min(duration).as_secs_f32()));
                    }
                } else if self.viewer.as_ref().is_some_and(|v| v.motion().is_some()) {
                    return self.update(Message::MotionSeek(
                        position.as_secs_f32() * crate::services::formats::MOTION_FPS,
                    ));
                }
            }
            Intent::SetVolume(volume) => {
                if volume.is_finite()
                    && let Some(stream) = self.viewer.as_ref().and_then(|v| v.stream.as_ref())
                    && stream.status.has_audio
                {
                    stream.player.set_volume(volume.clamp(0.0, 1.0));
                }
            }
            Intent::ToggleMute => return self.update(Message::VideoMute),
            Intent::PreviousTrack
            | Intent::NextTrack
            | Intent::ToggleShuffle
            | Intent::CycleRepeat => {}
        }
        Task::none()
    }

    pub(crate) fn start_media(&mut self) -> Task<Message> {
        let Some(viewer) = self.viewer.as_mut() else {
            return Task::none();
        };
        let Some(loaded) = &viewer.loaded else {
            return Task::none();
        };
        if let Some(path) = &loaded.stream {
            match VideoService::new(VideoServiceConfig::default()) {
                Ok(mut player) => {
                    player.set_frame_limit(Some((1280, 720)));
                    player.load(
                        MediaSource::local(path.clone()),
                        LoadOptions {
                            autoplay: false,
                            ..Default::default()
                        },
                    );
                    viewer.stream = Some(Streaming {
                        show_frame: loaded.video_info.is_some(),
                        status: player.status(),
                        player,
                        presenter: Default::default(),
                        uploading: false,
                        pending: None,
                        last_frame: None,
                    });
                }
                Err(error) => self.notice(format!("Playback unavailable: {error}"), true),
            }
            return Task::none();
        }
        if !loaded.animate {
            return Task::none();
        }
        let Some(path) = self.selected_photo().map(|p| p.path.clone()) else {
            return Task::none();
        };
        let generation = self.load_generation;
        let task = self.workers.submit(generation, move |context| {
            crate::services::formats::motion(&path, &|| context.is_cancelled())
                .map(|motion| motion.map(Arc::new))
                .map_err(|e| e.to_string())
        });
        if let Some(viewer) = self.viewer.as_mut() {
            viewer.animation_cancel = Some(task.cancellation());
        }
        Task::perform(task, move |settled| {
            Message::AnimationLoaded(
                generation,
                settled
                    .into_result()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
            )
        })
    }

    pub(crate) fn video_wake(&mut self, generation: u64) -> Task<Message> {
        if generation != self.load_generation {
            return Task::none();
        }
        let Some(stream) = self.viewer.as_mut().and_then(|v| v.stream.as_mut()) else {
            return Task::none();
        };
        let update = stream.player.poll();
        let mut error = None;
        if let Some(status) = update.status {
            if status.error != stream.status.error {
                error = status.error.as_ref().map(ToString::to_string);
            }
            stream.status = status;
        }
        let frame = update.frame;
        if let Some(viewer) = self.viewer.as_mut()
            && let Some(editor) = &mut viewer.video_editor
            && let Some(stream) = &viewer.stream
            && editor.previewing
            && (stream.status.position.as_secs_f32() >= self.video_edit.end
                || stream.status.playback == enzui_services::media::PlaybackState::EndOfStream)
        {
            if editor.loop_range {
                stream.player.seek(
                    std::time::Duration::from_secs_f32(self.video_edit.start),
                    enzui_services::media::SeekMode::Accurate,
                );
                stream.player.play();
            } else {
                editor.previewing = false;
                stream.player.pause();
                let interval = 1.0 / stream.status.frame_rate.unwrap_or(30.0).max(1.0) as f32;
                stream.player.seek(
                    std::time::Duration::from_secs_f32(
                        (self.video_edit.end - interval).max(self.video_edit.start),
                    ),
                    enzui_services::media::SeekMode::Accurate,
                );
            }
        }
        let task = frame.map_or_else(Task::none, |frame| self.queue_video_frame(Arc::new(frame)));
        if let Some(error) = error {
            self.notice(format!("Playback: {error}"), true);
        }
        task
    }

    pub(crate) fn video_ready(
        &mut self,
        generation: u64,
        ready: enzui_iced_widgets::video::ReadyFrame,
    ) -> Task<Message> {
        if generation != self.load_generation {
            return Task::none();
        }
        let Some(stream) = self.viewer.as_mut().and_then(|v| v.stream.as_mut()) else {
            return Task::none();
        };
        stream.presenter.ready(ready);
        self.advance_video_frame()
    }

    fn queue_video_frame(
        &mut self,
        frame: Arc<enzui_services::media::Rgba8Frame>,
    ) -> Task<Message> {
        let Some(viewer) = self.viewer.as_mut() else {
            return Task::none();
        };
        let Some(stream) = &mut viewer.stream else {
            return Task::none();
        };
        if viewer.video_editor.is_some() {
            stream.last_frame = Some(frame.clone());
        }
        if stream.uploading {
            stream.pending = Some(frame);
            return Task::none();
        }
        stream.uploading = true;
        let generation = self.load_generation;
        let frame = if viewer.video_editor.is_none() {
            match Arc::try_unwrap(frame) {
                Ok(frame) => {
                    return stream
                        .presenter
                        .offer(frame)
                        .map(move |ready| Message::VideoReady(generation, ready));
                }
                Err(frame) => frame,
            }
        } else {
            frame
        };
        let revision = viewer.video_revision;
        let edit = if viewer.video_editor.is_some() {
            self.video_edit
        } else {
            Default::default()
        };
        let task = self.interactive.submit(generation, move |context| {
            if context.is_cancelled() {
                return Err("preview cancelled".to_owned());
            }
            crate::services::video::transform_frame(&frame, edit).map_err(|e| e.to_string())
        });
        Task::perform(task, move |settled| {
            Message::VideoTransformed(
                generation,
                revision,
                settled
                    .into_result()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
            )
        })
    }

    fn advance_video_frame(&mut self) -> Task<Message> {
        let Some(stream) = self.viewer.as_mut().and_then(|v| v.stream.as_mut()) else {
            return Task::none();
        };
        stream.uploading = false;
        let pending = stream.pending.take();
        pending.map_or_else(Task::none, |frame| self.queue_video_frame(frame))
    }

    pub(crate) fn video_transformed(
        &mut self,
        generation: u64,
        revision: u64,
        result: Result<enzui_services::media::Rgba8Frame, String>,
    ) -> Task<Message> {
        if generation != self.load_generation {
            return Task::none();
        }
        let Some(viewer) = self.viewer.as_mut() else {
            return Task::none();
        };
        if revision != viewer.video_revision {
            return self.advance_video_frame();
        }
        let Some(stream) = &mut viewer.stream else {
            return Task::none();
        };
        match result {
            Ok(frame) => stream
                .presenter
                .offer(frame)
                .map(move |ready| Message::VideoEditedReady(generation, revision, ready)),
            Err(error) => {
                tracing::warn!(%error, "video preview unavailable");
                self.advance_video_frame()
            }
        }
    }

    pub(crate) fn video_edited_ready(
        &mut self,
        generation: u64,
        revision: u64,
        ready: enzui_iced_widgets::video::ReadyFrame,
    ) -> Task<Message> {
        if generation != self.load_generation {
            return Task::none();
        }
        if self
            .viewer
            .as_ref()
            .is_some_and(|v| v.video_revision == revision)
            && let Some(stream) = self.viewer.as_mut().and_then(|v| v.stream.as_mut())
        {
            stream.presenter.ready(ready);
        }
        self.advance_video_frame()
    }

    pub(crate) fn save_video_frame(&mut self) -> Task<Message> {
        if let Some(editor) = self.viewer.as_mut().and_then(|v| v.video_editor.as_mut()) {
            editor.previewing = false;
        }
        let Some(viewer) = self.viewer.as_ref() else {
            return Task::none();
        };
        let Some(stream) = &viewer.stream else {
            return Task::none();
        };
        let Some(path) = viewer.loaded.as_ref().and_then(|p| p.stream.clone()) else {
            return Task::none();
        };
        stream.player.pause();
        let duration = stream.status.duration.map_or(f32::MAX, |d| d.as_secs_f32());
        let frame_interval = 1.0 / stream.status.frame_rate.unwrap_or(30.0).max(1.0) as f32;
        let seconds = stream
            .status
            .position
            .as_secs_f32()
            .min((duration - frame_interval).max(0.0));
        self.busy = true;
        let generation = self.load_generation;
        let edit = if self
            .viewer
            .as_ref()
            .is_some_and(|v| v.video_editor.is_some())
        {
            self.video_edit
        } else {
            Default::default()
        };
        let task = self.interactive.submit(generation, move |context| {
            crate::services::video::extract_edited_frame(&path, seconds, edit, &|| {
                context.is_cancelled()
            })
            .map_err(|e| e.to_string())
        });
        Task::perform(task, move |settled| {
            Message::FrameSaved(
                generation,
                settled
                    .into_result()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
            )
        })
    }

    pub(crate) fn export_video(&mut self, destination: PathBuf) -> Task<Message> {
        let Some(path) = self.selected_photo().map(|p| p.path.clone()) else {
            return Task::none();
        };
        let edit = self.video_edit;
        self.busy = true;
        if let Some(editor) = self.viewer.as_mut().and_then(|v| v.video_editor.as_mut()) {
            editor.exporting = true;
        }
        self.notice("Exporting video…", false);
        let generation = self.load_generation;
        let task = self.workers.submit(generation, move |context| {
            crate::services::video::export(&path, &destination, edit, &|| context.is_cancelled())
                .map_err(|e| e.to_string())
        });
        Task::perform(task, |settled| {
            Message::ExportFinished(
                settled
                    .into_result()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
            )
        })
    }

    pub(crate) fn video_duration(&self) -> f32 {
        self.viewer
            .as_ref()
            .and_then(|v| v.loaded.as_ref())
            .and_then(|p| p.video_info.as_ref())
            .map_or(0.0, |i| i.duration)
    }

    pub(crate) fn open_video_editor(&mut self) -> Task<Message> {
        let duration = self.video_duration();
        if duration <= 0.0
            || self
                .viewer
                .as_ref()
                .is_some_and(|v| v.video_editor.is_some())
        {
            return Task::none();
        }
        let Some(path) = self.selected_photo().map(|p| p.path.clone()) else {
            return Task::none();
        };
        if self.video_edit.end == 0.0 {
            self.video_edit.end = duration;
        }
        self.dialog_text = path
            .with_file_name(format!(
                "{}-edited.mp4",
                path.file_stem().unwrap_or_default().to_string_lossy()
            ))
            .to_string_lossy()
            .into_owned();
        let Some(viewer) = self.viewer.as_mut() else {
            return Task::none();
        };
        viewer.video_revision = viewer.video_revision.wrapping_add(1);
        let session = viewer.video_revision;
        let playback_mute = viewer.stream.as_ref().is_some_and(|s| s.status.muted);
        if let Some(stream) = &mut viewer.stream {
            stream.player.pause();
            stream.show_frame = true;
            stream.player.set_rate(self.video_edit.speed as f64);
            stream.player.set_mute(self.video_edit.mute);
        }
        let generation = self.load_generation;
        let task = self.workers.submit(generation, move |context| {
            crate::services::video::filmstrip(&path, duration, &|| context.is_cancelled())
                .map_err(|e| e.to_string())
        });
        viewer.video_editor = Some(super::state::VideoEditor {
            session,
            before: self.video_edit,
            history: Vec::new(),
            redo: Vec::new(),
            gesture: None,
            loop_range: false,
            previewing: false,
            exporting: false,
            thumbnails: Vec::new(),
            thumbnail_cancel: Some(task.cancellation()),
            playback_mute,
        });
        let strip = Task::perform(task, move |settled| {
            Message::VideoFilmstrip(
                generation,
                session,
                settled
                    .into_result()
                    .map_err(|e| e.to_string())
                    .and_then(|r| r),
            )
        });
        let seek = self.update(Message::VideoSeek(self.video_edit.start));
        Task::batch([strip, seek])
    }

    pub(crate) fn video_gesture(&mut self, start: bool) {
        let Some(editor) = self.viewer.as_mut().and_then(|v| v.video_editor.as_mut()) else {
            return;
        };
        if start {
            editor.gesture.get_or_insert(self.video_edit);
        } else if let Some(before) = editor.gesture.take()
            && before != self.video_edit
        {
            editor.history.push(before);
            editor.redo.clear();
        }
    }

    pub(crate) fn change_video(
        &mut self,
        change: impl FnOnce(&mut crate::services::video::VideoEdit),
        seek: Option<f32>,
    ) -> Task<Message> {
        let before = self.video_edit;
        let Some(viewer) = self.viewer.as_mut() else {
            return Task::none();
        };
        let Some(editor) = &mut viewer.video_editor else {
            return Task::none();
        };
        change(&mut self.video_edit);
        if self.video_edit == before {
            return Task::none();
        }
        if editor.gesture.is_none() {
            editor.history.push(before);
            editor.redo.clear();
        }
        viewer.video_revision = viewer.video_revision.wrapping_add(1);
        let frame = viewer.stream.as_ref().and_then(|s| s.last_frame.clone());
        if let Some(stream) = &viewer.stream {
            if before.speed != self.video_edit.speed {
                stream.player.set_rate(self.video_edit.speed as f64);
            }
            if before.mute != self.video_edit.mute {
                stream.player.set_mute(self.video_edit.mute);
            }
        }
        if let Some(seconds) = seek {
            if let Some(editor) = self.viewer.as_mut().and_then(|v| v.video_editor.as_mut()) {
                editor.previewing = false;
            }
            if let Some(stream) = self.viewer.as_ref().and_then(|v| v.stream.as_ref()) {
                stream.player.pause();
            }
            return self.update(Message::VideoSeek(
                seconds.clamp(self.video_edit.start, self.video_edit.end),
            ));
        }
        frame.map_or_else(Task::none, |frame| self.queue_video_frame(frame))
    }

    pub(crate) fn undo_video(&mut self, undo: bool) -> Task<Message> {
        self.video_gesture(false);
        let Some(viewer) = self.viewer.as_mut() else {
            return Task::none();
        };
        let Some(editor) = &mut viewer.video_editor else {
            return Task::none();
        };
        editor.previewing = false;
        let next = if undo {
            editor.history.pop()
        } else {
            editor.redo.pop()
        };
        let Some(next) = next else {
            return Task::none();
        };
        if undo {
            editor.redo.push(self.video_edit);
        } else {
            editor.history.push(self.video_edit);
        }
        self.video_edit = next;
        viewer.video_revision = viewer.video_revision.wrapping_add(1);
        if let Some(stream) = &viewer.stream {
            stream.player.pause();
            stream.player.set_rate(next.speed as f64);
            stream.player.set_mute(next.mute);
        }
        self.update(Message::VideoSeek(next.start))
    }

    pub(crate) fn close_video_editor(&mut self, cancel: bool) -> Task<Message> {
        let Some(viewer) = self.viewer.as_mut() else {
            return Task::none();
        };
        let Some(editor) = viewer.video_editor.take() else {
            return Task::none();
        };
        if let Some(token) = editor.thumbnail_cancel {
            token.cancel();
        }
        if cancel {
            self.video_edit = editor.before;
        }
        viewer.video_revision = viewer.video_revision.wrapping_add(1);
        if let Some(stream) = &mut viewer.stream {
            stream.player.pause();
            stream.player.set_rate(1.0);
            stream.player.set_mute(editor.playback_mute);
            stream.last_frame = None;
            stream.player.seek(
                stream.status.position,
                enzui_services::media::SeekMode::Accurate,
            );
        }
        Task::none()
    }

    pub(crate) fn play_video_range(&mut self) -> Task<Message> {
        let Some(viewer) = self.viewer.as_mut() else {
            return Task::none();
        };
        let Some(stream) = &mut viewer.stream else {
            return Task::none();
        };
        let Some(editor) = &mut viewer.video_editor else {
            return Task::none();
        };
        stream.show_frame = true;
        if editor.previewing || stream.status.is_playing() {
            editor.previewing = false;
            stream.player.pause();
        } else {
            editor.previewing = true;
            let position = stream.status.position.as_secs_f32();
            if position < self.video_edit.start || position >= self.video_edit.end - 0.05 {
                stream.player.seek(
                    std::time::Duration::from_secs_f32(self.video_edit.start),
                    enzui_services::media::SeekMode::Accurate,
                );
            }
            stream.player.play();
        }
        Task::none()
    }
}
