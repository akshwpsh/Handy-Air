//! The Transcription API: send recorded audio to an OpenAI-compatible
//! `/audio/transcriptions` endpoint instead of running a Local Engine.
//!
//! This is a parallel path rather than a variant of the engine abstraction on
//! purpose — see `docs/adr/0002-transcription-api-as-a-parallel-path.md`.
//! Everything the network path needs lives in this module so that upstream
//! merges keep touching only a handful of lines elsewhere.

use crate::llm_client::{report_reqwest_error, sanitized_url_for_log};
use crate::settings::AppSettings;
use hound::{WavSpec, WavWriter};
use log::{debug, info, warn};
use serde::Deserialize;
use std::fmt;
use std::io::Cursor;
use std::sync::OnceLock;
use std::time::Duration;

/// Handy records 16 kHz mono; the endpoint downsamples to the same rate, so the
/// recording is uploaded as-is with no resampling.
pub const SAMPLE_RATE: u32 = 16_000;
const BITS_PER_SAMPLE: u16 = 16;
const BYTES_PER_SAMPLE: usize = 2;
const WAV_HEADER_BYTES: usize = 44;

/// Groq caps uploads at 25 MB on the free tier (100 MB on dev). At 16 kHz mono
/// 16-bit PCM that is a little under 14 minutes, which no realistic dictation
/// reaches — so the limit is enforced up front with a clear message rather than
/// spending the upload to be told no.
const MAX_UPLOAD_BYTES: usize = 25 * 1024 * 1024;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const RETRY_DELAY: Duration = Duration::from_millis(400);

/// The endpoint caps `prompt` at 224 tokens. Custom-word lists are normally far
/// shorter, but a long one would otherwise fail the whole request.
const MAX_PROMPT_CHARS: usize = 600;

/// Everything one Transcription API call needs, resolved from settings.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiRequest {
    pub base_url: String,
    pub model: String,
    /// ISO-639-1 hint. `None` lets the endpoint auto-detect.
    pub language: Option<String>,
    /// Stylistic hint carrying the user's custom words.
    pub prompt: Option<String>,
    /// Use the translations endpoint, whose output is always English.
    pub translate: bool,
    api_key: String,
}

/// Hand-written so the key cannot reach a log through a `{:?}`.
impl fmt::Debug for ApiRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let api_key = if self.api_key.is_empty() {
            "<empty>"
        } else {
            "[REDACTED]"
        };
        f.debug_struct("ApiRequest")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("language", &self.language)
            .field("prompt", &self.prompt)
            .field("translate", &self.translate)
            .field("api_key", &api_key)
            .finish()
    }
}

impl ApiRequest {
    pub fn endpoint(&self) -> String {
        let action = if self.translate {
            "translations"
        } else {
            "transcriptions"
        };
        format!("{}/audio/{}", self.base_url, action)
    }

    /// The non-file form fields, in a shape that can be asserted on in tests.
    fn form_fields(&self) -> Vec<(&'static str, String)> {
        let mut fields = vec![
            ("model", self.model.clone()),
            ("response_format", "json".to_string()),
            ("temperature", "0".to_string()),
        ];

        // The translations endpoint always emits English and rejects `language`.
        if !self.translate {
            if let Some(language) = &self.language {
                fields.push(("language", language.clone()));
            }
        }

        if let Some(prompt) = &self.prompt {
            fields.push(("prompt", prompt.clone()));
        }

        fields
    }

