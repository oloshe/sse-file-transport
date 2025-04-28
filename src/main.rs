use std::{
    collections::HashMap,
    convert::Infallible,
    hash::{DefaultHasher, Hash, Hasher},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{ConnectInfo, Path, State},
    response::{Html, IntoResponse, Sse, sse::Event},
    routing::{get, post},
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, mpsc};
use tokio_stream::{Stream, wrappers::ReceiverStream};
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};

#[derive(Debug, Default)]
struct AppState {
    users: Mutex<HashMap<i64, Option<mpsc::Sender<Result<Event, Infallible>>>>>,
    file_transfers: Mutex<HashMap<i64, TransferInfo>>,
    pending_transfers: Mutex<HashMap<i64, i64>>,
}

fn generate_id(seed: Option<String>) -> i64 {
    if let Some(seed) = seed {
        let mut hasher = DefaultHasher::new();
        seed.hash(&mut hasher);
        let seed_u64 = hasher.finish();
        let mut rng = ChaCha8Rng::seed_from_u64(seed_u64);
        rng.random_range(100000000..999999999)
    } else {
        rand::rng().random_range(100000000..999999999)
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    // 设置 CORS 中间件
    let cors = CorsLayer::new()
        .allow_origin(Any) // 允许任何来源
        .allow_methods(Any) // 允许所有 HTTP 方法
        .allow_headers(Any); // 允许所有头部

    let shared_state = Arc::new(AppState::default());
    let app = Router::new()
        // `GET /` goes to `root`
        .route("/", get(root))
        .route("/connect", get(connect))
        .route("/init-transfer", post(init_transfer))
        .route("/wait-for-transfer/{receiverId}", get(wait_for_transfer))
        .route("/accept-transfer", post(accept_transfer))
        .route("/upload-chunk/{transfer_id}", post(upload_chunk))
        .with_state(shared_state)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .into_make_service_with_connect_info::<SocketAddr>();

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn root() -> impl IntoResponse {
    Html(include_str!("./index.html"))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectResult {
    user_id: i64,
}

async fn connect(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> impl IntoResponse {
    println!("{addr}");
    let user_id = generate_id(Some(addr.to_string()));
    let mut users = state.users.lock().await;
    if users.contains_key(&user_id) {
        // 通知接收方
        if let Some(Some(tx)) = users.get(&user_id) {
            let event_data = InterruptedData {
            };

            let event = Event::default()
                .event("interrupted")
                .data(serde_json::to_string(&event_data).unwrap());

            if let Err(e) = tx.send(Ok(event)).await {
                eprintln!("Failed to notify receiver: {}", e);
            }
        }
    }
    users.insert(user_id, None);
    Json(ConnectResult { user_id })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorResponse {
    error: &'static str,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct InitTransferData {
    pub sender_id: i64,
    pub receiver_id: i64,
    pub file_name: String,
    pub file_size: i64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferInfo {
    pub transfer_id: i64,
    pub sender_id: i64,
    pub receiver_id: i64,
    pub file_name: String,
    pub file_size: i64,
    pub chunks: Vec<String>,
    pub completed: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferResponse {
    pub transfer_id: i64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferWaitingData {
    transfer_id: i64,
    sender_id: i64,
    file_name: String,
    file_size: i64,
}

#[axum::debug_handler]
async fn init_transfer(
    State(state): State<Arc<AppState>>,
    Json(data): Json<InitTransferData>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    let users = state.users.lock().await;
    if !users.contains_key(&data.receiver_id) {
        return Err(Json(ErrorResponse {
            error: "Receiver not found",
        }));
    }
    let transfer_id = generate_id(None);
    let transfer_info = TransferInfo {
        transfer_id,
        sender_id: data.sender_id,
        receiver_id: data.receiver_id,
        file_name: data.file_name.to_owned(),
        file_size: data.file_size,
        chunks: vec![],
        completed: false,
    };

    let mut file_transfers = state.file_transfers.lock().await;
    file_transfers.insert(transfer_id, transfer_info);

    let mut pending_transfers = state.pending_transfers.lock().await;
    pending_transfers.insert(data.receiver_id, transfer_id);

    // 通知接收方
    if let Some(Some(tx)) = users.get(&data.receiver_id) {
        let event_data = TransferWaitData {
            transfer_id,
            sender_id: data.sender_id,
            file_name: data.file_name.to_owned(),
            file_size: data.file_size,
        };

        let event = Event::default()
            .event("transfer-waiting")
            .data(serde_json::to_string(&event_data).unwrap());

        if let Err(e) = tx.send(Ok(event)).await {
            eprintln!("Failed to notify receiver: {}", e);
        }
    }

    Ok(Json(TransferResponse { transfer_id }))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TransferWaitData {
    transfer_id: i64,
    sender_id: i64,
    file_name: String,
    file_size: i64,
}


#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct InterruptedData {
    
}

async fn wait_for_transfer(
    State(state): State<Arc<AppState>>,
    Path(receiver_id): Path<i64>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // 创建通道用于发送事件
    let (tx, rx) = mpsc::channel::<Result<Event, Infallible>>(10);

    // 检查是否有等待中的传输
    {
        let pending_transfers = state.pending_transfers.lock().await;
        let file_transfers = state.file_transfers.lock().await;

        if let Some(transfer_id) = pending_transfers.get(&receiver_id) {
            if let Some(transfer) = file_transfers.get(transfer_id) {
                let event_data = TransferWaitData {
                    transfer_id: transfer.transfer_id,
                    sender_id: transfer.sender_id,
                    file_name: transfer.file_name.to_owned(),
                    file_size: transfer.file_size,
                };

                let event = Event::default()
                    .event("transfer-waiting")
                    .data(serde_json::to_string(&event_data).unwrap());

                tx.send(Ok(event)).await.unwrap();
            }
        }
    }

    // 存储发送器以便后续发送事件
    {
        let mut users = state.users.lock().await;
        users.insert(receiver_id, Some(tx));
    }

    // 返回 SSE 响应
    Sse::new(ReceiverStream::new(rx)).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AcceptTransferRequest {
    transfer_id: i64,
    receiver_id: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SuccessResponse {
    success: bool,
}

async fn accept_transfer(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AcceptTransferRequest>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    // 检查传输是否存在且接收者匹配
    let file_transfers = state.file_transfers.lock().await;
    let transfer = match file_transfers.get(&request.transfer_id) {
        Some(t) if t.receiver_id == request.receiver_id => t,
        _ => {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Transfer not found",
                }),
            ));
        }
    };

    // 通知发送方
    let users = state.users.lock().await;
    if let Some(Some(tx)) = users.get(&transfer.sender_id) {
        let event_data = TransferResponse {
            transfer_id: request.transfer_id,
        };

        let event = Event::default()
            .event("transfer-accepted")
            .data(serde_json::to_string(&event_data).unwrap());

        if let Err(e) = tx.send(Ok(event)).await {
            eprintln!("Failed to notify sender: {}", e);
        }
    }

    Ok(Json(SuccessResponse { success: true }))
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UploadChunkRequest {
    chunk: String, // 使用字节向量表示分片数据
    index: usize,
    is_last: bool,
}

async fn upload_chunk(
    Path(transfer_id): Path<i64>,
    State(state): State<Arc<AppState>>,
    Json(request): Json<UploadChunkRequest>,
) -> Result<impl IntoResponse, impl IntoResponse> {
    // 检查传输是否存在
    let mut file_transfers = state.file_transfers.lock().await;
    let transfer = match file_transfers.get_mut(&transfer_id) {
        Some(t) => t,
        None => {
            return Err((
                axum::http::StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: "Transfer not found",
                }),
            ));
        }
    };

    // 存储分片
    if transfer.chunks.len() <= request.index {
        transfer.chunks.resize(request.index + 1, "".to_string());
    }
    transfer.chunks[request.index] = request.chunk.clone();

    // 转发分片给接收方
    let users = state.users.lock().await;
    if let Some(Some(tx)) = users.get(&transfer.receiver_id) {
        let event = Event::default()
            .event("chunk-received")
            .data(serde_json::to_string(&request).unwrap());

        if let Err(e) = tx.send(Ok(event)).await {
            eprintln!("Failed to notify receiver: {}", e);
        }
    }

    // 如果是最后一个分片，清理传输
    if request.is_last {
        transfer.completed = true;
        let mut file_transfers = state.file_transfers.lock().await;
        file_transfers.remove(&transfer_id);

        // 同时从pending_transfers中移除
        let mut pending_transfers = state.pending_transfers.lock().await;
        pending_transfers.remove(&transfer.receiver_id);
    }

    Ok(Json(SuccessResponse { success: true }))
}
