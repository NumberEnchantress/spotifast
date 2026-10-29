//! Vacuum's pictures as image bytes: generated covers and rings, black
//! squares, and museum artworks, found, paced and cached as the extension's
//! `background.js` does. The app's picture loader calls in here for the
//! `vacuum-` addresses the gate hands out; nothing here draws.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::derive::{Deriver, PALETTE, Picture};
use super::gate::{ART, BLANK_ART, GENERATED, Pictures, generated_picture};
use super::museums::{self, Museum};

/// The side of a picture made here, in pixels, as the extension's.
const ART_SIZE: u32 = 400;
/// Generated pictures are drawn larger: they are sharp at any size.
const GENERATED_SIZE: u32 = 640;

/// What the pictures come from, set by the app when Vacuum's seed or
/// picture source changes.
#[derive(Clone)]
pub struct Config {
    pub seed_id: String,
    pub deriver: Arc<Deriver>,
    pub pictures: Pictures,
    /// Picture addresses of your own, for [`Pictures::Custom`].
    pub custom: Arc<Vec<String>>,
    /// Where each seed's museum picks are kept.
    pub state_dir: PathBuf,
}

fn config_slot() -> &'static RwLock<Option<Config>> {
    static CONFIG: OnceLock<RwLock<Option<Config>>> = OnceLock::new();
    CONFIG.get_or_init(Default::default)
}

pub fn configure(config: Option<Config>) {
    *config_slot()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = config;
}

fn config() -> Option<Config> {
    config_slot()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// A picture the museums gave, kept for good: the same seed and ID always
/// show it, whatever a museum changes later. `url` is `None` when no
/// candidate was usable, so the generated picture stays.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pick {
    pub museum: Option<Museum>,
    pub object: Option<u64>,
    pub url: Option<String>,
    pub frame: [u8; 2],
}

/// What a `vacuum-` address asks for.
pub enum Request {
    /// Bytes ready to show.
    Ready(Vec<u8>),
    /// A picture from this address, framed if round, else the generated
    /// one in `fallback`.
    Image {
        url: String,
        /// Fetched like the Art Institute's own pages ask.
        aic: bool,
        round: bool,
        frame: [u8; 2],
        fallback: Vec<u8>,
    },
}

/// The bytes, or what to fetch, for a `vacuum-` address. Museum lookups
/// happen here, paced; the image itself is fetched by the caller, which
/// keeps its own disk cache.
pub async fn request(client: Option<reqwest::Client>, uri: &str) -> Result<Request, String> {
    if uri.starts_with(BLANK_ART) {
        return Ok(Request::Ready(blank_png()));
    }
    let (picture, round) = generated_picture(uri).ok_or("not a Vacuum picture")?;
    let fallback = generated_png(&picture)?;
    if uri.starts_with(GENERATED) {
        return Ok(Request::Ready(fallback));
    }
    let rest = uri.strip_prefix(ART).ok_or("not a Vacuum picture")?;
    let mut parts = rest.split('/');
    let (Some(seed_id), Some(kind), Some(id)) = (parts.next(), parts.next(), parts.next()) else {
        return Ok(Request::Ready(fallback));
    };
    // Pictures of a seed no longer in use, or with nothing to fetch them
    // with, stay generated.
    let (Some(config), Some(client)) =
        (config().filter(|config| config.seed_id == seed_id), client)
    else {
        return Ok(Request::Ready(fallback));
    };
    let pick = match config.pictures {
        Pictures::Generated => return Ok(Request::Ready(fallback)),
        Pictures::Custom => {
            let Some((index, frame)) =
                museums::custom_pick(&config.deriver, kind, id, config.custom.len())
            else {
                return Ok(Request::Ready(fallback));
            };
            let url = config.custom[index].clone();
            if !url.starts_with("https://") {
                return Ok(Request::Ready(fallback));
            }
            return Ok(Request::Image {
                aic: url.starts_with("https://www.artic.edu/"),
                url,
                round,
                frame,
                fallback,
            });
        }
        Pictures::Museums => museum_pick(&client, &config, kind, id).await?,
    };
    match pick.url {
        Some(url) if museums::is_museum_image(&url) => Ok(Request::Image {
            aic: pick.museum == Some(Museum::Aic),
            url,
            round,
            frame: pick.frame,
            fallback,
        }),
        _ => Ok(Request::Ready(fallback)),
    }
}