    fn multipart_form(&self, wav: bytes::Bytes) -> Result<reqwest::multipart::Form, String> {
        // Known length, so this is a normal Content-Length body, not chunked —
        // some endpoints reject chunked uploads.
        let length = wav.len() as u64;
        let part = reqwest::multipart::Part::stream_with_length(wav, length)
            .file_name("audio.wav")
            .mime_str("audio/wav")
            .map_err(|e| format!("Failed to build the audio upload: {}", e))?;

        let mut form = reqwest::multipart::Form::new().part("file", part);
        for (name, value) in self.form_fields() {
            form = form.text(name, value);
        }
        Ok(form)
    }
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ModelListResponse {
    data: Option<Vec<ModelListEntry>>,
}

#[derive(Debug, Deserialize)]
struct ModelListEntry {
    id: Option<String>,
}

/// Whether transcription should go over the network at all.
pub fn is_enabled(settings: &AppSettings) -> bool {
    settings.transcription_api_enabled
}

/// Whether a failed API call may fall back to the Local Engine.
pub fn local_fallback_enabled(settings: &AppSettings) -> bool {
    settings.transcription_api_local_fallback
}

/// The `prompt` hint built from the user's custom words, plus whether any word
/// had to be left out to stay inside the endpoint's token budget.
#[derive(Debug, Default, PartialEq, Eq)]
struct PromptHint {
    text: Option<String>,
    /// Words the budget could not fit. They never reached the endpoint, so the
    /// local fuzzy correction still has to handle them.
    dropped: usize,
}

/// Join the user's custom words into a `prompt` hint, bounded to the endpoint's
/// token budget. Truncation drops whole words, never half of one.
fn custom_words_hint(custom_words: &[String]) -> PromptHint {
    let words: Vec<&str> = custom_words
        .iter()
        .map(|word| word.trim())
        .filter(|word| !word.is_empty())
        .collect();
    if words.is_empty() {
        return PromptHint::default();
    }

    let total = words.len();
    let mut prompt = String::new();
    let mut included = 0;
    for word in words {
        let separator = if prompt.is_empty() { "" } else { ", " };
        if prompt.chars().count() + separator.chars().count() + word.chars().count()
            > MAX_PROMPT_CHARS
        {
            break;
        }
        prompt.push_str(separator);
        prompt.push_str(word);
        included += 1;
    }

    PromptHint {
        text: if prompt.is_empty() {
            None
        } else {
            Some(prompt)
        },
        dropped: total - included,
    }
}

/// Whether every custom word the user configured reached the endpoint as part of
/// the `prompt`.
///
/// Callers use this to decide whether local fuzzy correction can be skipped. It
/// can only be skipped when nothing was dropped — otherwise the words past the
/// budget would get neither the endpoint hint nor the local correction.
pub fn custom_words_fully_prompted(settings: &AppSettings) -> bool {
    custom_words_hint(&settings.custom_words).dropped == 0
}

/// Resolve a request from settings, or explain what is still missing.
pub fn request_from_settings(settings: &AppSettings) -> Result<ApiRequest, String> {
    let base_url = settings
        .transcription_api_base_url
        .trim()
        .trim_end_matches('/')
        .to_string();
    if base_url.is_empty() {
        return Err("Transcription API endpoint is not set".to_string());
    }

    let api_key = settings.resolved_transcription_api_key();
    if api_key.is_empty() {
        return Err("Transcription API key is not set".to_string());
    }

    let model = settings.transcription_api_model.trim().to_string();
    if model.is_empty() {
        return Err("Transcription API model is not set".to_string());
    }

    let language = match settings.selected_language.trim() {
        "" | "auto" => None,
        selected => Some(selected.to_string()),
    };

    Ok(ApiRequest {
        base_url,
        model,
        language,
        prompt: custom_words_hint(&settings.custom_words).text,
        translate: settings.translate_to_english,
        api_key,
    })
}

/// Reject audio that cannot fit the upload limit before spending time encoding
/// it. Returns the encoded size the samples would produce.
fn guard_upload_size(sample_count: usize) -> Result<usize, String> {
    let encoded = WAV_HEADER_BYTES + sample_count * BYTES_PER_SAMPLE;
    if encoded > MAX_UPLOAD_BYTES {
        let limit_seconds = (MAX_UPLOAD_BYTES - WAV_HEADER_BYTES) as f64
            / (BYTES_PER_SAMPLE as f64 * SAMPLE_RATE as f64);
        let actual_seconds = sample_count as f64 / SAMPLE_RATE as f64;
        return Err(format!(
            "Recording is too long for the Transcription API: {:.1} minutes, limit is {:.1} minutes",
            actual_seconds / 60.0,
            limit_seconds / 60.0
        ));
    }
    Ok(encoded)
}

/// Encode 16 kHz mono samples as a WAV file in memory, matching the on-disk
/// format Handy already writes for its recordings.
pub fn encode_wav(samples: &[f32]) -> Result<Vec<u8>, String> {
    guard_upload_size(samples.len())?;

    let spec = WavSpec {
        channels: 1,
        sample_rate: SAMPLE_RATE,
        bits_per_sample: BITS_PER_SAMPLE,
        sample_format: hound::SampleFormat::Int,
    };

    let mut buffer = Cursor::new(Vec::with_capacity(
        WAV_HEADER_BYTES + samples.len() * BYTES_PER_SAMPLE,
    ));
    {
        let mut writer = WavWriter::new(&mut buffer, spec)
            .map_err(|e| format!("Failed to start WAV encoding: {}", e))?;
        for sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            writer
                .write_sample((clamped * i16::MAX as f32) as i16)
                .map_err(|e| format!("Failed to encode audio: {}", e))?;
        }
        writer
            .finalize()
            .map_err(|e| format!("Failed to finish WAV encoding: {}", e))?;
    }

