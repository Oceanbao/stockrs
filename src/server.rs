// https://docs.rs/axum-extra/latest/axum_extra/index.html
// https://docs.rs/axum/latest/axum/extract/index.html#common-extractors
// https://docs.rs/axum/0.7.5/axum/extract/struct.State.html
// https://docs.rs/axum/latest/axum/middleware/index.html#passing-state-from-middleware-to-handlers
// https://docs.rs/axum/latest/axum/routing/struct.Router.html#method.route_layer
mod web;
use std::time::Duration;

use axum::{
    async_trait,
    body::{Body, Bytes},
    error_handling::HandleErrorLayer,
    extract::{Extension, FromRequest, MatchedPath, Path, Query, Request},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post},
    BoxError, Form, Json, RequestExt, Router,
};
use http_body_util::BodyExt;
use hyper::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use tower::ServiceBuilder;
use tower_http::{classify::ServerErrorsFailureClass, trace::TraceLayer};
use tracing::{info_span, Span};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use web::errors::AppError;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // This returns an error if the `.env` file doesn't exist, but that's not what we want
    // since we're not going to use a `.env` file if we deploy this application.
    dotenv::dotenv().ok();

    // tracing-subscriber is logger crate to define logger impl `Subscriber` from `tracing`.
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(
            // RUST_LOG=realworld_axum_sqlx=debug,tower_http=debug
            tracing_subscriber::EnvFilter::try_from_default_env()
                // axum logs rejections from built-in extractors with `axum::rejection` target
                // at `TRACE` level. `axum::rejection=trace` enables showing those events.
                .unwrap_or_else(|_| "axumm=debug,tower_http=debug,axum::rejection=trace".into()),
        )
        .init();

    // tracing - record event outside any span context
    tracing::event!(tracing::Level::INFO, "something happened");
    // tracing - create span while entering it
    {
        // _span exits when drops
        let _span = tracing::span!(tracing::Level::INFO, "a_span").entered();
        // tracing - records an event within "a_span"
        tracing::event!(tracing::Level::INFO, "something happend inside a_span");
    }
    // regular macros following standard format: info!, error!, debug!, warn!, trace!
    tracing::debug!("Looks just like the log crate");
    tracing::info_span!("a more handy version of creating spans");

    let app = Router::new()
        .route("/", get(get_handler).post(post_handler))
        .route("/user", post(create_user))
        .route("/user/:id", get(get_user))
        .route("/users", get(list_users))
        .route("/user/:id", delete(delete_user))
        .route("/appstate", get(get_app_state))
        .route("/formorjson", get(custom_extractor_handler))
        // `TraceLayer` by tower-http with good defaults, here default and customised:
        .layer(middleware::from_fn(print_request_body))
        // NOTE: Extension (layer) is not type safe, while used by handlers, missing to add
        // .layer() still compiles!
        .layer(Extension(AppState { state: 42 }))
        .layer(
            ServiceBuilder::new()
                .layer(HandleErrorLayer::new(|error: BoxError| async move {
                    if error.is::<tower::timeout::error::Elapsed>() {
                        Ok(StatusCode::REQUEST_TIMEOUT)
                    } else {
                        Err((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            format!("Unhandled internal error: {}", error),
                        ))
                    }
                }))
                .timeout(Duration::from_secs(10))
                .layer(TraceLayer::new_for_http())
                .into_inner(),
        )
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request<_>| {
                    // Log the matched route's path with filled placeholder.
                    // Use request.uri() or OriginalUri for real path.
                    let matched_path = request
                        .extensions()
                        .get::<MatchedPath>()
                        .map(MatchedPath::as_str);

                    info_span!(
                        "http_request",
                        method = ?request.method(),
                        matched_path,
                        some_other_field = tracing::field::Empty,
                    )
                })
                .on_request(|_request: &Request<_>, _span: &Span| {
                    // Can use `_span.record("some_other_field", value)` in one of these
                    // closures to attach a value to the initially empty field in the info_span.
                })
                .on_response(|_response: &Response, _latency: Duration, _span: &Span| {
                    // ...
                })
                .on_body_chunk(|_chunk: &Bytes, _latency: Duration, _span: &Span| {
                    // ...
                })
                .on_eos(
                    |_trailers: Option<&HeaderMap>, _stream_duration: Duration, _span: &Span| {
                        // ...
                    },
                )
                .on_failure(
                    |_error: ServerErrorsFailureClass, _latency: Duration, _span: &Span| {
                        // ...
                    },
                ),
        );

    // Note the type given to each resource.
    // .layer(middleware::from_fn(move |req, next| {
    //     auth(req, next, middleware_database.clone()) // see auth()
    // }))
    // .layer(Extension(Arc::new(tera)))
    // .layer(Extension(database))
    // .layer(Extension(Arc::new(Mutex::new(random))))

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await?;
    tracing::debug!("server up and listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;

    Ok(())
}

#[derive(Clone, Debug, Serialize)]
struct AppState {
    state: u32,
}

async fn get_app_state(Extension(state): Extension<AppState>) -> impl IntoResponse {
    let result: serde_json::Value = serde_json::json!({
      "appState": state,
    });

    (axum::http::StatusCode::OK, Json(result))
}

// middleware that shows how to consume the request body upfront
async fn print_request_body(
    request: Request<Body>,
    next: Next,
) -> Result<impl IntoResponse, Response> {
    tracing::info!("Received request to {}", request.uri());

    let request = buffer_request_body(request).await?;

    Ok(next.run(request).await)
}

// the trick is to take the request apart, buffer the body, do what you need to do, then put
// the request back together
async fn buffer_request_body(request: Request) -> Result<Request, Response> {
    let (parts, body) = request.into_parts();

    // this wont work if the body is an long running stream
    let bytes = body
        .collect()
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response())?
        .to_bytes();

    do_thing_with_request_body(bytes.clone());

    Ok(Request::from_parts(parts, Body::from(bytes)))
}