/// The picture addresses in a list of your own: a CSV, TSV or text file,
/// every https address in it, in order, each once.
pub fn parse_picture_list(text: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    text.split([',', '\t', '\n', '\r', ';'])
        .map(|field| field.trim().trim_matches('"').trim())
        .filter(|field| field.starts_with("https://") && !field.contains(char::is_whitespace))
        .filter(|field| seen.insert(field.to_string()))
        .map(str::to_owned)
        .collect()
}

/// A generated picture as PNG bytes.
pub fn generated_png(picture: &Picture) -> Result<Vec<u8>, String> {
    let svg = super::pictures::svg(picture, GENERATED_SIZE);
    let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default())
        .map_err(|error| error.to_string())?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(GENERATED_SIZE, GENERATED_SIZE)
        .ok_or("could not make a picture")?;
    let scale = GENERATED_SIZE as f32 / tree.size().width();
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|error| error.to_string())
}

/// A black square.
fn blank_png() -> Vec<u8> {
    let image = image::RgbaImage::from_pixel(64, 64, image::Rgba([0, 0, 0, 255]));
    encode(&image).unwrap_or_default()
}

fn encode(image: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|error| error.to_string())?;
    Ok(bytes.into_inner())
}

fn palette_rgb(index: u8) -> [u8; 3] {
    let hex = &PALETTE[usize::from(index) % PALETTE.len()][1..];
    let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).unwrap_or(0);
    [channel(0), channel(2), channel(4)]
}

/// A cover: the artwork cropped square, full bleed. An artist picture: a
/// round crop inside a two-colour ring frame, so the two still look apart.
pub fn render(bytes: &[u8], round: bool, frame: [u8; 2]) -> Result<Vec<u8>, String> {
    let decoded = image::load_from_memory(bytes).map_err(|error| error.to_string())?;
    let side = decoded.width().min(decoded.height());
    if side == 0 {
        return Err("an empty picture".into());
    }
    let square = decoded.crop_imm(
        (decoded.width() - side) / 2,
        (decoded.height() - side) / 2,
        side,
        side,
    );
    if !round {
        let cover = square
            .resize_exact(ART_SIZE, ART_SIZE, image::imageops::FilterType::Triangle)
            .to_rgba8();
        return encode(&cover);
    }
    let inner = (ART_SIZE as f32 * 0.8) as u32;
    let photo = square
        .resize_exact(inner, inner, image::imageops::FilterType::Triangle)
        .to_rgba8();
    let centre = ART_SIZE as f32 / 2.0;
    let [outer_rgb, ring_rgb] = [palette_rgb(frame[0]), palette_rgb(frame[1])];
    let offset = (ART_SIZE - inner) / 2;
    let picture = image::RgbaImage::from_fn(ART_SIZE, ART_SIZE, |x, y| {
        let distance =
            ((x as f32 + 0.5 - centre).powi(2) + (y as f32 + 0.5 - centre).powi(2)).sqrt();
        let rgb = if distance <= centre * 0.8 {
            let pixel = photo.get_pixel(
                (x.saturating_sub(offset)).min(inner - 1),
                (y.saturating_sub(offset)).min(inner - 1),
            );
            return *pixel;
        } else if distance <= centre * 0.88 {
            ring_rgb
        } else if distance <= centre {
            outer_rgb
        } else {
            [0x12, 0x12, 0x12]
        };
        image::Rgba([rgb[0], rgb[1], rgb[2], 255])
    });
    encode(&picture)
}

// --- museum lookups ---------------------------------------------------------

/// Each museum's pace, as the extension's: the Met's bot protection refuses
/// bursts far below its documented limit, so it gets one request at a time,
/// 500 ms apart.
struct Lane {
    permits: tokio::sync::Semaphore,
    gap: Duration,
    next: Mutex<Instant>,
}

impl Lane {
    fn new(gap_ms: u64, at_once: usize) -> Self {
        Self {
            permits: tokio::sync::Semaphore::new(at_once),
            gap: Duration::from_millis(gap_ms),
            next: Mutex::new(Instant::now()),
        }
    }
}

fn lane(museum: Museum) -> &'static Lane {
    static LANES: OnceLock<[Lane; 3]> = OnceLock::new();
    let lanes = LANES.get_or_init(|| [Lane::new(500, 1), Lane::new(1100, 1), Lane::new(150, 2)]);
    &lanes[match museum {
        Museum::Met => 0,
        Museum::Aic => 1,
        Museum::Cma => 2,
    }]
}