    Ok(buffer.into_inner())
}

/// A one-worker runtime dedicated to this module.
///
/// The transcription entry point is synchronous and is called from inside
/// Tauri's runtime, so this is always driven from a thread of our own (see
/// [`run_blocking`]) — never by blocking on the caller's runtime thread.
fn runtime() -> Result<&'static tokio::runtime::Runtime, String> {
    // Built inside `get_or_init` on purpose. Building it first and handing the
    // value over would make the loser of a concurrent first call drop a live
    // runtime, and `Runtime::drop` blocks until its worker threads have shut
    // down — a stall on whichever thread is trying to transcribe.
    static RUNTIME: OnceLock<std::io::Result<tokio::runtime::Runtime>> = OnceLock::new();

    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
        })
        .as_ref()
        .map_err(|e| format!("Failed to start the Transcription API runtime: {}", e))
}

/// Drive an async call to completion from synchronous code.
///
/// The work is handed to a scoped thread because the caller is usually already
/// inside a tokio runtime, where blocking on another runtime panics.
fn run_blocking<T, F>(future: F) -> Result<T, String>
where
    T: Send,
    F: std::future::Future<Output = Result<T, String>> + Send,
{
    let runtime = runtime()?;
    std::thread::scope(|scope| {
        scope
            .spawn(move || runtime.block_on(future))
            .join()
            .map_err(|_| "The Transcription API worker thread panicked".to_string())?
    })
}

/// Transcribe recorded samples over the network. Blocking.
pub fn transcribe(settings: &AppSettings, audio: &[f32]) -> Result<String, String> {
    let request = request_from_settings(settings)?;
    let wav = encode_wav(audio)?;

    info!(
        "Transcription API: uploading {:.1}s of audio ({} bytes) to model '{}'",
        audio.len() as f64 / SAMPLE_RATE as f64,
        wav.len(),
        request.model
    );

    run_blocking(send(request, wav))
}

/// Retry only what a retry can plausibly fix: rate limits and server faults.
/// A 401 or a 413 answers identically every time.
fn is_retryable_status(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Turn a failure response into a message worth showing a user.
fn describe_status_error(status: reqwest::StatusCode, body: &str) -> String {
    let hint = match status.as_u16() {
        401 | 403 => "The Transcription API key was rejected. Check the key in Settings.",
        404 => "The Transcription API endpoint was not found. Check the base URL in Settings.",
        413 => "The recording was too large for the Transcription API.",
        429 => "The Transcription API rate limit was reached. Try again shortly.",
        _ => "The Transcription API request failed.",
    };

    // Bounded: an endpoint is free to answer with something enormous.
    let detail: String = body.trim().chars().take(500).collect();
    if detail.is_empty() {
        format!("{} (status {})", hint, status.as_u16())
    } else {
        format!("{} (status {}): {}", hint, status.as_u16(), detail)
    }
}

async fn send(request: ApiRequest, wav: Vec<u8>) -> Result<String, String> {
    let url = request.endpoint();
    debug!(
        "Transcription API request to {}",
        sanitized_url_for_log(&url)
    );

    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| report_reqwest_error("Failed to build the Transcription API client", &e))?;

    // Refcounted so the retry re-sends the same buffer instead of cloning it.
    // At the 25 MB cap a plain clone would double peak memory on every attempt.
    let wav = bytes::Bytes::from(wav);

    let mut attempt = 0;
    loop {
        attempt += 1;
        let form = request.multipart_form(wav.clone())?;
        let response = client
            .post(&url)
            .bearer_auth(&request.api_key)
            .multipart(form)
            .send()
            .await;

        match response {
            Ok(response) => {
                let status = response.status();
                if status.is_success() {
                    // A decode failure must not echo its source: the body holds
                    // the transcription. `report_reqwest_error` drops the source
                    // chain for decode errors for exactly this reason.
                    let parsed: TranscriptionResponse = response.json().await.map_err(|e| {
                        report_reqwest_error("Failed to parse the Transcription API response", &e)
                    })?;
                    return Ok(parsed.text.unwrap_or_default());
                }

                let retryable = is_retryable_status(status);
                let body = response.text().await.unwrap_or_default();
                if retryable && attempt == 1 {
                    warn!(
                        "Transcription API answered {} — retrying once",
                        status.as_u16()
                    );
                    tokio::time::sleep(RETRY_DELAY).await;
                    continue;
                }
                return Err(describe_status_error(status, &body));
            }
            Err(error) => {
                // `is_request` also covers request-construction failures such
                // as a malformed URL, which answer identically forever — only
                // timeouts and connect failures are worth a second attempt.
                let transport_fault = error.is_timeout() || error.is_connect();
                let details = report_reqwest_error("Transcription API request failed", &error);
                if transport_fault && attempt == 1 {
                    warn!("Transcription API transport failure — retrying once");
                    tokio::time::sleep(RETRY_DELAY).await;
                    continue;
                }
                return Err(details);
            }
        }
    }
}

