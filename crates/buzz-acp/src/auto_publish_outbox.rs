use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine;
use nostr::{Event, EventBuilder, Kind, Tag};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::time::Instant;

use crate::relay::RestClient;

const MAX_RECORDS: usize = 64;
const MAX_RECORD_BYTES: usize = 2 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 256 * 1024;
const MAX_TURN_BYTES: usize = 1024;
const MAX_REASON_BYTES: usize = 4096;
const MAX_ACK_BYTES: usize = 16 * 1024;
const MAX_BATCH: usize = 8;
const MAX_ATTEMPTS: usize = 3;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const BATCH_TIMEOUT: Duration = Duration::from_secs(20);

pub(crate) struct Outbox {
    rest: RestClient,
    http: reqwest::Client,
    directory: PathBuf,
    namespace: String,
    flushing: tokio::sync::Mutex<()>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u8,
    namespace: String,
    turn_id: String,
    payload: Payload,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Payload {
    Pending { event: Event },
    Deferred { text: String, reason: String },
}

struct Stored {
    path: PathBuf,
    stage: PathBuf,
    record: Record,
}

pub(crate) fn default_root(rest: &RestClient) -> Result<PathBuf, String> {
    Outbox::default_root(rest)
}

impl Outbox {
    pub(crate) fn default_root(_rest: &RestClient) -> Result<PathBuf, String> {
        if let Some(root) = std::env::var_os("BUZZ_ACP_OUTBOX_DIR") {
            if root.is_empty() {
                return Err("BUZZ_ACP_OUTBOX_DIR must not be empty".into());
            }
            return Ok(PathBuf::from(root));
        }
        let home_variable = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        let home = std::env::var_os(home_variable)
            .filter(|home| !home.is_empty())
            .ok_or_else(|| format!("set BUZZ_ACP_OUTBOX_DIR or {home_variable}"))?;
        Ok(PathBuf::from(home).join(".buzz").join("auto-publish-outbox"))
    }

    pub(crate) fn new(mut rest: RestClient, root: PathBuf) -> Result<Self, String> {
        let url = url::Url::parse(&rest.base_url).map_err(|_| "invalid outbox relay URL")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("outbox relay URL must be HTTP(S), without credentials/query/fragment".into());
        }
        rest.base_url = url.as_str().trim_end_matches('/').to_string();
        let namespace = format!(
            "{}-{}",
            hex::encode(Sha256::digest(rest.base_url.as_bytes())),
            rest.keys.public_key().to_hex()
        );
        if root.as_os_str().is_empty() {
            return Err("outbox root must not be empty".into());
        }
        create_private_directory(&root)?;
        let directory = root.join(&namespace);
        create_private_directory(&directory)?;
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(REQUEST_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|error| format!("outbox HTTP client: {error}"))?;
        Ok(Self {
            rest,
            http,
            directory,
            namespace,
            flushing: tokio::sync::Mutex::new(()),
        })
    }

    pub(crate) fn enqueue(&self, turn_id: &str, event: &Event) -> Result<(), String> {
        self.validate_event(event)?;
        self.persist(Record {
            version: 1,
            namespace: self.namespace.clone(),
            turn_id: turn_id.into(),
            payload: Payload::Pending { event: event.clone() },
        })?;
        Ok(())
    }

    pub(crate) fn store_deferred(
        &self,
        turn_id: &str,
        text: &str,
        reason: &str,
    ) -> Result<PathBuf, String> {
        if text.len() > MAX_TEXT_BYTES || reason.len() > MAX_REASON_BYTES {
            return Err("deferred draft exceeds 256 KiB text or 4 KiB reason limit".into());
        }
        self.persist(Record {
            version: 1,
            namespace: self.namespace.clone(),
            turn_id: turn_id.into(),
            payload: Payload::Deferred { text: text.into(), reason: reason.into() },
        })
    }

    pub(crate) async fn flush_event(&self, event_id: &str) -> Result<(), String> {
        if nostr::EventId::from_hex(event_id).is_err() {
            return Err("invalid outbox event ID".into());
        }
        let _guard = tokio::time::timeout(BATCH_TIMEOUT, self.flushing.lock())
            .await.map_err(|_| "outbox flush lock timed out")?;
        let (records, _) = self.inventory()?;
        let matching: Vec<_> = records.iter().filter(|stored| {
            matches!(&stored.record.payload, Payload::Pending { event } if event.id.to_hex() == event_id)
        }).collect();
        let stored = matching.first().ok_or("requested event is not a validated pending outbox record")?;
        let Payload::Pending { event } = &stored.record.payload else {
            return Err("requested event is not pending".into());
        };
        sync_directory(stored.path.parent().ok_or("outbox slot has no parent")?)?;
        tokio::time::timeout(BATCH_TIMEOUT, self.deliver(event))
            .await.map_err(|_| "outbox event delivery timed out; retained")??;
        for stored in matching { self.remove_accepted(stored)?; }
        Ok(())
    }

