use actix_web::{web, App, HttpResponse, HttpServer, Result};
use actix_files::Files;
use actix_cors;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::fs;
use actix_web::web::Bytes;
use futures::StreamExt;
use tokio::sync::mpsc;
use std::sync::Mutex;
use lazy_static::lazy_static;
use urlencoding;
use futures::future::BoxFuture;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use serde_json::json;
use chrono::Utc;
use crate::models::VenueCollection;

// Create two static senders - one for status updates and one for completion notification
lazy_static! {
    static ref STATUS_SENDER: Arc<Mutex<Option<mpsc::Sender<String>>>> = Arc::new(Mutex::new(None));
    static ref ACTIVE_CONNECTIONS: Arc<Mutex<Vec<Connection>>> = Arc::new(Mutex::new(Vec::new()));
}

// Import only what we need
use crate::{Config, Location, Processing, search_pool_tables};

#[derive(Deserialize, Debug)]
pub struct SearchParams {
    latitude: f64,
    longitude: f64,
    radius: f64,
    months_threshold: i64,
    save_negative: bool,
    reprocess_all: bool,
}

#[derive(Serialize)]
pub struct VenueResponse {
    name: String,
    address: String,
    probability: f32,
    photos: Vec<String>,
}

#[derive(Clone)]
pub struct AppState {
    pub api_key: String,
    pub cred_path: String,
    pub output_dir: String,
    pub model_path: String,
}

// Add timestamp to connection info
#[derive(Clone)]
struct Connection {
    sender: mpsc::Sender<String>,
    created_at: u64,
}

// Add cleanup function
async fn cleanup_old_connections() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    let mut connections = ACTIVE_CONNECTIONS.lock().unwrap();
    let before_len = connections.len();
    
    // Remove connections older than 5 minutes
    connections.retain(|conn| {
        now - conn.created_at < 300 // 5 minutes in seconds
    });
    
    let removed = before_len - connections.len();
    if removed > 0 {
        println!("Cleaned up {} stale connections", removed);
    }
}

pub async fn status_updates() -> impl actix_web::Responder {
    let (tx, rx) = mpsc::channel(1000);
    
    // Add connection with timestamp
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    
    ACTIVE_CONNECTIONS.lock().unwrap().push(Connection {
        sender: tx,
        created_at: now,
    });
    
    // Run cleanup
    cleanup_old_connections().await;
    
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx)
        .map(|msg| {
            Ok::<_, actix_web::Error>(Bytes::from(format!("data: {}\n\n", msg)))
        });

    HttpResponse::Ok()
        .append_header(("Content-Type", "text/event-stream"))
        .append_header(("Cache-Control", "no-cache"))
        .streaming(stream)
}

pub async fn search_venues(
    params: web::Json<SearchParams>,
    data: web::Data<AppState>,
) -> Result<HttpResponse> {
    println!("Starting search_venues with params: {:?}", params);

    let config = Config {
        location: Location {
            latitude: params.latitude,
            longitude: params.longitude,
            radius_meters: params.radius,
        },
        processing: Processing {
            months_threshold: params.months_threshold,
            reprocess_all: params.reprocess_all,
            save_negative_images: params.save_negative,
        },
        place_types: vec!["bar".to_string(), "restaurant".to_string(), "hotel".to_string()],
    };

    // Get all active connections
    let connections = ACTIVE_CONNECTIONS.lock().unwrap().clone();

    let result = search_pool_tables(
        config,
        &data.api_key,
        &data.cred_path,
        &data.output_dir,
        &data.model_path,
        Some(move |msg: &str| -> BoxFuture<'static, ()> {
            let connections = connections.clone();
            let msg = msg.to_string();
            println!("Sending status update: {}", msg);
            
            Box::pin(async move {
                for conn in &connections {
                    if let Err(e) = conn.sender.send(msg.clone()).await {
                        println!("Error sending status: {}", e);
                    }
                }
            })
        })
    ).await;

    match result {
        Ok(venues) => {
            println!("Search complete, found {} venues", venues.len());
            let venues_response: Vec<VenueResponse> = venues
                .into_iter()
                .filter(|v| v.pool_table_probability > 0.0)
                .map(|v| {
                    let name = v.name.clone();
                    let photos = get_venue_photos(&data.output_dir, &name);
                    println!("Found {} photos for {}", photos.len(), name);
                    VenueResponse {
                        name: v.name,
                        address: v.address,
                        probability: v.pool_table_probability,
                        photos,
                    }
                })
                .collect();

            println!("Returning {} venues with pool tables", venues_response.len());
            Ok(HttpResponse::Ok()
                .content_type("application/json")
                .json(venues_response))
        },
        Err(e) => Ok(HttpResponse::InternalServerError()
            .content_type("application/json")
            .body(format!("{{\"error\": \"{}\"}}", e)))
    }
}

// Helper function to get photos for a venue
fn get_venue_photos(output_dir: &str, venue_name: &str) -> Vec<String> {
    let folder_path = std::path::Path::new(output_dir).join(venue_name);
    println!("Looking for photos in: {}", folder_path.display());
    
    std::fs::read_dir(&folder_path)
        .unwrap_or_else(|_| std::fs::read_dir(std::path::PathBuf::new()).unwrap())
        .filter_map(|entry| {
            entry.ok().and_then(|e| {
                let path = e.path();
                if path.extension()?.to_str()? == "jpg" {
                    let encoded_venue = urlencoding::encode(venue_name);
                    let encoded_filename = urlencoding::encode(path.file_name()?.to_str()?);
                    Some(format!("/photos/{}/{}", 
                        encoded_venue,
                        encoded_filename))
                } else {
                    None
                }
            })
        })
        .collect()
}

