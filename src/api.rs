use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::http::header;
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::storage::PriceStore;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<PriceStore>,
    pub chain_tip: Arc<AtomicUsize>,
}

#[derive(Serialize)]
struct PriceResponse {
    height: usize,
    price_usd: f64,
    timestamp: u32,
}

#[derive(Serialize)]
struct DatePriceResponse {
    date: String,
    height: usize,
    price_usd: f64,
    timestamp: u32,
}

#[derive(Serialize)]
struct LatestResponse {
    height: usize,
    price_usd: f64,
    timestamp: u32,
}

#[derive(Serialize)]
struct RangeEntry {
    height: usize,
    price_usd: f64,
    timestamp: u32,
}

#[derive(Serialize)]
struct HealthResponse {
    synced_height: usize,
    chain_tip: usize,
    syncing: bool,
    progress_percent: f64,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Deserialize)]
pub struct RangeParams {
    pub from: String,
    pub to: String,
}

#[derive(Deserialize)]
pub struct ChartParams {
    #[serde(default = "default_points")]
    pub points: usize,
}

fn default_points() -> usize {
    600
}

const INDEX_HTML: &str = include_str!("../static/index.html");
const FAVICON_PNG: &[u8] = include_bytes!("../favicon.png");

const FONT_CINZEL_DEC: &[u8] = include_bytes!("../static/fonts/cinzel-decorative-400.woff2");
const FONT_CINZEL_400: &[u8] = include_bytes!("../static/fonts/cinzel-400.woff2");
const FONT_CINZEL_700: &[u8] = include_bytes!("../static/fonts/cinzel-700.woff2");
const FONT_CORMORANT: &[u8] = include_bytes!("../static/fonts/cormorant-400.woff2");
const FONT_CORMORANT_I: &[u8] = include_bytes!("../static/fonts/cormorant-400i.woff2");
const FONT_CORMORANT_600: &[u8] = include_bytes!("../static/fonts/cormorant-600.woff2");
const FONT_JBMONO_400: &[u8] = include_bytes!("../static/fonts/jetbrains-400.woff2");
const FONT_JBMONO_500: &[u8] = include_bytes!("../static/fonts/jetbrains-500.woff2");

const WOFF2: &str = "font/woff2";

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(serve_index))
        .route("/favicon.png", get(serve_favicon))
        .route("/fonts/cinzel-decorative-400.woff2", get(|| async { ([(header::CONTENT_TYPE, WOFF2)], FONT_CINZEL_DEC) }))
        .route("/fonts/cinzel-400.woff2", get(|| async { ([(header::CONTENT_TYPE, WOFF2)], FONT_CINZEL_400) }))
        .route("/fonts/cinzel-700.woff2", get(|| async { ([(header::CONTENT_TYPE, WOFF2)], FONT_CINZEL_700) }))
        .route("/fonts/cormorant-400.woff2", get(|| async { ([(header::CONTENT_TYPE, WOFF2)], FONT_CORMORANT) }))
        .route("/fonts/cormorant-400i.woff2", get(|| async { ([(header::CONTENT_TYPE, WOFF2)], FONT_CORMORANT_I) }))
        .route("/fonts/cormorant-600.woff2", get(|| async { ([(header::CONTENT_TYPE, WOFF2)], FONT_CORMORANT_600) }))
        .route("/fonts/jetbrains-400.woff2", get(|| async { ([(header::CONTENT_TYPE, WOFF2)], FONT_JBMONO_400) }))
        .route("/fonts/jetbrains-500.woff2", get(|| async { ([(header::CONTENT_TYPE, WOFF2)], FONT_JBMONO_500) }))
        .route("/api/price/latest", get(get_latest_price))
        .route("/api/price/date/{date}", get(get_price_at_date))
        .route("/api/price/chart", get(get_chart_data))
        .route("/api/price/range", get(get_price_range))
        .route("/api/price/{height}", get(get_price_at_height))
        .route("/health", get(health_check))
        .with_state(state)
}

async fn serve_index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn serve_favicon() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "image/png")], FAVICON_PNG)
}

async fn get_price_at_height(
    Path(height): Path<usize>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    match (s.store.get_price(height), s.store.get_timestamp(height)) {
        (Some(price), Some(ts)) => Json(PriceResponse {
            height,
            price_usd: round_price(price),
            timestamp: ts,
        })
        .into_response(),
        _ => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: format!("Height {} not yet synced or out of range", height),
            }),
        )
            .into_response(),
    }
}

async fn get_price_at_date(
    Path(date): Path<String>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let target_ts = match parse_date_to_timestamp(&date) {
        Some(ts) => ts,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid date format. Use YYYY-MM-DD".to_string(),
                }),
            )
                .into_response()
        }
    };

    match s.store.height_for_timestamp(target_ts) {
        Some(height) => match (s.store.get_price(height), s.store.get_timestamp(height)) {
            (Some(price), Some(ts)) => Json(DatePriceResponse {
                date,
                height,
                price_usd: round_price(price),
                timestamp: ts,
            })
            .into_response(),
            _ => (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Price data not available for this date".to_string(),
                }),
            )
                .into_response(),
        },
        None => (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "No blocks found for this date".to_string(),
            }),
        )
            .into_response(),
    }
}