fn do_thing_with_request_body(bytes: Bytes) {
    tracing::debug!(body = ?bytes);
}

async fn post_handler(
    BufferRequestBody(body): BufferRequestBody,
) -> Result<(StatusCode, String), AppError> {
    tracing::info!(?body, "handler received body");

    if body.len() < 5 {
        return Err(AppError::BadRequest);
    }

    Ok((StatusCode::OK, "post done.".to_string()))
}

async fn get_handler() -> Html<&'static str> {
    Html("<h1>Hello world!</h1>")
}

#[derive(Debug, Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
    email: String,
}

async fn create_user(Json(user): Json<User>) -> impl IntoResponse {
    Response::builder()
        .status(StatusCode::CREATED)
        .body(Body::from(format!("user created ok: {}", user.name)))
        .unwrap()
}

#[derive(Deserialize)]
struct UserId {
    id: u64,
}

async fn get_user(Path(path): Path<u32>, Query(user_id): Query<UserId>) -> String {
    format!("getting {} with id {:?}", path, user_id.id)
}

async fn list_users() -> Json<Vec<User>> {
    let users = vec![
        User {
            id: 1,
            name: "Ocean".to_string(),
            email: "ocean@bao.com".to_string(),
        },
        User {
            id: 2,
            name: "Bao".to_string(),
            email: "bao@ocean.com".to_string(),
        },
    ];
    Json(users)
}

// Define a handler that performs an operation and may return an error
async fn delete_user(Query(id): Query<u64>) -> Result<Json<User>, impl IntoResponse> {
    match perform_delete_user(id).await {
        Ok(_) => Ok(Json(User {
            id,
            name: "Deleted User".into(),
            email: "deleted@user.com".into(),
        })),
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to delete user: {}", e),
        )),
    }
}

// Hypothetical async function to delete a user by ID
async fn perform_delete_user(user_id: u64) -> Result<(), String> {
    // Simulate an error for demonstration
    if user_id == 1 {
        Err("User cannot be deleted.".to_string())
    } else {
        // Logic to delete a user...
        Ok(())
    }
}

// extractor that shows how to consume the request body upfront
struct BufferRequestBody(Bytes);

// we must implement `FromRequest` (and not `FromRequestParts`) to consume the body
#[async_trait]
impl<S> FromRequest<S> for BufferRequestBody
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let body = Bytes::from_request(req, state)
            .await
            .map_err(|err| err.into_response())?;

        do_thing_with_request_body(body.clone());

        Ok(Self(body))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct Payload {
    foo: String,
}

// custom extractor
struct JsonOrForm<T>(T);

async fn custom_extractor_handler(payload: Option<JsonOrForm<Payload>>) {
    if let Some(JsonOrForm(payload)) = payload {
        dbg!(payload);
    } else {
        dbg!("JsonOrForm not supplied");
    }
}

#[async_trait]
impl<S, T> FromRequest<S> for JsonOrForm<T>
where
    S: Send + Sync,
    Json<T>: FromRequest<()>,
    Form<T>: FromRequest<()>,
    T: 'static,
{
    type Rejection = Response;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let content_type_header = req.headers().get(CONTENT_TYPE);
        let content_type = content_type_header.and_then(|value| value.to_str().ok());

        if let Some(content_type) = content_type {
            if content_type.starts_with("application/json") {
                let Json(payload) = req.extract().await.map_err(IntoResponse::into_response)?;
                return Ok(Self(payload));
            }

            if content_type.starts_with("application/x-www-form-urlencoded") {
                let Form(payload) = req.extract().await.map_err(IntoResponse::into_response)?;
                return Ok(Self(payload));
            }
        }

        Err(StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response())
    }
}

/* To compress response via tower layer
// ------------------------------------
use tower_http::compression::CompressionLayer;
use axum::{routing::get, Router};

fn init_router() -> Router {
    Router::new().route("/", get(hello_world)).layer(CompressionLayer::new)
}
*/

/* For middleware requiring state
fn init_router() -> Router {
    let state = setup_state(); // app state initialisation goes here

    Router::new()
        .route("/", get(hello_world))
        .layer(middleware::from_fn_with_state(state.clone(), check_hello_world))
        .with_state(state)
}
*/

/* Static file
use tower_http::services::{ServeDir, ServeFile};


fn init_router() -> Router {
    Router::new().nest_service(
         "/", ServeDir::new("dist")
        .not_found_service(ServeFile::new("dist/index.html")),
    )
}
*/

// Handle Database Init
// -----------------------
// let pool = match PgPoolOptions::new()
//     .max_connections(10)
//     .connect(&database_url)
//     .await
// {
//     Ok(pool) => {
//         println!("✅Connection to the database is successful!");
//         pool
//     }
//     Err(err) => {
//         println!("🔥 Failed to connect to the database: {:?}", err);
//         std::process::exit(1);
//     }
// };

// CORS Layer
// -----------------------
// let cors = CorsLayer::new()
//     .allow_origin("http://localhost:3000".parse::<HeaderValue>().unwrap())
//     .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
//     .allow_credentials(true)
//     .allow_headers([AUTHORIZATION, ACCEPT, CONTENT_TYPE]);

// let app = create_router(Arc::new(AppState { db: pool.clone() })).layer(cors);
// pub fn create_router(app_state: Arc<AppState>) -> Router {

// Use of OnceCell for db
// -----------------------
// static DB_POOL: OnceCell<MySqlPool> = OnceCell::new()
// assert!(DB_POOL.set(pool).is_ok())
// fn get_db() -> Option<&'static MySqlPool> { DB_POOL.get() }