// Add this struct for feedback requests
#[derive(Deserialize)]
pub struct FeedbackRequest {
    venue_name: String,
    photo_path: String,
    is_positive: bool,
}

// Add this handler function
pub async fn handle_feedback(
    feedback: web::Json<FeedbackRequest>,
    data: web::Data<AppState>,
) -> Result<HttpResponse> {
    println!("Received feedback for venue: {}, photo: {}, is_positive: {}", 
        feedback.venue_name, feedback.photo_path, feedback.is_positive);

    // Decode the URL-encoded photo path and venue name
    let decoded_path = match urlencoding::decode(&feedback.photo_path) {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Error decoding path: {}", e);
            return Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": "Invalid photo path encoding"
            })));
        }
    };
    
    let decoded_venue = match urlencoding::decode(&feedback.venue_name) {
        Ok(venue) => venue,
        Err(e) => {
            eprintln!("Error decoding venue name: {}", e);
            return Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": "Invalid venue name encoding"
            })));
        }
    };

    // Extract the filename from the photo path
    let photo_path = decoded_path.trim_start_matches("/photos/");
    let source_path = Path::new(&data.output_dir).join(photo_path);
    
    println!("Source path: {}", source_path.display());

    if !feedback.is_positive {
        // Create no_pool_table_training directory if it doesn't exist
        let negative_dir = Path::new(&data.output_dir).join("no_pool_table_training");
        println!("Creating negative directory at: {}", negative_dir.display());
        
        if let Err(e) = fs::create_dir_all(&negative_dir) {
            eprintln!("Error creating negative directory: {}", e);
            return Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": format!("Failed to create negative directory: {}", e)
            })));
        }
        
        // Verify source file exists
        if !source_path.exists() {
            eprintln!("Source file does not exist: {}", source_path.display());
            return Ok(HttpResponse::NotFound().json(json!({
                "success": false,
                "error": format!("Source file not found: {}", source_path.display())
            })));
        }
        
        // Move the photo to the negative directory
        if let Some(filename) = source_path.file_name() {
            let dest_path = negative_dir.join(filename);
            println!("Copying file from {} to {}", source_path.display(), dest_path.display());
            
            if let Err(e) = fs::copy(&source_path, &dest_path) {
                eprintln!("Error copying file to negative directory: {}", e);
                return Ok(HttpResponse::InternalServerError().json(json!({
                    "success": false,
                    "error": format!("Failed to copy file: {}", e)
                })));
            }
        } else {
            eprintln!("Could not extract filename from source path");
            return Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": "Invalid source path"
            })));
        }

        // Update venue probability in database
        let db_path = Path::new("venues_database.json");
        println!("Updating venue database at: {}", db_path.display());
        
        let mut collection = match VenueCollection::load_from_json(db_path) {
            Ok(collection) => collection,
            Err(e) => {
                println!("Could not load database, creating new one: {}", e);
                VenueCollection::new()
            }
        };

        if let Some(venue) = collection.venues.iter_mut()
            .find(|v| v.name == decoded_venue) {
            venue.pool_table_probability = 0.0;
            venue.processed_date = Utc::now();
            
            // Save updated database
            if let Err(e) = collection.save_to_json(db_path) {
                eprintln!("Error saving venue database: {}", e);
                return Ok(HttpResponse::InternalServerError().json(json!({
                    "success": false,
                    "error": format!("Failed to update venue database: {}", e)
                })));
            }
            println!("Successfully updated venue probability in database");
        } else {
            eprintln!("Venue not found in database: {}", decoded_venue);
            return Ok(HttpResponse::NotFound().json(json!({
                "success": false,
                "error": "Venue not found in database"
            })));
        }
    }

    println!("Feedback processed successfully");
    Ok(HttpResponse::Ok().json(json!({ "success": true })))
}

pub async fn start_server(state: AppState) -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .wrap(
                actix_cors::Cors::default()
                    .allow_any_origin()
                    .allow_any_method()
                    .allow_any_header()
                    .max_age(3600)
            )
            .app_data(
                web::JsonConfig::default()
                    .limit(4096)
                    .error_handler(|err, _| {
                        let err_msg = err.to_string();
                        actix_web::error::InternalError::from_response(
                            err,
                            HttpResponse::BadRequest()
                                .content_type("application/json")
                                .body(format!("{{\"error\": \"{}\"}}", err_msg))
                        ).into()
                    })
            )
            .service(
                web::resource("/api/status")
                    .route(web::get().to(status_updates))
            )
            .service(
                web::resource("/api/search")
                    .route(web::post().to(search_venues))
            )
            .service(
                web::resource("/api/feedback")
                    .route(web::post().to(handle_feedback))
            )
            // Serve static files first
            .service(
                Files::new("/photos", &state.output_dir)
                    .use_last_modified(true)
            )
            .service(
                Files::new("/", "./static")
                    .index_file("index.html")
            )
    })
    .bind("127.0.0.1:3000")?
    .keep_alive(std::time::Duration::from_secs(300))
    .client_request_timeout(std::time::Duration::from_secs(300))
    .run()
    .await
} 