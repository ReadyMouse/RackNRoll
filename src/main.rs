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

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    location: Location,
    processing: Processing,
    place_types: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct Location {
    latitude: f64,
    longitude: f64,
    radius_meters: f64,
}

#[derive(Serialize, Deserialize, Debug)]
struct Processing {
    months_threshold: i64,
    reprocess_all: bool,
    save_negative_images: bool,
}

#[derive(Parser)]
struct Cli {
    #[arg(short, long, default_value = "config.yaml")]
    config: String,
    
    #[arg(long)]
    web: bool,
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
                if let Some(callback) = &status_callback {
                    callback(&format!("Downloaded photos to {}", folder_path.display())).await;
                }
                
                match run_python_script(
                    &folder_path,
                    model_path,
                    &folder_path,
                    config.processing.save_negative_images,
                ) {
                    Ok(probability) => {
                        let status = format!("New inference for {}: {:.2}%", 
                            place.display_name.text, probability * 100.0);
                        println!("Status update: {}", status); // Debug print
                        if let Some(callback) = &status_callback {
                            callback(&status).await;
                        }
                        
                        // Add debug print for remaining photos
                        if probability > 0.0 {
                            if let Ok(entries) = std::fs::read_dir(&folder_path) {
                                let photo_count = entries.filter(|e| e.is_ok()).count();
                                println!("Found {} photos in {} after inference", photo_count, folder_path.display());
                            }
                        }
                        
                        let venue = Venue::new(
                            place.display_name.text,
                            place.id,
                            place.formatted_address,
                            probability
                        );
                        collection.add_venue(venue);
                    },
                    Err(e) => eprintln!("Error: {}", e)
                }
            },
            Err(e) => eprintln!("Error getting photos for {}: {}", &place.display_name.text, e)
        }
    }

    // Save to JSON file
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