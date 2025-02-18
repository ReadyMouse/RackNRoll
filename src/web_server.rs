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
// use crate::models::Venue;

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
    human_approved: i64,
    photos: Vec<String>,
    place_id: String,
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
    // println!("Search parameters:");
    // println!("  Latitude: {}", params.latitude);
    // println!("  Longitude: {}", params.longitude);
    // println!("  Radius: {} meters", params.radius);
    // println!("  Months threshold: {}", params.months_threshold);
    // println!("  Save negative: {}", params.save_negative);
    // println!("  Reprocess all: {}", params.reprocess_all);

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
        config.clone(), // Clone config since we'll need it later
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
            println!("Search complete, found {} total venues", venues.len());
            let venues_response: Vec<VenueResponse> = venues
                .into_iter()
                .filter(|v| {
                    let has_pool = v.pool_table_probability > 0.0;
                    let in_radius = calculate_distance(
                        params.latitude,
                        params.longitude,
                        v.latitude,
                        v.longitude
                    ) <= params.radius;
                    
                    println!("Venue '{}' coordinates: ({}, {})", 
                        v.name, v.latitude, v.longitude);
                    
                    has_pool && in_radius
                })
                .map(|v| {
                    let name = v.name.clone();
                    let photos = get_venue_photos(&data.output_dir, &name);
                    println!("Found {} photos for {}", photos.len(), name);
                    VenueResponse {
                        name: v.name,
                        address: v.address,
                        probability: v.pool_table_probability,
                        human_approved: v.human_approved as i64,
                        photos,
                        place_id: v.place_id,
                    }
                })
                .collect();

            println!("Returning {} venues with pool tables in radius", venues_response.len());
            Ok(HttpResponse::Ok()
                .content_type("application/json")
                .json(venues_response))
        },
        Err(e) => Ok(HttpResponse::InternalServerError()
            .content_type("application/json")
            .body(format!("{{\"error\": \"{}\"}}", e)))
    }
}

// Add this helper function to calculate distance between two points
fn calculate_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS: f64 = 6371000.0; // Earth's radius in meters
    
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let delta_lat = (lat2 - lat1).to_radians();
    let delta_lon = (lon2 - lon1).to_radians();

    let a = (delta_lat / 2.0).sin() * (delta_lat / 2.0).sin() +
            lat1_rad.cos() * lat2_rad.cos() *
            (delta_lon / 2.0).sin() * (delta_lon / 2.0).sin();
    
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    
    let distance = EARTH_RADIUS * c; // Returns distance in meters
    
    println!("Distance calculation: ({}, {}) to ({}, {}) = {} meters", 
        lat1, lon1, lat2, lon2, distance);
    
    distance
}

// Helper function to get photos for a venue
fn get_venue_photos(output_dir: &str, venue_name: &str) -> Vec<String> {
    let sanitized_name = sanitize_filename(venue_name);
    let folder_path = std::path::Path::new(output_dir).join(&sanitized_name);
    println!("Looking for photos in: {}", folder_path.display());
    
    // Return empty vec if directory doesn't exist instead of panicking
    match std::fs::read_dir(&folder_path) {
        Ok(entries) => entries
            .filter_map(|entry| {
                entry.ok().and_then(|e| {
                    let path = e.path();
                    if path.extension()?.to_str()? == "jpg" {
                        let encoded_filename = urlencoding::encode(path.file_name()?.to_str()?);
                        Some(format!("/photos/{}/{}", 
                            sanitized_name,
                            encoded_filename))
                    } else {
                        None
                    }
                })
            })
            .collect(),
        Err(e) => {
            println!("Could not read directory for '{}': {}", venue_name, e);
            Vec::new()
        }
    }
}

// Add this struct for feedback requests
#[derive(Deserialize)]
pub struct FeedbackRequest {
    venue_name: String,
    photo_path: String,
    is_positive: bool,
    place_id: String,
}

