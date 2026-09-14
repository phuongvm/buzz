use std::fmt;

const MAX_CAPTURE_BYTES: usize = 256 * 1024;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum OutputMode {
    #[default]
    ToolOnly,
    ConversationalFinal,
}

impl OutputMode {
    pub(crate) fn parse(value: Option<&str>) -> Result<Self, String> {
        match value.map(str::trim) {
            None | Some("") | Some("tool-only") => Ok(Self::ToolOnly),
            Some("conversational-final") => Ok(Self::ConversationalFinal),
            Some(_) => Err("BUZZ_ACP_OUTPUT_MODE must be tool-only or conversational-final".into()),
        }
    }
}

#[derive(Default)]
pub(crate) struct TurnOutput {
    turn_id: Option<String>,
    text: String,
    saw_tool: bool,
    overflowed: bool,
}

impl fmt::Debug for TurnOutput {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("TurnOutput")
            .field("turn_id", &self.turn_id)
            .field("bytes", &self.text.len())
            .field("saw_tool", &self.saw_tool)
            .field("overflowed", &self.overflowed)
            .finish()
    }
}

impl TurnOutput {
    pub(crate) fn begin(mode: OutputMode, turn_id: &str) -> Self {
        Self {
            turn_id: (mode == OutputMode::ConversationalFinal).then(|| turn_id.to_owned()),
            ..Self::default()
        }
    }

    pub(crate) fn instruction(&self) -> Option<String> {
        self.turn_id.as_ref().map(|turn_id| format!(
            "Publication contract for this turn: conversational-final. The harness, not a tool, publishes your final response. Do not call tools or publish messages yourself in this mode. Return exactly one final response between [[BUZZ_FINAL:{turn_id}]] and [[/BUZZ_FINAL:{turn_id}]]. These markers must appear only once each. Put only channel-ready text inside, no thoughts, progress or drafts. Text outside the markers is not published. Keep the final response below 65536 UTF-8 bytes. If tools are necessary, do not claim success: this turn will require review instead of automatic publication."
        ))
    }

    pub(crate) fn reset(&mut self) {
        self.text.clear();
        self.saw_tool = false;
        self.overflowed = false;
    }

    pub(crate) fn push_text(&mut self, text: &str) {
        if self.turn_id.is_none() || self.overflowed { return; }
        let remaining = MAX_CAPTURE_BYTES.saturating_sub(self.text.len());
        let mut boundary = remaining.min(text.len());
        while !text.is_char_boundary(boundary) { boundary -= 1; }
        self.text.push_str(&text[..boundary]);
        self.overflowed = boundary < text.len();
    }

    pub(crate) fn observe_tool(&mut self) {
        if self.turn_id.is_some() { self.saw_tool = true; }
    }

    pub(crate) fn captured_text(&self) -> &str { &self.text }

    pub(crate) fn final_text(&self) -> Result<Option<String>, String> {
        let Some(turn_id) = &self.turn_id else { return Ok(None); };
        if self.saw_tool {
            return Err("automatic publication deferred: this conversational turn used tools; publication ownership cannot be proven".into());
        }
        if self.overflowed {
            return Err("automatic publication deferred: captured output exceeded 256 KiB; the retained draft is incomplete".into());
        }
        let opening = format!("[[BUZZ_FINAL:{turn_id}]]");
        let closing = format!("[[/BUZZ_FINAL:{turn_id}]]");
        if self.text.matches(&opening).count() != 1 || self.text.matches(&closing).count() != 1 {
            return Err("automatic publication deferred: missing or repeated final response envelope".into());
        }
        let start = self.text.find(&opening).ok_or("missing final envelope")? + opening.len();
        let end = self.text.find(&closing).ok_or("missing final envelope terminator")?;
        if end < start { return Err("automatic publication deferred: reversed final envelope".into()); }
        let content = self.text[start..end].trim();
        if content.is_empty() || content.len() > MAX_MESSAGE_BYTES {
            return Err("automatic publication deferred: final response must contain 1..65536 UTF-8 bytes".into());
        }
        let normalized = content.to_ascii_lowercase();
        if normalized.contains("<think") || normalized.contains("</think")
            || content.contains("[[BUZZ_FINAL:") || content.contains("[[/BUZZ_FINAL:") {
            return Err("automatic publication deferred: final response contains thought markup or nested envelopes".into());
        }
        Ok(Some(content.to_owned()))
    }
}


