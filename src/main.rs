use dotenv::dotenv;
use std::env;
use tokio;
use std::process::Command;
use std::path::{PathBuf, Path};
use serde::{Serialize, Deserialize};
use std::fs::File;
use serde_yaml;
use clap::Parser;
use futures::future::BoxFuture;

mod google_places_search;
use google_places_search::{search_places, PlacesResponse};  //just import function call

mod google_places_photos_reviews;
use google_places_photos_reviews::GooglePlacesClient;

mod web_server;
use web_server::{AppState, start_server};

mod models;
use models::{Venue, VenueCollection};

fn run_python_script(
    file_path: &PathBuf, 
    model_path: &str, 
    output_dir: &PathBuf,
    save_negative: bool,
) -> Result<f32, Box<dyn std::error::Error>> {
    let output = Command::new("python3")
        .arg("PoolTableInference.py")
        .arg("-i")
        .arg(file_path)
        .arg("-m")
        .arg(model_path)
        .arg("-o")
        .arg(output_dir)
        .arg("--save-negative")
        .arg(save_negative.to_string())
        .output()?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        println!("Error running Python script: {}", error);
        return Err(error.into());
    }

    // Parse the output to get probability
    let output_str = String::from_utf8_lossy(&output.stdout);
    for line in output_str.lines() {
        if line.starts_with("VENUE_PROBABILITY:") {
            if let Ok(prob) = line.strip_prefix("VENUE_PROBABILITY:").unwrap().trim().parse::<f32>() {
                return Ok(prob);
            }
        }
    }
    
    // If we didn't find a probability, return 0.0 as default
    Ok(0.0)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    pub location: Location,
    pub processing: Processing,
    pub place_types: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
    pub radius_meters: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Processing {
    pub months_threshold: i64,
    pub reprocess_all: bool,
    pub save_negative_images: bool,
}

#[derive(Parser)]
struct Cli {
    #[arg(short, long, default_value = "config.yaml")]
    config: String,
    
    #[arg(long)]
    web: bool,
}

fn cleanup_empty_directories(output_dir: &str) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::path::Path::new(output_dir);
    if !path.exists() {
        return Ok(());
    }

    for entry in std::fs::read_dir(path)? {
        if let Ok(entry) = entry {
            if entry.path().is_dir() {
                let dir_path = entry.path();
                let jpg_count = std::fs::read_dir(&dir_path)?
                    .filter_map(Result::ok)
                    .filter(|e| {
                        e.path()
                            .extension()
                            .and_then(|ext| ext.to_str())
                            .map(|ext| ext.to_lowercase() == "jpg")
                            .unwrap_or(false)
                    })
                    .count();

                if jpg_count == 0 {
                    println!("Removing empty directory: {}", dir_path.display());
                    std::fs::remove_dir_all(&dir_path)?;
                }
            }
        }
    }
    Ok(())
}