// Add this handler function
pub async fn handle_feedback(
    feedback: web::Json<FeedbackRequest>,
    data: web::Data<AppState>,
) -> Result<HttpResponse> {
    println!("Starting feedback handler");
    println!("Received feedback for venue: {} (place_id: {})", feedback.venue_name, feedback.place_id);
    println!("Photo path: {}", feedback.photo_path);
    println!("Is positive: {}", feedback.is_positive);

    let db_path = Path::new("venues_database.json");
    println!("Loading venue database from: {}", db_path.display());
    
    let mut collection = match VenueCollection::load_from_json(db_path) {
        Ok(collection) => {
            println!("Successfully loaded database with {} venues", collection.venues.len());
            collection
        },
        Err(e) => {
            eprintln!("Error loading database from {}: {}", db_path.display(), e);
            eprintln!("Database error details: {:?}", e);
            return Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": format!("Failed to load venue database: {}", e)
            })));
        }
    };

    // Update the matching logic to use place_id instead of name
    let venue_index = collection.venues.iter().position(|v| {
        let matches = v.place_id == feedback.place_id;
        if matches {
            println!("Found matching venue: {} (place_id: {}, current approvals: {})", 
                v.name, v.place_id, v.human_approved);
        }
        matches
    });

    if let Some(index) = venue_index {
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
            
            // Get the source photo path
            let photo_path = feedback.photo_path.trim_start_matches("/photos/");
            let source_path = Path::new(&data.output_dir)
                .join(photo_path);

            if !source_path.exists() {
                eprintln!("Source file does not exist: {}", source_path.display());
                return Ok(HttpResponse::NotFound().json(json!({
                    "success": false,
                    "error": format!("Source file not found: {}", source_path.display())
                })));
            }
            
            // Move the photo to negative directory
            if let Some(filename) = source_path.file_name() {
                let dest_path = negative_dir.join(filename);
                println!("Moving file from {} to {}", source_path.display(), dest_path.display());
                
                if let Err(e) = fs::copy(&source_path, &dest_path) {
                    eprintln!("Error copying file to negative directory: {}", e);
                    return Ok(HttpResponse::InternalServerError().json(json!({
                        "success": false,
                        "error": format!("Failed to copy file: {}", e)
                    })));
                }
                
                // Remove the original file
                if let Err(e) = fs::remove_file(&source_path) {
                    eprintln!("Error removing original file: {}", e);
                    // Continue execution - not critical if original remains
                }
            }

            // Check if this was the last photo
            let venue_dir = Path::new(&data.output_dir)
                .join(sanitize_filename(&feedback.venue_name));
            
            let remaining_photos = match fs::read_dir(&venue_dir) {
                Ok(entries) => entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.path().extension()
                            .and_then(|ext| ext.to_str())
                            .map(|ext| ext == "jpg")
                            .unwrap_or(false)
                    })
                    .count(),
                Err(_) => 0
            };

            println!("Remaining photos for venue: {}", remaining_photos);

            // If no photos remain, update venue probability to 0
            if remaining_photos == 0 {
                collection.venues[index].pool_table_probability = 0.0;
                collection.venues[index].processed_date = Utc::now();
                
                if let Err(e) = collection.save_to_json(db_path) {
                    eprintln!("Error saving venue database: {}", e);
                    return Ok(HttpResponse::InternalServerError().json(json!({
                        "success": false,
                        "error": format!("Failed to update venue database: {}", e)
                    })));
                }
                println!("Updated venue probability to 0 as all photos were removed");
            }

            return Ok(HttpResponse::Ok().json(json!({
                "success": true,
                "message": if remaining_photos == 0 {
                    "Thanks for your feedback. All photos have been removed and venue has been marked as not having pool tables."
                } else {
                    "Thanks for your feedback to help our training."
                }
            })));
        } else {
            // Create confirmed_pool_tables directory if it doesn't exist
            let confirmed_dir = Path::new(&data.output_dir).join("confirmed_pool_tables");
            println!("Creating confirmed directory at: {}", confirmed_dir.display());
            
            if let Err(e) = fs::create_dir_all(&confirmed_dir) {
                eprintln!("Error creating confirmed directory: {}", e);
                return Ok(HttpResponse::InternalServerError().json(json!({
                    "success": false,
                    "error": format!("Failed to create confirmed directory: {}", e)
                })));
            }

            // Copy the photo to confirmed directory
            let photo_path = feedback.photo_path.trim_start_matches("/photos/");
            let source_path = Path::new(&data.output_dir)
                .join(photo_path);
            if !source_path.exists() {
                eprintln!("Source file does not exist: {}", source_path.display());
                return Ok(HttpResponse::NotFound().json(json!({
                    "success": false,
                    "error": format!("Source file not found: {}", source_path.display())
                })));
            }
            
            if let Some(filename) = source_path.file_name() {
                let dest_path = confirmed_dir.join(filename);
                println!("Copying file from {} to {}", source_path.display(), dest_path.display());
                
                if let Err(e) = fs::copy(&source_path, &dest_path) {
                    eprintln!("Error copying file to confirmed directory: {}", e);
                    return Ok(HttpResponse::InternalServerError().json(json!({
                        "success": false,
                        "error": format!("Failed to copy file: {}", e)
                    })));
                }
            }

            // Update venue in database
            collection.venues[index].human_approved += 1;
            let approval_count = collection.venues[index].human_approved;
            println!("Updated approval count for {} to {}", feedback.venue_name, approval_count);
            
            // Save updated database
            if let Err(e) = collection.save_to_json(db_path) {
                eprintln!("Error saving venue database: {}", e);
                return Ok(HttpResponse::InternalServerError().json(json!({
                    "success": false,
                    "error": format!("Failed to update venue database: {}", e)
                })));
            }
            
            println!("Successfully saved database with updated approval count");
            return Ok(HttpResponse::Ok().json(json!({
                "success": true,
                "message": format!("Thank you! This venue has been approved {} times.", approval_count)
            })));
        }
    } else {
        eprintln!("Venue not found in database: '{}'", feedback.venue_name);
        println!("Available venues in database:");
        for venue in &collection.venues {
            println!("  - '{}'", venue.name);
        }
        return Ok(HttpResponse::NotFound().json(json!({
            "success": false,
            "error": "Venue not found in database"
        })));
    }
}

