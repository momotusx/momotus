//! The read half of 6.3 (plans/plugins-and-marketplace.md): what
//! `getStreams` returned, read field by field inside the JS context. Nothing
//! is deserialized: only the fields below are read, each with a bounded,
//! type-checked read, and a read that throws (a getter, a Proxy trap) drops
//! that stream. Sanitizing and the URL rule are the host's (E2).

use crate::engine::{PluginStream, PluginSubtitle};
use rquickjs::{CatchResultExt, Ctx, Function, Object, Result, Value};

pub const MAX_STREAMS: usize = 100;
/// All the text the streams carry, together (6.2's 256 KiB).
pub const MAX_TOTAL: usize = 256 * 1024;
pub const MAX_URL: usize = 8 * 1024;
pub const MAX_TEXT: usize = 1024;
pub const MAX_PAIRS: usize = 16;

/// A reader with the length check the prelude made, so no long string is
/// copied out of the heap before it is measured.
pub struct Reader<'js> {
    ctx: Ctx<'js>,
    /// `(v) => typeof v === "string" ? v.length : -1`, made before the plugin ran.
    length: Function<'js>,
    spent: usize,
}

impl<'js> Reader<'js> {
    pub fn new(ctx: Ctx<'js>) -> Result<Self> {
        let length = ctx.eval(r#"(v) => (typeof v === "string" ? v.length : -1)"#)?;
        Ok(Self {
            ctx,
            length,
            spent: 0,
        })
    }

    /// Every stream that reads whole, up to the caps; anything but an array
    /// reads as none, as Nuvio has it.
    pub fn streams(&mut self, value: Value<'js>) -> Vec<PluginStream> {
        let Some(list) = value.into_array() else {
            return Vec::new();
        };
        let len = list.len().min(MAX_STREAMS);
        let mut out = Vec::new();
        for i in 0..len {
            let Ok(item) = list.get::<Value>(i).catch(&self.ctx) else {
                continue;
            };
            let before = self.spent;
            match self.stream(item) {
                Some(stream) if self.spent <= MAX_TOTAL => out.push(stream),
                _ => self.spent = before,
            }
            if self.spent >= MAX_TOTAL {
                break;
            }
        }
        out
    }

    fn stream(&mut self, item: Value<'js>) -> Option<PluginStream> {
        let item = item.into_object()?;
        let url = self.field(&item, "url")?;
        // A plain string, or `{url}` as Nuvio also takes.
        let url = match url.clone().into_object() {
            Some(inner) => self.string_field(&inner, "url", MAX_URL)?,
            None => self.string(url, MAX_URL)?,
        }?;
        if url.is_empty() {
            return None;
        }
        Some(PluginStream {
            url,
            name: self.text_field(&item, "name", MAX_TEXT)?,
            title: self.text_field(&item, "title", MAX_TEXT)?,
            quality: self.text_field(&item, "quality", MAX_TEXT)?,
            size: self.text_field(&item, "size", MAX_TEXT)?,
            language: self.text_field(&item, "language", MAX_TEXT)?,
            headers: self.pairs(&item)?,
            subtitles: self.subtitles(&item)?,
        })
    }

    /// `object[name]`; None when the read threw.
    fn field(&self, object: &Object<'js>, name: &str) -> Option<Value<'js>> {
        object.get::<_, Value>(name).catch(&self.ctx).ok()
    }

    /// A text field: Some(None) when absent or not text, None when reading
    /// it threw or it is over `max` (which drops the stream for a URL and
    /// cuts nothing else: an over-long name drops the stream too, so what
    /// crosses is always whole).
    fn text_field(
        &mut self,
        object: &Object<'js>,
        name: &str,
        max: usize,
    ) -> Option<Option<String>> {
        let value = self.field(object, name)?;
        self.text(value, max)
    }

    /// As `text_field`, strings only.
    fn string_field(
        &mut self,
        object: &Object<'js>,
        name: &str,
        max: usize,
    ) -> Option<Option<String>> {
        let value = self.field(object, name)?;
        self.string(value, max)
    }

    /// A string or a number, as text.
    fn text(&mut self, value: Value<'js>, max: usize) -> Option<Option<String>> {
        if let Some(n) = value.as_number() {
            let text = if n.fract() == 0.0 && n.abs() < 1e15 {
                format!("{}", n as i64)
            } else {
                format!("{n}")
            };
            self.spent += text.len();
            return Some(Some(text));
        }
        self.string(value, max)
    }

    /// A string at most `max` UTF-16 units long: Some(None) when it is not a
    /// string or is empty, None when it is longer.
    fn string(&mut self, value: Value<'js>, max: usize) -> Option<Option<String>> {
        let length: i64 = self.length.call((value.clone(),)).ok()?;
        if length < 0 {
            return Some(None);
        }
        if length as usize > max {
            return None;
        }
        let text: String = value.get().ok()?;
        self.spent += text.len();
        Some((!text.is_empty()).then_some(text))
    }

    /// `headers`: string names and values only, the first `MAX_PAIRS`.
    fn pairs(&mut self, item: &Object<'js>) -> Option<Vec<(String, String)>> {
        let Some(headers) = self.field(item, "headers")?.into_object() else {
            return Some(Vec::new());
        };
        let mut out = Vec::new();
        for key in headers.keys::<Value>().take(MAX_PAIRS) {
            let Some(name) = self.string(key.ok()?, MAX_TEXT)? else {
                continue;
            };
            let value = headers
                .get::<_, Value>(name.as_str())
                .catch(&self.ctx)
                .ok()?;
            if let Some(value) = self.string(value, MAX_TEXT)? {
                out.push((name, value));
            }
        }
        Some(out)
    }

    /// `subtitles[]`: `{url, language}`, the first `MAX_PAIRS`.
    fn subtitles(&mut self, item: &Object<'js>) -> Option<Vec<PluginSubtitle>> {
        let Some(list) = self.field(item, "subtitles")?.into_array() else {
            return Some(Vec::new());
        };
        let mut out = Vec::new();
        for i in 0..list.len().min(MAX_PAIRS) {
            let Some(sub) = list.get::<Value>(i).catch(&self.ctx).ok()?.into_object() else {
                continue;
            };
            let Some(url) = self.string_field(&sub, "url", MAX_URL)? else {
                continue;
            };
            let lang = self.text_field(&sub, "language", MAX_TEXT)?;
            out.push(PluginSubtitle { url, lang });
        }
        Some(out)
    }
}