pub async fn search_pool_tables(
    config: Config,
    api_key: &str,
    cred_path: &str,
    output_dir: &str,
    model_path: &str,
    status_callback: Option<impl Fn(&str) -> BoxFuture<'static, ()> + Send + Sync + 'static>,
) -> Result<Vec<Venue>, Box<dyn std::error::Error>> {
    let mut all_places = PlacesResponse { places: Vec::new() };
    
    // Load an existing collection of venues, or make a new one.
    let mut collection = match VenueCollection::load_from_json(Path::new("venues_database.json")) {
        Ok(loaded_collection) => {
            if let Some(callback) = &status_callback {
                callback("Loaded existing venue database").await;
            }
            loaded_collection
        },
        Err(e) => {
            println!("Could not load from JSON, creating new collection: {}", e);
            if let Some(callback) = &status_callback {
                callback("Creating new venue database").await;
            }
            VenueCollection::new()
        }
    };

    // Create the photos client
    let photos_client = GooglePlacesClient::new(
        cred_path,
        api_key,
        output_dir,
    );

    for place_type in &config.place_types {
        match search_places(
            api_key,
            config.location.latitude,
            config.location.longitude,
            config.location.radius_meters,
            place_type
        ).await {
            Ok(places) => all_places.places.extend(places.places),
            Err(e) => eprintln!("Error searching for {}: {}", place_type, e)
        }
    }

    // Process each place
    let mut venues_processed = 0;
    for place in all_places.places {
        if let Some(callback) = &status_callback {
            callback(&format!("Processing {}", place.display_name.text)).await;
        }

        let (should_process, prob) = collection.should_process_venue(
            &place.id,
            config.processing.months_threshold
        );
        
        if !should_process && !config.processing.reprocess_all {
            let status = format!("From Database: Probability of pool table: {:.2}% at {}", 
                prob * 100.0, 
                &place.display_name.text
            );
            if let Some(callback) = &status_callback {
                callback(&status).await;
            }
            continue;
        }

        match photos_client.get_place_photos(&place.id).await {
            Ok(_) => {
                let folder_path = Path::new(output_dir).join(&place.display_name.text);
                // if let Some(callback) = &status_callback {
                //     callback(&format!("Downloaded photos to {}", folder_path.display())).await;
                // }
                
                match run_python_script(
                    &folder_path,
                    model_path,
                    &folder_path,
                    config.processing.save_negative_images,
                ) {
                    Ok(probability) => {
                        let status = format!("Probability of pool table at {}: {:.2}%", 
                            place.display_name.text, probability * 100.0);
                        println!("Status update: {}", status);
                        if let Some(callback) = &status_callback {
                            callback(&status).await;
                        }
                        
                        let venue_name = place.display_name.text.clone();
                        let venue = Venue::new(
                            venue_name.clone(),
                            place.id,
                            place.formatted_address,
                            probability,
                            place.location.latitude,
                            place.location.longitude
                        );
                        collection.add_venue(venue);
                        
                        // Increment processed count and save periodically
                        venues_processed += 1;
                        if venues_processed % 5 == 0 {
                            if let Some(callback) = &status_callback {
                                callback("Saving database checkpoint...").await;
                            }
                            if let Err(e) = collection.save_to_json(Path::new("venues_database.json")) {
                                eprintln!("Error saving venue database checkpoint: {}", e);
                            }
                        }
                    },
                    Err(e) => eprintln!("Error: {}", e)
                }
            },
            Err(e) => eprintln!("Error getting photos for {}: {}", &place.display_name.text, e)
        }
    }

    // After processing all places, cleanup any empty directories
    if let Err(e) = cleanup_empty_directories(output_dir) {
        eprintln!("Error cleaning up empty directories: {}", e);
    }

    // Final save to ensure we don't miss any venues
    collection.save_to_json(Path::new("venues_database.json"))?;
    
    Ok(collection.venues)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    
    dotenv().ok();
    let api_key = env::var("GOOGLE_PLACES_API_KEY").expect("GOOGLE_PLACES_API_KEY must be set");
    let cred_path = env::var("GOOGLE_PLACES_CRED_PATH").expect("GOOGLE_PLACES_CRED_PATH must be set");
    let output_dir = env::var("OUTPUT_DIRECTORY").expect("OUTPUT_DIRECTORY must be set");
    let model_path = env::var("YOLO_WEIGHTS_PATH").expect("YOLO_WEIGHTS_PATH must be set");

    if cli.web {
        println!("Starting web server on http://localhost:3000");
        start_server(AppState {
            api_key,
            cred_path,
            output_dir,
            model_path,
        }).await?;
    } else {
        let config: Config = {
            let file = File::open(&cli.config)?;
            serde_yaml::from_reader(file)?
        };

        let venues = search_pool_tables(
            config,
            &api_key,
            &cred_path,
            &output_dir,
            &model_path,
            Some(|msg: &str| -> BoxFuture<'static, ()> {
                let msg = msg.to_string(); // Clone the message before moving
                Box::pin(async move {
                    println!("{}", msg);
                })
            })
        ).await?;

        // Save filtered results to CSV
        let config_name = Path::new(&cli.config)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("config");
        
        let filtered_filename = format!("{}_results_pool_tables.csv", config_name);
        let collection = VenueCollection { 
            venues, 
            last_updated: chrono::Utc::now() 
        };
        collection.save_filtered_venues_csv(Path::new(&filtered_filename), 0.80)?;
    }

    Ok(())
}

// Coding Crab Rangoon //
// If the crabs got your pants, perhaps wear shorts. 
// ******************* //