// Add this function to sanitize filenames
fn sanitize_filename(name: &str) -> String {
    // Replace forward slashes and other problematic characters with underscores
    name.replace('/', "_")
        .replace('\\', "_")
        .replace(':', "_")
        .replace('*', "_")
        .replace('?', "_")
        .replace('"', "_")
        .replace('<', "_")
        .replace('>', "_")
        .replace('|', "_")
}

// Add this new struct for venue-level feedback
#[derive(Deserialize)]
pub struct VenueFeedbackRequest {
    venue_name: String,
    place_id: String,
    is_positive: bool,
}

// Add this new handler function
pub async fn handle_venue_feedback(
    feedback: web::Json<VenueFeedbackRequest>,
) -> Result<HttpResponse> {
    println!("Starting venue feedback handler");
    println!("Received feedback for venue: {} (place_id: {})", feedback.venue_name, feedback.place_id);
    println!("Is positive: {}", feedback.is_positive);

    let db_path = Path::new("venues_database.json");
    println!("Loading venue database from: {}", db_path.display());
    
    let mut collection = match VenueCollection::load_from_json(db_path) {
        Ok(collection) => {
            println!("Successfully loaded database with {} venues", collection.venues.len());
            collection
        },
        Err(e) => {
            eprintln!("Error loading database: {}", e);
            return Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": format!("Failed to load venue database: {}", e)
            })));
        }
    };

    // Find venue by place_id
    let venue_index = collection.venues.iter().position(|v| v.place_id == feedback.place_id);

    if let Some(index) = venue_index {
        if feedback.is_positive {
            // Increment the approval count
            collection.venues[index].human_approved += 1;
            let approval_count = collection.venues[index].human_approved;
            println!("Updated approval count for {} to {}", feedback.venue_name, approval_count);
        } else {
            // Set probability to 0 for negative feedback
            collection.venues[index].pool_table_probability = 0.0;
            collection.venues[index].processed_date = Utc::now();
            println!("Set pool table probability to 0 for {}", feedback.venue_name);
        }
        
        // Save updated database
        if let Err(e) = collection.save_to_json(db_path) {
            eprintln!("Error saving venue database: {}", e);
            return Ok(HttpResponse::InternalServerError().json(json!({
                "success": false,
                "error": format!("Failed to update venue database: {}", e)
            })));
        }
        
        println!("Successfully saved database with updates");
        return Ok(HttpResponse::Ok().json(json!({
            "success": true,
            "message": "Thank you for your feedback!"
        })));
    } else {
        eprintln!("Venue not found in database: '{}'", feedback.venue_name);
        return Ok(HttpResponse::NotFound().json(json!({
            "success": false,
            "error": "Venue not found in database"
        })));
    }
}

pub async fn start_server(state: AppState) -> std::io::Result<()> {
    HttpServer::new(move || {
        App::new()
            .app_data(web::Data::new(state.clone()))
            .app_data(web::JsonConfig::default()
                .limit(4194304)  // Increase JSON payload limit to 4MB
                .error_handler(|err, _| {
                    eprintln!("JSON error: {:?}", err);
                    let err_msg = err.to_string();  // Convert to string before moving
                    actix_web::error::InternalError::from_response(
                        err_msg.clone(),  // Use cloned string
                        HttpResponse::BadRequest().json(json!({
                            "success": false,
                            "error": format!("JSON error: {}", err_msg)
                        }))
                    ).into()
                }))
            .wrap(
                actix_cors::Cors::default()
                    .allow_any_origin()
                    .allow_any_method()
                    .allow_any_header()
                    .max_age(3600)
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
            .service(
                web::resource("/api/venue-feedback")
                    .route(web::post().to(handle_venue_feedback))
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
    .keep_alive(std::time::Duration::from_secs(900))
    .client_request_timeout(std::time::Duration::from_secs(900))
    .run()
    .await
} 