/// List the models an endpoint offers, for the settings dropdown. Doubles as
/// the key check: a bad key answers 401 here without costing a transcription.
pub async fn list_models(base_url: &str, api_key: &str) -> Result<Vec<String>, String> {
    let base_url = base_url.trim().trim_end_matches('/');
    if base_url.is_empty() {
        return Err("Transcription API endpoint is not set".to_string());
    }
    if api_key.trim().is_empty() {
        return Err("Transcription API key is not set".to_string());
    }

    let url = format!("{}/models", base_url);
    debug!("Listing models from {}", sanitized_url_for_log(&url));

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| report_reqwest_error("Failed to build the Transcription API client", &e))?;

    let response = client
        .get(&url)
        .bearer_auth(api_key.trim())
        .send()
        .await
        .map_err(|e| report_reqwest_error("Failed to list Transcription API models", &e))?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(describe_status_error(status, &body));
    }

    let parsed: ModelListResponse = response
        .json()
        .await
        .map_err(|e| report_reqwest_error("Failed to parse the model list", &e))?;

    let mut models: Vec<String> = parsed
        .data
        .unwrap_or_default()
        .into_iter()
        .filter_map(|entry| entry.id)
        .collect();
    models.sort();
    models.dedup();
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::get_default_settings;

    fn request(translate: bool) -> ApiRequest {
        ApiRequest {
            base_url: "https://api.groq.com/openai/v1".to_string(),
            model: "whisper-large-v3-turbo".to_string(),
            language: Some("ko".to_string()),
            prompt: Some("Handy, Groq".to_string()),
            translate,
            api_key: "test-key".to_string(),
        }
    }

    fn field<'a>(fields: &'a [(&'static str, String)], name: &str) -> Option<&'a str> {
        fields
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }

    #[test]
    fn encoded_audio_is_a_wav_file_of_the_expected_size() {
        let samples = vec![0.0_f32; 1600];
        let wav = encode_wav(&samples).unwrap();

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(
            wav.len(),
            WAV_HEADER_BYTES + samples.len() * BYTES_PER_SAMPLE
        );
    }

    #[test]
    fn encoded_audio_round_trips_sample_values() {
        let samples = vec![0.0_f32, 0.5, -0.5, 1.0, -1.0];
        let wav = encode_wav(&samples).unwrap();

        let mut reader = hound::WavReader::new(Cursor::new(wav)).unwrap();
        assert_eq!(reader.spec().sample_rate, SAMPLE_RATE);
        assert_eq!(reader.spec().channels, 1);

        let decoded: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
        assert_eq!(decoded.len(), samples.len());
        assert_eq!(decoded[0], 0);
        assert!((decoded[1] - i16::MAX / 2).abs() <= 1);
        assert!((decoded[2] + i16::MAX / 2).abs() <= 1);
        assert_eq!(decoded[3], i16::MAX);
        assert_eq!(decoded[4], -i16::MAX);
    }

    #[test]
    fn samples_beyond_the_upload_limit_are_rejected_before_encoding() {
        let just_fits = (MAX_UPLOAD_BYTES - WAV_HEADER_BYTES) / BYTES_PER_SAMPLE;
        assert!(guard_upload_size(just_fits).is_ok());

        let error = guard_upload_size(just_fits + 1).unwrap_err();
        assert!(error.contains("too long"), "{}", error);
        // The message has to tell the user what the limit actually is:
        // 25 MB of 16 kHz 16-bit mono PCM is 13.7 minutes.
        assert!(error.contains("limit is 13.7 minutes"), "{}", error);
    }

    #[test]
    fn translation_uses_a_different_endpoint() {
        assert_eq!(
            request(false).endpoint(),
            "https://api.groq.com/openai/v1/audio/transcriptions"
        );
        assert_eq!(
            request(true).endpoint(),
            "https://api.groq.com/openai/v1/audio/translations"
        );
    }

    #[test]
    fn language_is_sent_for_transcription_but_not_translation() {
        // The translations endpoint always emits English and rejects `language`.
        let fields = request(false).form_fields();
        assert_eq!(field(&fields, "language"), Some("ko"));

        let fields = request(true).form_fields();
        assert_eq!(field(&fields, "language"), None);
    }

    #[test]
    fn requests_pin_deterministic_output() {
        let fields = request(false).form_fields();
        assert_eq!(field(&fields, "response_format"), Some("json"));
        assert_eq!(field(&fields, "temperature"), Some("0"));
        assert_eq!(field(&fields, "model"), Some("whisper-large-v3-turbo"));
        assert_eq!(field(&fields, "prompt"), Some("Handy, Groq"));
    }

    #[test]
    fn debug_output_never_contains_the_api_key() {
        let rendered = format!("{:?}", request(false));
        assert!(!rendered.contains("test-key"), "{}", rendered);
        assert!(rendered.contains("[REDACTED]"), "{}", rendered);
    }

    #[test]
    fn custom_words_become_a_prompt() {
        assert_eq!(custom_words_hint(&[]).text, None);
        assert_eq!(
            custom_words_hint(&["  ".to_string(), "".to_string()]).text,
            None
        );
        assert_eq!(
            custom_words_hint(&["Handy".to_string(), " Groq ".to_string()]).text,
            Some("Handy, Groq".to_string())
        );
    }

    #[test]
    fn an_overlong_word_list_is_truncated_between_words() {
        let words: Vec<String> = (0..500).map(|i| format!("word{}", i)).collect();
        let hint = custom_words_hint(&words);
        let prompt = hint.text.unwrap();

        assert!(prompt.chars().count() <= MAX_PROMPT_CHARS);
        // Truncation drops whole words, so the tail is never a partial one.
        assert!(prompt.split(", ").all(|word| word.starts_with("word")));
        assert!(prompt.starts_with("word0, word1, "));
        assert!(hint.dropped > 0);
    }

    #[test]
    fn dropped_custom_words_keep_their_claim_on_local_correction() {
        let mut settings = get_default_settings();
        settings.transcription_api_key = "key".to_string().into();

        // Nothing configured, and a short list that fits: the endpoint has seen
        // everything, so local fuzzy correction can be skipped.
        assert!(custom_words_fully_prompted(&settings));
        settings.custom_words = vec!["Handy".to_string(), "Groq".to_string()];
        assert!(custom_words_fully_prompted(&settings));
        assert_eq!(custom_words_hint(&settings.custom_words).dropped, 0);

        // A list too long for the prompt budget: the words past the cut never
        // reached the endpoint, so correction must still run for them.
        settings.custom_words = (0..500).map(|i| format!("word{}", i)).collect();
        assert!(!custom_words_fully_prompted(&settings));
    }

    #[test]
    fn auto_language_is_left_to_the_endpoint() {
        let mut settings = get_default_settings();
        settings.transcription_api_key = "key".to_string().into();
        settings.selected_language = "auto".to_string();
        assert_eq!(request_from_settings(&settings).unwrap().language, None);

        settings.selected_language = "ko".to_string();
        assert_eq!(
            request_from_settings(&settings).unwrap().language,
            Some("ko".to_string())
        );
    }

    #[test]
    fn a_missing_key_is_reported_rather_than_sent_empty() {
        let settings = get_default_settings();
        let error = request_from_settings(&settings).unwrap_err();
        assert!(error.contains("key"), "{}", error);
    }

    #[test]
    fn the_key_falls_back_to_the_post_processing_key_for_the_same_endpoint() {
        let mut settings = get_default_settings();
        // Default endpoint is Groq's, and the Groq post-processing provider
        // carries the same base URL — so its key should be picked up.
        settings
            .post_process_api_keys
            .insert("groq".to_string(), "shared-key".to_string());
        assert_eq!(settings.resolved_transcription_api_key(), "shared-key");

        // An explicit key always wins.
        settings.transcription_api_key = "own-key".to_string().into();
        assert_eq!(settings.resolved_transcription_api_key(), "own-key");

        // Pointing the endpoint elsewhere must not send the Groq key to it.
        settings.transcription_api_key = String::new().into();
        settings.transcription_api_base_url = "https://example.test/v1".to_string();
        assert_eq!(settings.resolved_transcription_api_key(), "");
    }

    #[test]
    fn the_runtime_is_shared_and_usable_from_a_plain_thread() {
        // Built once, inside get_or_init, so a concurrent first call never has
        // to drop a live runtime on the calling thread.
        let address = |runtime: &'static tokio::runtime::Runtime| runtime as *const _ as usize;
        let first = address(runtime().unwrap());
        let second = std::thread::spawn(move || address(runtime().unwrap()))
            .join()
            .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn only_transient_failures_are_retried() {
        assert!(is_retryable_status(reqwest::StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(!is_retryable_status(reqwest::StatusCode::UNAUTHORIZED));
        assert!(!is_retryable_status(reqwest::StatusCode::PAYLOAD_TOO_LARGE));
    }

    /// End to end against a real endpoint.
    ///
    /// Ignored by default: it needs a key and costs a little money, so it runs
    /// only when asked for:
    ///
    /// ```text
    /// cargo test transcription_api -- --ignored --nocapture
    /// ```
    ///
    /// Requires `src-tauri/tests/groq_key.txt` (git-ignored — never commit a
    /// key) and a short speech recording at
    /// `src-tauri/tests/fixtures/speech.wav`, 16 kHz mono. Put the words you
    /// expect in `speech.expect.txt` next to it to assert on the content rather
    /// than merely on getting something back.
    #[test]
    #[ignore = "hits a real endpoint; needs tests/groq_key.txt and a fixture"]
    fn transcribes_a_real_recording_end_to_end() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let key_path = root.join("tests").join("groq_key.txt");
        let fixture_path = root.join("tests").join("fixtures").join("speech.wav");
        let expected_path = root
            .join("tests")
            .join("fixtures")
            .join("speech.expect.txt");

        let key = std::fs::read_to_string(&key_path)
            .unwrap_or_else(|e| panic!("failed to read {}: {}", key_path.display(), e))
            .trim()
            .to_string();
        assert!(!key.is_empty(), "{} is empty", key_path.display());

        let mut reader = hound::WavReader::open(&fixture_path)
            .unwrap_or_else(|e| panic!("failed to open {}: {}", fixture_path.display(), e));
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, SAMPLE_RATE, "the fixture must be 16 kHz");
        assert_eq!(spec.channels, 1, "the fixture must be mono");
        let samples: Vec<f32> = reader
            .samples::<i16>()
            .map(|sample| sample.unwrap() as f32 / i16::MAX as f32)
            .collect();

        let mut settings = get_default_settings();
        settings.transcription_api_enabled = true;
        settings.transcription_api_key = key.into();

        let text = transcribe(&settings, &samples).expect("transcription failed");
        println!("Transcription API returned: {:?}", text);
        assert!(!text.trim().is_empty(), "the endpoint returned no text");

        // The stronger assertion, when the fixture says what it contains.
        if let Ok(expected) = std::fs::read_to_string(&expected_path) {
            let expected = expected.trim().to_lowercase();
            if !expected.is_empty() {
                assert!(
                    text.to_lowercase().contains(&expected),
                    "expected {:?} in {:?}",
                    expected,
                    text
                );
            }
        }
    }

    #[test]
    fn failure_messages_explain_the_status_and_stay_bounded() {
        let message = describe_status_error(reqwest::StatusCode::UNAUTHORIZED, "bad key");
        assert!(message.contains("key was rejected"), "{}", message);
        assert!(message.contains("401"), "{}", message);

        let huge = "x".repeat(5000);
        let message = describe_status_error(reqwest::StatusCode::BAD_REQUEST, &huge);
        assert!(message.len() < 700, "{}", message.len());
    }
}