    pub(crate) async fn flush(&self) -> Result<usize, String> {
        let _guard = self.flushing.try_lock().map_err(|_| "outbox flush already in progress")?;
        let deadline = Instant::now() + BATCH_TIMEOUT;
        let (records, mut errors) = self.inventory()?;
        let mut attempted = HashSet::new();
        let mut accepted = HashSet::new();
        for stored in &records {
            let Payload::Pending { event } = &stored.record.payload else { continue; };
            let event_id = event.id.to_hex();
            if !attempted.insert(event_id.clone()) { continue; }
            if attempted.len() > MAX_BATCH || Instant::now() >= deadline {
                errors.push("outbox batch limit reached; pending records retained".into());
                break;
            }
            sync_directory(stored.path.parent().ok_or("outbox slot has no parent")?)?;
            match tokio::time::timeout_at(deadline, self.deliver(event)).await {
                Ok(Ok(())) => { accepted.insert(event_id); }
                Ok(Err(error)) => errors.push(format!("event {event_id}: {error}")),
                Err(_) => {
                    errors.push("outbox batch timed out; pending records retained".into());
                    break;
                }
            }
        }
        for stored in &records {
            if let Payload::Pending { event } = &stored.record.payload {
                if accepted.contains(&event.id.to_hex()) {
                    if let Err(error) = self.remove_accepted(stored) { errors.push(error); }
                }
            }
        }
        let (remaining, final_errors) = self.inventory()?;
        errors.extend(final_errors);
        if remaining.iter().any(|stored| matches!(stored.record.payload, Payload::Pending { .. })) {
            errors.push("outbox still has pending events".into());
        }
        if errors.is_empty() { Ok(accepted.len()) } else {
            Err(format!("outbox accepted {} event(s); {}", accepted.len(), errors.join("; ")))
        }
    }

    fn validate_event(&self, event: &Event) -> Result<(), String> {
        if event.pubkey != self.rest.keys.public_key() { return Err("foreign outbox event author".into()); }
        if event.content.len() > MAX_TEXT_BYTES { return Err("outbox event content exceeds 256 KiB".into()); }
        event.verify().map_err(|_| "invalid outbox event ID or signature".to_string())
    }

    fn validate_record(&self, record: &Record) -> Result<(), String> {
        if record.version != 1 || record.namespace != self.namespace {
            return Err("unsupported or foreign outbox namespace".into());
        }
        if record.turn_id.is_empty() || record.turn_id.len() > MAX_TURN_BYTES {
            return Err("outbox turn ID must contain 1..1024 bytes".into());
        }
        match &record.payload {
            Payload::Pending { event } => self.validate_event(event),
            Payload::Deferred { text, reason } if text.len() <= MAX_TEXT_BYTES && reason.len() <= MAX_REASON_BYTES => Ok(()),
            Payload::Deferred { .. } => Err("oversized deferred outbox record".into()),
        }
    }

    fn record_name(record: &Record, bytes: &[u8]) -> String {
        match &record.payload {
            Payload::Pending { event } => format!("{}.json", event.id.to_hex()),
            Payload::Deferred { .. } => format!("deferred-{}.json", hex::encode(Sha256::digest(bytes))),
        }
    }

