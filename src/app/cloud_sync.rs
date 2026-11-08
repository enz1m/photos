use std::collections::{BTreeMap, VecDeque};
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use enzui_iced_core::blocking_task::BlockingTaskOwner;
use enzui_services::credential_store::{CredentialOutcome, connect_for_app};
use iced::Task;
use photos_library::catalog::{
    PhotoId, ServiceAccount, ServiceSync, SyncDirection, TransferRule, TransferSource,
};
use sha2::{Digest, Sha256};

use super::{Message, Photos, cloud};
use crate::services::remote::{
    self, ApiKey, PhotoService, RemoteAlbum,
    sync::{Report, UploadScope},
};

#[derive(Debug, Clone)]
pub(crate) enum Event {
    New(SyncDirection),
    CancelForm,
    Server(u64),
    Local(Option<u64>),
    Remote(Option<RemoteAlbum>),
    Folder(PathBuf),
    Add,
    Toggle(u64, bool),
    Remove(u64),
    Now(u64),
    Tick,
    Cancel,
    DraftClient(u64, u64, Result<Arc<dyn PhotoService>, String>),
    Albums(u64, u64, Result<Vec<RemoteAlbum>, String>),
    Ready(
        u64,
        u64,
        TransferRule,
        Result<Arc<dyn PhotoService>, String>,
    ),
    Done(u64, u64, TransferRule, Report),
}

pub(crate) struct Draft {
    pub direction: SyncDirection,
    pub account: u64,
    pub local: Option<u64>,
    pub remote: Option<RemoteAlbum>,
    pub folder: PathBuf,
    pub albums: Vec<RemoteAlbum>,
    pub loading: bool,
    pub error: Option<String>,
}