async fn get_latest_price(State(s): State<AppState>) -> impl IntoResponse {
    match s.store.last_height() {
        Some(height) => match (s.store.get_price(height), s.store.get_timestamp(height)) {
            (Some(price), Some(ts)) => Json(LatestResponse {
                height,
                price_usd: round_price(price),
                timestamp: ts,
            })
            .into_response(),
            _ => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(ErrorResponse {
                    error: "No data yet".to_string(),
                }),
            )
                .into_response(),
        },
        None => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(ErrorResponse {
                error: "Not yet synced".to_string(),
            }),
        )
            .into_response(),
    }
}

async fn get_price_range(
    Query(params): Query<RangeParams>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let (from_h, to_h) = if params.from.contains('-') {
        // Date mode
        let from_ts = match parse_date_to_timestamp(&params.from) {
            Some(ts) => ts,
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "Invalid 'from' date".to_string(),
                    }),
                )
                    .into_response()
            }
        };
        let to_ts = match parse_date_to_timestamp(&params.to) {
            Some(ts) => ts + 86400, // Include the full day
            None => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse {
                        error: "Invalid 'to' date".to_string(),
                    }),
                )
                    .into_response()
            }
        };
        let from = s.store.height_for_timestamp(from_ts).unwrap_or(0);
        let to = s.store.height_for_timestamp(to_ts).unwrap_or(s.store.len());
        (from, to)
    } else {
        // Height mode
        let from: usize = params.from.parse().unwrap_or(0);
        let to: usize = params.to.parse().unwrap_or(0);
        (from, to + 1) // Include the 'to' height
    };

    // Cap range
    let max_entries = 10_000;
    let to_h = to_h.min(from_h + max_entries);

    let entries: Vec<RangeEntry> = s
        .store
        .get_prices_range(from_h, to_h)
        .into_iter()
        .map(|(h, p, ts)| RangeEntry {
            height: h,
            price_usd: round_price(p),
            timestamp: ts,
        })
        .collect();

    Json(entries).into_response()
}

async fn get_chart_data(
    Query(params): Query<ChartParams>,
    State(s): State<AppState>,
) -> impl IntoResponse {
    let points = params.points.min(2000).max(10);
    let last = match s.store.last_height() {
        Some(h) => h,
        None => {
            return Json(Vec::<RangeEntry>::new()).into_response();
        }
    };

    let start = 550_000usize;
    if last <= start {
        return Json(Vec::<RangeEntry>::new()).into_response();
    }

    let total = last - start;
    let step = total / points;
    let step = step.max(1);

    let mut entries = Vec::with_capacity(points + 1);
    let mut h = start;
    while h <= last {
        if let (Some(price), Some(ts)) = (s.store.get_price(h), s.store.get_timestamp(h)) {
            entries.push(RangeEntry {
                height: h,
                price_usd: round_price(price),
                timestamp: ts,
            });
        }
        h += step;
    }

    // Always include the latest point
    if entries.last().map(|e| e.height) != Some(last) {
        if let (Some(price), Some(ts)) = (s.store.get_price(last), s.store.get_timestamp(last)) {
            entries.push(RangeEntry {
                height: last,
                price_usd: round_price(price),
                timestamp: ts,
            });
        }
    }

    Json(entries).into_response()
}

async fn health_check(State(s): State<AppState>) -> Json<HealthResponse> {
    let synced = s.store.last_height().unwrap_or(0);
    let tip = s.chain_tip.load(Ordering::Relaxed);
    let progress = if tip > 0 {
        (synced as f64 / tip as f64) * 100.0
    } else {
        0.0
    };

    Json(HealthResponse {
        synced_height: synced,
        chain_tip: tip,
        syncing: synced < tip,
        progress_percent: (progress * 100.0).round() / 100.0,
    })
}

fn round_price(price: f64) -> f64 {
    (price * 100.0).round() / 100.0
}

fn parse_date_to_timestamp(date: &str) -> Option<u32> {
    let parts: Vec<&str> = date.split('-').collect();
    if parts.len() != 3 {
        return None;
    }

    let year: i32 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    let day: u32 = parts[2].parse().ok()?;

    if month < 1 || month > 12 || day < 1 || day > 31 {
        return None;
    }

    // Days from year 0 to Unix epoch (1970-01-01)
    let days = days_from_civil(year, month, day) - days_from_civil(1970, 1, 1);
    Some((days * 86400) as u32)
}

/// Convert civil date to days since epoch 0 (algorithm from Howard Hinnant)
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let m = if month <= 2 { month + 9 } else { month - 3 } as i64;
    let d = day as i64;

    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * m + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;

    era * 146097 + doe - 719468
}