/// Images from the museums' servers, four at once, 50 ms apart: a turn to
/// fetch one, held while it is fetched.
pub async fn image_turn() -> Option<tokio::sync::SemaphorePermit<'static>> {
    static IMAGES: OnceLock<Lane> = OnceLock::new();
    let lane = IMAGES.get_or_init(|| Lane::new(50, 4));
    let permit = lane.permits.acquire().await.ok()?;
    let wait = {
        let mut next = lane
            .next
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let now = Instant::now();
        let start = (*next).max(now);
        *next = start + lane.gap;
        start - now
    };
    tokio::time::sleep(wait).await;
    Some(permit)
}

/// A lookup: the record (usable or not), `None` when the object does not
/// exist, or an error when the museum could not be reached, so nothing is
/// kept and it is tried again later. Refusals pause that museum (3, 6, 9...
/// seconds) and retry, up to six times.
async fn lookup_json(
    client: &reqwest::Client,
    museum: Museum,
    url: &str,
) -> Result<Option<serde_json::Value>, String> {
    let lane = lane(museum);
    for attempt in 0..6u32 {
        let response = {
            let _permit = lane
                .permits
                .acquire()
                .await
                .map_err(|error| error.to_string())?;
            let wait = {
                let mut next = lane
                    .next
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let now = Instant::now();
                let start = (*next).max(now);
                *next = start + lane.gap;
                start - now
            };
            tokio::time::sleep(wait).await;
            let mut request = client.get(url);
            if museum == Museum::Aic {
                request = request.header("AIC-User-Agent", museums::AIC_USER_AGENT);
            }
            request.send().await.map_err(|error| error.to_string())?
        };
        let status = response.status();
        if status == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if status.is_success() {
            return response
                .json()
                .await
                .map(Some)
                .map_err(|error| error.to_string());
        }
        if status != reqwest::StatusCode::FORBIDDEN
            && status != reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            return Err(format!("{url} answered {status}"));
        }
        let mut next = lane
            .next
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *next = (*next).max(Instant::now() + Duration::from_secs(3 * u64::from(attempt + 1)));
    }
    Err(format!("{url} kept refusing"))
}

/// Chicago is asked about up to 100 objects at once; its answers are
/// remembered for the session.
#[derive(Default)]
struct Chicago {
    known: HashMap<u64, Option<String>>,
    waiting: HashMap<u64, Vec<tokio::sync::oneshot::Sender<Option<String>>>>,
    flushing: bool,
}

fn chicago() -> &'static Mutex<Chicago> {
    static CHICAGO: OnceLock<Mutex<Chicago>> = OnceLock::new();
    CHICAGO.get_or_init(Default::default)
}

/// The image of Chicago object `object`, asked with every other one waiting.
fn chicago_image(
    client: &reqwest::Client,
    object: u64,
) -> tokio::sync::oneshot::Receiver<Option<String>> {
    let (send, receive) = tokio::sync::oneshot::channel();
    let mut state = chicago()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(known) = state.known.get(&object) {
        let _ = send.send(known.clone());
        return receive;
    }
    state.waiting.entry(object).or_default().push(send);
    if !state.flushing {
        state.flushing = true;
        let client = client.clone();
        tokio::spawn(async move { flush_chicago(client).await });
    }
    receive
}