pub(crate) struct State {
    pub draft: Option<Draft>,
    pub status: BTreeMap<u64, String>,
    pub running: Option<(u64, u64)>,
    generation: u64,
    form_generation: u64,
    queue: VecDeque<u64>,
    flag: Option<Arc<AtomicBool>>,
    owner: BlockingTaskOwner,
    form_owner: BlockingTaskOwner,
}
impl State {
    pub fn new() -> Self {
        let owner = |name| {
            BlockingTaskOwner::new(name, NonZeroUsize::MIN, NonZeroUsize::MIN).unwrap_or_else(
                |error| {
                    tracing::error!(%error, "cannot start transfer worker");
                    std::process::exit(1)
                },
            )
        };
        Self {
            draft: None,
            status: BTreeMap::new(),
            running: None,
            generation: 0,
            form_generation: 0,
            queue: VecDeque::new(),
            flag: None,
            owner: owner("photos-service-transfers"),
            form_owner: owner("photos-transfer-albums"),
        }
    }
    pub fn cancel(&mut self) {
        self.queue.clear();
        if let Some(flag) = &self.flag {
            flag.store(true, Ordering::Release);
        }
    }
    pub fn close_form(&mut self) {
        self.form_generation = self.form_generation.wrapping_add(1);
        self.draft = None;
    }
}
fn message(event: Event) -> Message {
    Message::Cloud(cloud::Event::Sync(event))
}
async fn client(
    catalog: PathBuf,
    account: ServiceAccount,
) -> Result<Arc<dyn PhotoService>, String> {
    let store = connect_for_app("org.enzimos.Photos").await;
    match store.get(cloud::credential(&catalog, &account)).await {
        CredentialOutcome::Retrieved(secret) => remote::connect(
            &account,
            ApiKey::new(String::from_utf8_lossy(secret.as_bytes()).into_owned()),
        )
        .map_err(|e| e.to_string()),
        _ => Err("Unlock the keyring and reconnect this server to resume transfers.".into()),
    }
}
impl Photos {
    pub(crate) fn cloud_sync_event(&mut self, event: Event) -> Task<Message> {
        match event {
            Event::New(direction) => {
                let Some(account) = self
                    .cloud
                    .active
                    .or_else(|| self.catalog.services.first().map(|a| a.id))
                else {
                    return Task::none();
                };
                self.cloud.sync.draft = Some(Draft {
                    direction,
                    account,
                    local: None,
                    remote: None,
                    folder: self.import_destination().to_owned(),
                    albums: Vec::new(),
                    loading: false,
                    error: None,
                });
                return self.cloud_sync_event(Event::Server(account));
            }
            Event::CancelForm => self.cloud.sync.close_form(),
            Event::Server(id) => {
                self.settings_menu = None;
                let Some(account) = self.catalog.services.iter().find(|a| a.id == id).cloned()
                else {
                    return Task::none();
                };
                let Some(draft) = &mut self.cloud.sync.draft else {
                    return Task::none();
                };
                draft.account = id;
                draft.remote = None;
                draft.albums.clear();
                draft.loading = true;
                draft.error = None;
                self.cloud.sync.form_generation = self.cloud.sync.form_generation.wrapping_add(1);
                let generation = self.cloud.sync.form_generation;
                if let Some(client) = self.cloud.clients.get(&id).cloned() {
                    return self.cloud_sync_event(Event::DraftClient(generation, id, Ok(client)));
                }
                return Task::perform(client(self.paths.catalog.clone(), account), move |result| {
                    message(Event::DraftClient(generation, id, result))
                });
            }
            Event::DraftClient(generation, id, result) => {
                if generation != self.cloud.sync.form_generation
                    || self
                        .cloud
                        .sync
                        .draft
                        .as_ref()
                        .is_none_or(|d| d.account != id)
                {
                    return Task::none();
                }
                let client = match result {
                    Ok(client) => client,
                    Err(error) => {
                        if let Some(draft) = &mut self.cloud.sync.draft {
                            draft.loading = false;
                            draft.error = Some(error);
                        }
                        return Task::none();
                    }
                };
                self.cloud.clients.insert(id, client.clone());
                let work = self
                    .cloud
                    .sync
                    .form_owner
                    .submit(generation, move |context| {
                        client
                            .albums(&|| context.is_cancelled())
                            .map_err(|e| e.to_string())
                    });
                return Task::perform(work, move |settled| {
                    message(Event::Albums(
                        generation,
                        id,
                        settled
                            .into_result()
                            .map_err(|e| e.to_string())
                            .and_then(|result| result),
                    ))
                });
            }
            Event::Albums(generation, id, result) => {
                if generation != self.cloud.sync.form_generation {
                    return Task::none();
                }
                if let Some(draft) = &mut self.cloud.sync.draft
                    && draft.account == id
                {
                    draft.loading = false;
                    match result {
                        Ok(albums) => draft.albums = albums,
                        Err(error) => draft.error = Some(error),
                    }
                }
            }
            Event::Local(album) => {
                self.settings_menu = None;
                if let Some(draft) = &mut self.cloud.sync.draft {
                    draft.local = album;
                }
            }
            Event::Remote(album) => {
                self.settings_menu = None;
                if let Some(draft) = &mut self.cloud.sync.draft {
                    draft.remote = album;
                }
            }
            Event::Folder(folder) => {
                self.settings_menu = None;
                if let Some(draft) = &mut self.cloud.sync.draft {
                    draft.folder = folder;
                }
            }
            Event::Add => {
                if self.catalog_authority.is_none()
                    || self.persistence_blocked
                    || self.catalog.transfer_rules.len() >= 512
                {
                    return Task::none();
                }
                let Some(draft) = self.cloud.sync.draft.as_ref() else {
                    return Task::none();
                };
                if !self.catalog.services.iter().any(|a| a.id == draft.account)
                    || !draft.folder.is_absolute()
                    || draft
                        .local
                        .is_some_and(|id| !self.catalog.albums.iter().any(|a| a.id == id))
                {
                    return Task::none();
                }
                let Some(next) = self.catalog.next_transfer_rule.checked_add(1) else {
                    return Task::none();
                };
                let rule = TransferRule {
                    account: draft.account,
                    enabled: false,
                    source: draft
                        .local
                        .map_or(TransferSource::Library, TransferSource::Album),
                    remote_label: draft.remote.as_ref().map_or_else(
                        || "Entire server library".into(),
                        |a| {
                            a.name
                                .chars()
                                .filter(|c| !c.is_control())
                                .take(512)
                                .collect()
                        },
                    ),
                    plan: ServiceSync {
                        folder: draft.folder.clone(),
                        direction: draft.direction,
                        album: draft.remote.as_ref().map(|a| a.id.clone()),
                    },
                    legacy_receipts: false,
                };
                self.catalog
                    .transfer_rules
                    .insert(self.catalog.next_transfer_rule, rule);
                self.catalog.next_transfer_rule = next;
                self.catalog.version = 4;
                self.cloud.sync.close_form();
                return self.changed();
            }
            Event::Toggle(id, enabled) => {
                if self.catalog_authority.is_none() || self.persistence_blocked {
                    return Task::none();
                }
                let Some(rule) = self.catalog.transfer_rules.get_mut(&id) else {
                    return Task::none();
                };
                rule.enabled = enabled;
                if !enabled
                    && self
                        .cloud
                        .sync
                        .running
                        .is_some_and(|(running, _)| running == id)
                {
                    self.cloud.sync.cancel();
                }
                return Task::batch([
                    self.changed(),
                    if enabled {
                        self.cloud_sync_event(Event::Now(id))
                    } else {
                        Task::none()
                    },
                ]);
            }
            Event::Remove(id) => {
                if self.catalog_authority.is_none() || self.persistence_blocked {
                    return Task::none();
                }
                self.catalog.transfer_rules.remove(&id);
                self.cloud.sync.status.remove(&id);
                if self
                    .cloud
                    .sync
                    .running
                    .is_some_and(|(running, _)| running == id)
                {
                    self.cloud.sync.cancel();
                }
                self.cloud.sync.queue.retain(|queued| *queued != id);
                return self.changed();
            }
            Event::Cancel => self.cloud.sync.cancel(),
            Event::Tick => {
                if self.cloud.sync.running.is_none() {
                    self.cloud.sync.queue = self
                        .catalog
                        .transfer_rules
                        .iter()
                        .filter(|(_, rule)| rule.enabled)
                        .map(|(id, _)| *id)
                        .collect();
                    return self.next_transfer();
                }
            }
            Event::Now(id) => {
                if self.cloud.sync.running.is_some()
                    || self.catalog_authority.is_none()
                    || self.persistence_blocked
                    || self.close_requested
                {
                    return Task::none();
                }
                let Some(rule) = self.catalog.transfer_rules.get(&id).cloned() else {
                    return Task::none();
                };
                let Some(account) = self
                    .catalog
                    .services
                    .iter()
                    .find(|a| a.id == rule.account)
                    .cloned()
                else {
                    return Task::none();
                };
                self.cloud.sync.generation = self.cloud.sync.generation.wrapping_add(1);
                let generation = self.cloud.sync.generation;
                self.cloud.sync.running = Some((id, generation));
                self.cloud.sync.flag = Some(Arc::new(AtomicBool::new(false)));
                self.cloud
                    .sync
                    .status
                    .insert(id, "Transferring new originals…".into());
                if let Some(client) = self.cloud.clients.get(&rule.account).cloned() {
                    return self.cloud_sync_event(Event::Ready(generation, id, rule, Ok(client)));
                }
                return Task::perform(client(self.paths.catalog.clone(), account), move |result| {
                    message(Event::Ready(generation, id, rule.clone(), result))
                });
            }
            Event::Ready(generation, id, rule, result) => {
                if self.cloud.sync.running != Some((id, generation)) {
                    return Task::none();
                }
                if self
                    .cloud
                    .sync
                    .flag
                    .as_ref()
                    .is_none_or(|f| f.load(Ordering::Acquire))
                    || self.catalog.transfer_rules.get(&id) != Some(&rule)
                {
                    self.cloud.sync.running = None;
                    return self.next_transfer();
                }
                let client = match result {
                    Ok(client) => client,
                    Err(error) => {
                        self.cloud.sync.running = None;
                        self.cloud.sync.status.insert(id, error);
                        return self.next_transfer();
                    }
                };
                let scope = match self.transfer_upload_scope(&rule) {
                    Ok(scope) => scope,
                    Err(error) => {
                        self.cloud.sync.running = None;
                        self.cloud.sync.status.insert(id, error);
                        return self.next_transfer();
                    }
                };
                let account = self
                    .catalog
                    .services
                    .iter()
                    .find(|a| a.id == rule.account)
                    .expect("rule account");
                // Receipt identities remain stable when a two-way plan migrates to independent rules.
                let key = if rule.legacy_receipts {
                    format!(
                        "{}:{}:{}:{:?}",
                        rule.account,
                        account.url,
                        rule.plan.folder.display(),
                        rule.plan.album
                    )
                } else {
                    format!(
                        "rule:{id}:{}:{}:{}:{:?}",
                        rule.account,
                        account.url,
                        rule.plan.folder.display(),
                        rule.plan.album
                    )
                };
                let state = self.paths.catalog.with_file_name(format!(
                    "service-sync-{:x}.json",
                    Sha256::digest(key.as_bytes())
                ));
                let flag = self
                    .cloud
                    .sync
                    .flag
                    .as_ref()
                    .expect("transfer flag")
                    .clone();
                let config = rule.plan.clone();
                let work = self.cloud.sync.owner.submit(generation, move |context| {
                    remote::sync::run_scoped(
                        client.as_ref(),
                        &config,
                        &state,
                        scope.as_ref(),
                        &|| context.is_cancelled() || flag.load(Ordering::Acquire),
                    )
                });
                return Task::perform(work, move |settled| {
                    message(Event::Done(
                        generation,
                        id,
                        rule.clone(),
                        settled.into_result().unwrap_or_else(|e| Report {
                            error: Some(e.to_string()),
                            ..Default::default()
                        }),
                    ))
                });
            }
            Event::Done(generation, id, rule, report) => {
                if self.cloud.sync.running != Some((id, generation)) {
                    return Task::none();
                }
                self.cloud.sync.running = None;
                self.cloud.sync.flag = None;
                self.cloud.sync.status.insert(
                    id,
                    format!(
                        "{} downloaded · {} uploaded · {} unchanged · {} conflicts. {}",
                        report.downloaded,
                        report.uploaded,
                        report.unchanged,
                        report.conflicts,
                        report.error.unwrap_or_default()
                    ),
                );
                let mut jobs = vec![self.next_transfer()];
                if report.uploaded > 0 {
                    jobs.push(self.scan());
                }
                if !report.paths.is_empty()
                    && self.catalog_authority.is_some()
                    && !self.persistence_blocked
                {
                    let mut changed = false;
                    for path in &report.paths {
                        changed |= self.catalog.imported.insert(PhotoId::for_path(path));
                    }
                    if rule.plan.folder != self.paths.photos
                        && !self.catalog.sources.contains(&rule.plan.folder)
                    {
                        self.catalog.sources.push(rule.plan.folder);
                        changed = true;
                    }
                    if changed || report.downloaded > 0 {
                        jobs.extend([self.changed(), self.scan()]);
                    }
                }
                return Task::batch(jobs);
            }
        }
        self.continue_close()
    }
    fn next_transfer(&mut self) -> Task<Message> {
        while let Some(id) = self.cloud.sync.queue.pop_front() {
            if self
                .catalog
                .transfer_rules
                .get(&id)
                .is_some_and(|rule| rule.enabled)
            {
                return self.cloud_sync_event(Event::Now(id));
            }
        }
        self.continue_close()
    }
    fn transfer_upload_scope(&self, rule: &TransferRule) -> Result<Option<UploadScope>, String> {
        if rule.plan.direction == SyncDirection::Download || rule.source == TransferSource::Folder {
            return Ok(None);
        }
        let members = match rule.source {
            TransferSource::Album(id) => Some(
                self.catalog
                    .albums
                    .iter()
                    .find(|a| a.id == id)
                    .ok_or("The source album is unavailable; upload stopped.")?
                    .photos
                    .difference(&self.catalog.hidden)
                    .cloned()
                    .collect(),
            ),
            _ => None,
        };
        let mut roots = vec![self.paths.photos.clone()];
        for root in &self.catalog.sources {
            if !roots.contains(root) {
                roots.push(root.clone());
            }
        }
        Ok(Some(UploadScope {
            roots,
            members,
            excluded: self.catalog.hidden.clone(),
        }))
    }
}
