//! Default Compute template program.

use fastly::http::{header, Method, StatusCode};
use fastly::kv_store;
use fastly::{mime, Error, Request, Response};

fn get() -> Result<serde_json::Value, Error> {
    let store = kv_store::KVStore::open("journeyman")
        .expect("failed to open KV store")
        .unwrap();
    let mut res = store.lookup("players")?;
    let body = res.take_body();
    let json: serde_json::Value =
        serde_json::from_str(&body.into_string()).expect("json deserialization failed");
    Ok(json)
}

/// The entry point for your application.
///
/// This function is triggered when your service receives a client request. It could be used to
/// route based on the request properties (such as method or path), send the request to a backend,
/// make completely new requests, and/or generate synthetic responses.
///
/// If `main` returns an error, a 500 error response will be delivered to the client.
#[fastly::main]
fn main(req: Request) -> Result<Response, Error> {
    // Log service version
    println!(
        "FASTLY_SERVICE_VERSION: {}",
        std::env::var("FASTLY_SERVICE_VERSION").unwrap_or_else(|_| String::new())
    );

    // Filter request methods...
    match req.get_method() {
        // Block requests with unexpected methods
        &Method::POST | &Method::PUT | &Method::PATCH | &Method::DELETE => {
            return Ok(Response::from_status(StatusCode::METHOD_NOT_ALLOWED)
                .with_header(header::ALLOW, "GET, HEAD, PURGE")
                .with_body_text_plain("This method is not allowed\n"))
        }

        // Let any other requests through
        _ => (),
    };

    // Pattern match on the path...
    match req.get_path() {
        "/get_players" => {
            let db = get()?;
            // Example of returning a JSON response.
            Ok(Response::from_status(StatusCode::OK)
                .with_content_type(mime::APPLICATION_JSON)
                .with_header("Access-Control-Allow-Origin", "*")
                .with_body(serde_json::to_string(&db).expect("failed to serialize DB")))
        }

        // Catch all other requests and return a 404.
        _ => Ok(Response::from_status(StatusCode::NOT_FOUND)
            .with_header("Access-Control-Allow-Origin", "*")
            .with_body_text_plain("The page you requested could not be found\n")),
    }
}