    fn persist(&self, record: Record) -> Result<PathBuf, String> {
        self.validate_record(&record)?;
        let bytes = bounded_json(&record)?;
        let name = Self::record_name(&record, &bytes);
        let (existing, errors) = self.inventory()?;
        for stored in existing {
            if stored.path.file_name() == Some(std::ffi::OsStr::new(&name)) {
                sync_directory(stored.path.parent().ok_or("outbox slot has no parent")?)?;
                return Ok(stored.path);
            }
        }
        if !errors.is_empty() { return Err(format!("outbox needs recovery: {}", errors.join("; "))); }
        for index in 0..MAX_RECORDS {
            let slot = self.directory.join(format!("slot-{index:04}"));
            match private_directory_builder().create(&slot) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(format!("reserve outbox slot: {error}")),
            }
            let path = slot.join(&name);
            let stage = path.with_extension("stage");
            let result = (|| {
                sync_directory(&self.directory)?;
                let mut options = OpenOptions::new();
                options.write(true).create_new(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    options.mode(0o600);
                }
                let mut file = options.open(&stage).map_err(|error| format!("create outbox stage: {error}"))?;
                file.write_all(&bytes).and_then(|()| file.sync_all()).map_err(|error| format!("sync outbox record: {error}"))?;
                drop(file);
                fs::hard_link(&stage, &path).map_err(|error| format!("publish immutable outbox record: {error}"))?;
                sync_directory(&slot)?;
                remove_if_present(&stage)?;
                sync_directory(&slot)?;
                Ok(path.clone())
            })();
            return result.map_err(|error: String| format!("{error}; retained slot {} needs inspection", slot.display()));
        }
        Err(format!("outbox full: {MAX_RECORDS} slots, at most {} bytes; existing records retained", MAX_RECORDS * MAX_RECORD_BYTES))
    }

    fn inventory(&self) -> Result<(Vec<Stored>, Vec<String>), String> {
        require_directory(&self.directory)?;
        let mut records = Vec::new();
        let mut errors = Vec::new();
        let entries = fs::read_dir(&self.directory).map_err(|error| format!("read outbox directory: {error}"))?;
        for (count, entry) in entries.enumerate() {
            if count >= MAX_RECORDS { return Err("outbox directory exceeds slot quota; manual recovery required".into()); }
            let entry = entry.map_err(|error| format!("read outbox slot: {error}"))?;
            let name = entry.file_name();
            let valid_name = name.to_str().is_some_and(|name| {
                name.strip_prefix("slot-").and_then(|index| index.parse::<usize>().ok())
                    .is_some_and(|index| index < MAX_RECORDS && name == format!("slot-{index:04}"))
            });
            if !valid_name {
                errors.push("unexpected outbox directory entry; retained for inspection".into());
                continue;
            }
            match self.load_slot(&entry.path()) {
                Ok(Some(stored)) => records.push(stored),
                Ok(None) => {}
                Err(error) => errors.push(format!("{}: {error}", entry.path().display())),
            }
        }
        records.sort_by(|left, right| left.path.cmp(&right.path));
        Ok((records, errors))
    }

    fn load_slot(&self, slot: &Path) -> Result<Option<Stored>, String> {
        match fs::symlink_metadata(slot) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("inspect outbox slot: {error}")),
            Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => return Err("outbox slot is not a regular directory".into()),
            Ok(_) => {}
        }
        let entries = match fs::read_dir(slot) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("read outbox slot: {error}")),
        };
        let mut committed = None;
        let mut staging = None;
        for (count, entry) in entries.enumerate() {
            if count >= 2 { return Err("outbox slot exceeds file quota".into()); }
            let path = entry.map_err(|error| error.to_string())?.path();
            match path.extension().and_then(|extension| extension.to_str()) {
                Some("json") if committed.is_none() => committed = Some(path),
                Some("stage") if staging.is_none() => staging = Some(path),
                _ => return Err("unexpected outbox slot file".into()),
            }
        }
        let Some(path) = committed else { return Err("incomplete outbox slot; retained for manual recovery".into()); };
        let bytes = match read_bounded(&path) {
            Ok(bytes) => bytes,
            Err(_) if !path.try_exists().unwrap_or(true) => return Ok(None),
            Err(error) => return Err(error),
        };
        let record: Record = serde_json::from_slice(&bytes).map_err(|_| "invalid outbox JSON; retained for inspection")?;
        self.validate_record(&record)?;
        if path.file_name() != Some(std::ffi::OsStr::new(&Self::record_name(&record, &bytes))) {
            return Err("outbox filename does not match its signed event or draft".into());
        }
        let stage = path.with_extension("stage");
        if let Some(staging) = staging {
            if staging != stage { return Err("foreign outbox staging file".into()); }
            match read_bounded(&staging) {
                Ok(staged) if staged == bytes => {}
                Ok(_) => return Err("outbox staging content mismatch".into()),
                Err(_) if !staging.try_exists().unwrap_or(true) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(Some(Stored { path, stage, record }))
    }

    fn remove_accepted(&self, stored: &Stored) -> Result<(), String> {
        remove_if_present(&stored.stage)?;
        remove_if_present(&stored.path)?;
        let slot = stored.path.parent().ok_or("outbox slot has no parent")?;
        if slot.try_exists().map_err(|error| error.to_string())? {
            sync_directory(slot)?;
            match fs::remove_dir(slot) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(format!("remove accepted outbox slot: {error}")),
            }
        }
        sync_directory(&self.directory)
    }

    async fn deliver(&self, event: &Event) -> Result<(), String> {
        let body = bounded_json(event)?;
        let mut last_error = String::new();
        for attempt in 0..MAX_ATTEMPTS {
            if attempt > 0 { tokio::time::sleep(Duration::from_millis(250 << (attempt - 1))).await; }
            match self.send_once(event, &body).await {
                Ok(()) => return Ok(()),
                Err((error, retryable)) => {
                    last_error = error;
                    if !retryable { break; }
                }
            }
        }
        Err(format!("{last_error}; signed event retained for a later flush"))
    }

    async fn send_once(&self, event: &Event, body: &[u8]) -> Result<(), (String, bool)> {
        let url = format!("{}/events", self.rest.base_url);
        let auth = self.authorization(&url, body).map_err(|error| (error, false))?;
        let mut request = self.http.post(&url).header("Authorization", auth)
            .header("Content-Type", "application/json").body(body.to_vec());
        if let Some(tag) = &self.rest.auth_tag_json { request = request.header("x-auth-tag", tag); }
        let operation = async {
            let mut response = request.send().await
                .map_err(|error| (format!("outbox transport: {}", error.without_url()), true))?;
            let status = response.status();
            if !status.is_success() {
                return Err((format!("outbox HTTP {status}"), status.as_u16() == 429 || status.is_server_error()));
            }
            if response.content_length().is_some_and(|size| size > MAX_ACK_BYTES as u64) {
                return Err(("outbox ACK exceeds size limit".into(), false));
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|error| {
                (format!("outbox ACK transport: {}", error.without_url()), true)
            })? {
                if bytes.len() + chunk.len() > MAX_ACK_BYTES { return Err(("outbox ACK exceeds size limit".into(), false)); }
                bytes.extend_from_slice(&chunk);
            }
            let ack: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| ("outbox ACK is not JSON".into(), false))?;
            if ack.get("accepted").and_then(serde_json::Value::as_bool) != Some(true)
                || ack.get("event_id").and_then(serde_json::Value::as_str) != Some(event.id.to_hex().as_str())
            { return Err(("outbox ACK must accept the exact event ID".into(), false)); }
            Ok(())
        };
        tokio::time::timeout(REQUEST_TIMEOUT, operation).await
            .map_err(|_| ("outbox request timed out".into(), true))?
    }

    fn authorization(&self, url: &str, body: &[u8]) -> Result<String, String> {
        let nonce = uuid::Uuid::new_v4().to_string();
        let payload = hex::encode(Sha256::digest(body));
        let tags = [["u", url], ["method", "POST"], ["payload", payload.as_str()], ["nonce", nonce.as_str()]]
            .into_iter().map(Tag::parse).collect::<Result<Vec<_>, _>>().map_err(|_| "outbox auth tag error")?;
        let auth = EventBuilder::new(Kind::HttpAuth, "").tags(tags).sign_with_keys(&self.rest.keys)
            .map_err(|_| "outbox auth signing error")?;
        Ok(format!("Nostr {}", base64::engine::general_purpose::STANDARD.encode(bounded_json(&auth)?)))
    }
}