pub(crate) async fn deliver(
    output: TurnOutput,
    rest: &crate::relay::RestClient,
    batch: Option<&crate::queue::FlushBatch>,
    thread_tags: &crate::queue::ThreadTags,
    turn_id: &str,
    ended: bool,
) -> Result<(), String> {
    if output.turn_id.is_none() { return Ok(()); }
    let root = crate::auto_publish_outbox::default_root(rest)?;
    let outbox = crate::auto_publish_outbox::Outbox::new(rest.clone(), root)?;
    let final_text = if ended {
        output.final_text()
    } else {
        Err("automatic publication deferred: turn did not finish normally".into())
    };
    let content = match final_text {
        Ok(Some(content)) => content,
        Ok(None) => return Ok(()),
        Err(reason) => {
            let path = outbox.store_deferred(turn_id, output.captured_text(), &reason)?;
            return Err(format!("{reason}; draft retained at {}", path.display()));
        }
    };
    let build_result = (|| {
        let batch = batch.ok_or("automatic publication has no triggering batch")?;
        let trigger = batch.events.last().ok_or("automatic publication has no triggering event")?;
        let thread_ref = match (&thread_tags.root_event_id, &thread_tags.parent_event_id) {
            (Some(root), Some(parent)) => Some(buzz_sdk::ThreadRef {
                root_event_id: nostr::EventId::from_hex(root).map_err(|error| error.to_string())?,
                parent_event_id: nostr::EventId::from_hex(parent).map_err(|error| error.to_string())?,
            }),
            (None, None) => None,
            _ => return Err("automatic publication has incomplete reply destination".to_owned()),
        };
        let author = trigger.event.pubkey.to_hex();
        let mentions = if trigger.event.pubkey == rest.keys.public_key() { vec![] } else { vec![author.as_str()] };
        buzz_sdk::build_message(batch.channel_id, &content, thread_ref.as_ref(), &mentions, false, &[], &[])
            .map_err(|error| error.to_string())?
            .sign_with_keys(&rest.keys).map_err(|error| error.to_string())
    })();
    let event = match build_result {
        Ok(event) => event,
        Err(reason) => {
            let path = outbox.store_deferred(turn_id, output.captured_text(), &reason)?;
            return Err(format!("{reason}; draft retained at {}", path.display()));
        }
    };
    outbox.enqueue(turn_id, &event)?;
    outbox.flush_event(&event.id.to_hex()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(body: &str) -> TurnOutput {
        let mut output = TurnOutput::begin(OutputMode::ConversationalFinal, "fresh");
        output.push_text(body);
        output
    }

    #[test]
    fn tool_only_is_default_and_captures_nothing() {
        assert_eq!(OutputMode::parse(None), Ok(OutputMode::ToolOnly));
        assert!(OutputMode::parse(Some("true")).is_err());
        let mut output = TurnOutput::begin(OutputMode::ToolOnly, "fresh");
        output.push_text(&"secret".repeat(100_000));
        assert_eq!(output.captured_text(), "");
        assert_eq!(output.final_text(), Ok(None));
    }

    #[test]
    fn fragmented_envelope_ignores_progress() {
        let mut output = capture("Progress: inspect later. [[BUZZ_FI");
        output.push_text("NAL:fresh]]Xin chào 🐝[[/BUZZ_FINAL:fresh]] internal footer");
        assert_eq!(output.final_text(), Ok(Some("Xin chào 🐝".into())));
    }

    #[test]
    fn rejects_stale_missing_multiple_and_reversed_envelopes() {
        for text in [
            "raw final text",
            "[[BUZZ_FINAL:old]]reply[[/BUZZ_FINAL:old]]",
            "[[BUZZ_FINAL:fresh]]unfinished",
            "[[/BUZZ_FINAL:fresh]][[BUZZ_FINAL:fresh]]",
            "[[BUZZ_FINAL:fresh]]one[[/BUZZ_FINAL:fresh]][[BUZZ_FINAL:fresh]]two[[/BUZZ_FINAL:fresh]]",
        ] { assert!(capture(text).final_text().is_err(), "{text}"); }
    }

    #[test]
    fn rejects_thought_markup_instead_of_leaking_nested_tail() {
        for body in ["<think>outer<think>inner</think>SECRET</think>public", "<THINK>secret</THINK>", "<think>unclosed", "<think >secret</think >"] {
            let output = capture(&format!("[[BUZZ_FINAL:fresh]]{body}[[/BUZZ_FINAL:fresh]]"));
            assert!(output.final_text().is_err());
        }
    }

    #[test]
    fn tool_activity_never_means_message_was_sent() {
        let mut output = capture("[[BUZZ_FINAL:fresh]]reply[[/BUZZ_FINAL:fresh]]");
        output.observe_tool();
        assert!(output.final_text().is_err());
    }

    #[test]
    fn enforces_utf8_byte_limits_without_panicking() {
        let exactly = "a".repeat(MAX_MESSAGE_BYTES);
        assert!(capture(&format!("[[BUZZ_FINAL:fresh]]{exactly}[[/BUZZ_FINAL:fresh]]")).final_text().is_ok());
        assert!(capture(&format!("[[BUZZ_FINAL:fresh]]{exactly}é[[/BUZZ_FINAL:fresh]]")).final_text().is_err());
        let mut output = capture(&"a".repeat(MAX_CAPTURE_BYTES - 1));
        output.push_text("🐝");
        assert_eq!(output.captured_text().len(), MAX_CAPTURE_BYTES - 1);
        assert!(output.final_text().is_err());
    }

    #[test]
    fn reset_drops_old_text_and_tool_state() {
        let mut output = capture("old");
        output.observe_tool();
        output.reset();
        output.push_text("[[BUZZ_FINAL:fresh]]new[[/BUZZ_FINAL:fresh]]");
        assert_eq!(output.final_text(), Ok(Some("new".into())));
    }
}
