//! Internet radio playback: fetch an Icecast/Shoutcast-style HTTP stream and
//! decode it as it arrives. This is the mode that works fully today, no
//! hardware required.

use crate::source::caching::CachingReader;
use anyhow::{anyhow, Context, Result};
use rodio::{Decoder, Sink};
use std::thread::{self, JoinHandle};

/// Starts fetching `url` and decoding it as MP3 on a background thread,
/// appending decoded audio to `sink` as it arrives. Returns the thread
/// handle so callers can detect if/when the stream dies.
///
/// Most public internet radio stations serve MP3 over plain HTTP, which is
/// what this targets. Streams that require seeking (rare for live radio) or
/// use codecs rodio can't sniff without seeking aren't supported yet.
pub fn spawn_stream(url: String, sink_source: SinkHandle) -> JoinHandle<Result<()>> {
    thread::spawn(move || -> Result<()> {
        let response = reqwest::blocking::Client::builder()
            .timeout(None) 
            .build()?
            .get(&url)
            .send()
            .with_context(|| format!("connecting to {url}"))?;

        if !response.status().is_success() {
            return Err(anyhow!("station returned HTTP {}", response.status()));
        }

        let reader = CachingReader::new(response);
        let decoder =
            Decoder::new_mp3(reader).context("decoding stream as MP3 (only MP3 streams are supported currently)")?;

        let sink = sink_source.0;
        sink.append(decoder);
        // Block this thread for the lifetime of playback so the caller can
        // join on it to know when the stream has ended or errored.
        sink.sleep_until_end();
        Ok(())
    })
}

/// Thin wrapper so we don't need to name rodio's Sink type all over main.rs.
pub struct SinkHandle(pub std::sync::Arc<Sink>);