async fn flush_chicago(client: reqwest::Client) {
    let mut pause = Duration::from_millis(1100);
    loop {
        let ids: Vec<u64> = {
            let mut state = chicago()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.waiting.is_empty() {
                state.flushing = false;
                return;
            }
            state.waiting.keys().take(100).copied().collect()
        };
        match lookup_json(&client, Museum::Aic, &museums::aic_batch_url(&ids)).await {
            Ok(json) => {
                let found: HashMap<u64, Option<String>> = json
                    .as_ref()
                    .and_then(|json| json.get("data"))
                    .and_then(|data| data.as_array())
                    .map(|records| {
                        records
                            .iter()
                            .filter_map(|record| {
                                Some((record.get("id")?.as_u64()?, museums::aic_image(record)))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let mut state = chicago()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                for id in ids {
                    let url = found.get(&id).cloned().flatten();
                    state.known.insert(id, url.clone());
                    for waiter in state.waiting.remove(&id).unwrap_or_default() {
                        let _ = waiter.send(url.clone());
                    }
                }
                pause = Duration::from_millis(1100);
            }
            Err(error) => {
                log::warn!("Vacuum: the Art Institute of Chicago: {error}");
                // Refused or unreachable: wait longer, then try again.
                pause = (pause * 2).min(Duration::from_secs(60));
                tokio::time::sleep(pause).await;
            }
        }
    }
}

/// The picks of each seed, read once and written as they grow.
fn picks() -> &'static Mutex<HashMap<String, HashMap<String, Pick>>> {
    static PICKS: OnceLock<Mutex<HashMap<String, HashMap<String, Pick>>>> = OnceLock::new();
    PICKS.get_or_init(Default::default)
}

fn picks_file(config: &Config) -> PathBuf {
    config
        .state_dir
        .join("vacuum")
        .join(format!("pictures-{}.json", config.seed_id))
}

async fn museum_pick(
    client: &reqwest::Client,
    config: &Config,
    kind: &str,
    id: &str,
) -> Result<Pick, String> {
    let key = format!("{kind}:{id}");
    let loaded = picks()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .contains_key(&config.seed_id);
    if !loaded {
        let path = picks_file(config);
        let read = tokio::task::spawn_blocking(move || {
            std::fs::read_to_string(path)
                .ok()
                .and_then(|text| serde_json::from_str::<HashMap<String, Pick>>(&text).ok())
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        picks()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(config.seed_id.clone())
            .or_insert(read);
    }
    if let Some(pick) = picks()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&config.seed_id)
        .and_then(|picks| picks.get(&key))
    {
        return Ok(pick.clone());
    }
    let pick = resolve(client, &config.deriver, kind, id).await?;
    picks()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entry(config.seed_id.clone())
        .or_default()
        .insert(key, pick.clone());
    save_picks_soon(config);
    Ok(pick)
}

/// Writes a seed's picks two seconds after they change, once for however
/// many arrived meanwhile.
fn save_picks_soon(config: &Config) {
    static PENDING: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    let pending = PENDING.get_or_init(Default::default);
    if !pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(config.seed_id.clone())
    {
        return;
    }
    let seed_id = config.seed_id.clone();
    let path = picks_file(config);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&seed_id);
        let text = picks()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&seed_id)
            .and_then(|picks| serde_json::to_string(picks).ok());
        if let Some(text) = text {
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(error) = fastframe_file::write(&path, text) {
                    log::warn!("Vacuum could not keep its picture picks: {error}");
                }
            })
            .await;
        }
    });
}

/// Walks the chain: the first candidate that is free to use and has an
/// image on a museum's own image server.
async fn resolve(
    client: &reqwest::Client,
    deriver: &Deriver,
    kind: &str,
    id: &str,
) -> Result<Pick, String> {
    let chain: Vec<museums::Candidate> = (0..museums::MAX_CANDIDATES)
        .map(|n| museums::candidate(deriver, kind, id, n))
        .collect();
    // Chicago is asked about this chain's candidates all at once.
    let mut chicago_answers: HashMap<usize, tokio::sync::oneshot::Receiver<Option<String>>> = chain
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.museum == Museum::Aic)
        .map(|(index, candidate)| (index, chicago_image(client, candidate.object)))
        .collect();
    let mut single: HashMap<usize, tokio::task::JoinHandle<Result<Option<String>, String>>> =
        HashMap::new();
    let lookup = |index: usize| {
        let candidate = chain[index];
        let client = client.clone();
        tokio::spawn(async move {
            let Some(url) = museums::lookup_url(candidate.museum, candidate.object) else {
                return Ok(None);
            };
            let json = lookup_json(&client, candidate.museum, &url).await?;
            Ok(json.and_then(|json| match candidate.museum {
                Museum::Met => museums::met_image(&json),
                _ => museums::cma_image(&json),
            }))
        })
    };
    for (index, candidate) in chain.iter().enumerate() {
        let url = if candidate.museum == Museum::Aic {
            match chicago_answers.remove(&index) {
                Some(answer) => answer.await.map_err(|error| error.to_string())?,
                None => None,
            }
        } else {
            // Ask about the next candidate too; still take them in order.
            for (ahead, next) in chain.iter().enumerate().skip(index).take(2) {
                if next.museum != Museum::Aic && !single.contains_key(&ahead) {
                    single.insert(ahead, lookup(ahead));
                }
            }
            match single.remove(&index) {
                Some(handle) => handle.await.map_err(|error| error.to_string())??,
                None => None,
            }
        };
        if let Some(url) = url.filter(|url| museums::is_museum_image(url)) {
            for handle in single.into_values() {
                handle.abort();
            }
            return Ok(Pick {
                museum: Some(candidate.museum),
                object: Some(candidate.object),
                url: Some(url),
                frame: candidate.frame,
            });
        }
    }
    // None usable: the generated picture stays, for good.
    Ok(Pick {
        museum: None,
        object: None,
        url: None,
        frame: chain[0].frame,
    })
}