fn bounded_json(value: &impl Serialize) -> Result<Vec<u8>, String> {
    struct Bounded(Vec<u8>);
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_RECORD_BYTES.saturating_sub(self.0.len()) {
                return Err(std::io::Error::other("outbox record exceeds 2 MiB"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let mut writer = Bounded(Vec::new());
    serde_json::to_writer(&mut writer, value).map_err(|error| error.to_string())?;
    Ok(writer.0)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() { return Err("outbox record is not a regular file".into()); }
    if metadata.len() > MAX_RECORD_BYTES as u64 { return Err("outbox record exceeds 2 MiB".into()); }
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_RECORD_BYTES { return Err("outbox record exceeds 2 MiB".into()); }
    Ok(bytes)
}

fn private_directory_builder() -> fs::DirBuilder {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.recursive(false);
    builder
}

fn require_directory(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if metadata.is_dir() && !metadata.file_type().is_symlink() { Ok(()) }
    else { Err("outbox path is not a regular directory".into()) }
}

fn create_private_directory(path: &Path) -> Result<(), String> {
    private_directory_builder().recursive(true).create(path).map_err(|error| format!("create outbox directory: {error}"))?;
    require_directory(path)?;
    sync_directory(path)?;
    if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty()) { sync_directory(parent)?; }
    Ok(())
}

fn sync_directory(path: &Path) -> Result<(), String> {
    #[cfg(unix)]
    File::open(path).and_then(|directory| directory.sync_all()).map_err(|error| format!("sync outbox directory: {error}"))?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("remove acknowledged outbox file: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::Keys;

    #[test]
    fn test_outbox_store_deferred() {
        let keys = Keys::generate();
        let rest = RestClient {
            base_url: "http://127.0.0.1:8080".into(),
            keys,
            http: reqwest::Client::new(),
            auth_tag_json: None,
        };
        let unique = format!("test-outbox-{}", uuid::Uuid::new_v4());
        let temp_dir = std::env::temp_dir().join(unique);
        let outbox = Outbox::new(rest, temp_dir.clone()).unwrap();
        let path = outbox.store_deferred("turn-1", "some text", "some reason").unwrap();
        assert!(path.exists());
        let _ = fs::remove_dir_all(&temp_dir);
    }
}
