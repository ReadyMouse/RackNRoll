use dotenv::dotenv;
use std::env;
use tokio;
use std::process::Command;
use std::path::PathBuf;
use std::path::Path;
use std::fs;
use chrono::{DateTime, Utc, Duration};
use serde::{Serialize, Deserialize};
use std::fs::File;
use serde_yaml;
use clap::Parser;

mod google_places_search;
use google_places_search::{search_places, PlacesResponse};  //just import function call

mod google_places_photos_reviews;
use google_places_photos_reviews::GooglePlacesClient;

// TODO: Add Location information to Venue
#[derive(Serialize, Deserialize, Debug)]
struct Venue {    
    name: String,
    place_id: String,
    address: String,
    pool_table_probability: f32,
    processed_date: DateTime<Utc>,
}

#[derive(Serialize, Deserialize, Debug)]
struct VenueCollection {
    venues: Vec<Venue>,
    last_updated: DateTime<Utc>,
}

impl Venue {
    fn new(name: String, place_id: String, address: String, probability: f32) -> Self {
        Venue {
            name,
            place_id,
            address,
            pool_table_probability: probability,
            processed_date: Utc::now(),
        }
    }
}

impl VenueCollection {
    fn new() -> Self {
        VenueCollection {
            venues: Vec::new(),
            last_updated: Utc::now(),
        }
    }

    fn add_venue(&mut self, venue: Venue) {
        self.venues.push(venue);
        self.last_updated = Utc::now();
    }

    fn save_to_json(&self, file_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(file_path, json)?;
        Ok(())
    }

    fn load_from_json(file_path: &std::path::Path) -> Result<Self, Box<dyn std::error::Error>> {
        let json_str = std::fs::read_to_string(file_path)?;
        let collection = serde_json::from_str(&json_str)?;
        Ok(collection)
    }

    fn should_process_venue(&self, place_id: &str, months_threshold: i64) -> (bool, f32) {
        if let Some(existing_venue) = self.venues.iter().find(|v| v.place_id == place_id) {
            let now = Utc::now();
            let duration_since_update = now - existing_venue.processed_date;
            let months = Duration::days(months_threshold * 30); // approximate months to days

            // Hand out the probability
            let prob = existing_venue.pool_table_probability;
            (duration_since_update > months, prob)
        } else {
            (true, 0.0) // Venue doesn't exist, should process
        }
    }
}

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
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parse command line arguments
    let cli = Cli::parse();

    // Replace the hardcoded parameters with config file reading
    let config: Config = {
        let file = File::open(&cli.config)?;
        serde_yaml::from_reader(file)?
    };

    dotenv().ok();
    let api_key = env::var("GOOGLE_PLACES_API_KEY").expect("GOOGLE_PLACES_API_KEY must be set");
    let cred_path = env::var("GOOGLE_PLACES_CRED_PATH").expect("GOOGLE_PLACES_CRED_PATH must be set");
    let output_dir = env::var("OUTPUT_DIRECTORY").expect("OUTPUT_DIRECTORY must be set");

    // Use the config values instead of hardcoded ones
    let mut all_places = PlacesResponse { places: Vec::new() };

    // Load an existing collection of venues, or make a new one.
    // TODO: Make the database file dynamic, and user input. Hate me later. EPK.
    let mut collection = match VenueCollection::load_from_json(Path::new("venues_database.json")) {
        Ok(loaded_collection) => loaded_collection,
        Err(e) => {
            println!("You have no crabs in your crate.");
            println!("Could not load from JSON, creating new collection: {}", e);
            VenueCollection::new()
        }
    };

    // Replace the hardcoded place_types array with config.place_types
    for place_type in &config.place_types {
        match search_places(
            &api_key,
            config.location.latitude,
            config.location.longitude,
            config.location.radius_meters,
            place_type
        ).await {
            Ok(places) => all_places.places.extend(places.places),
            Err(e) => eprintln!("Error searching for {}: {}", place_type, e)
        }
    }

    // Create the photos client
    let photos_client = GooglePlacesClient::new(
        &cred_path,  
        &api_key,
        &output_dir,
    );

    // Get the variables for the YOLO models
    let model_path = env::var("YOLO_WEIGHTS_PATH").expect("YOLO_WEIGHTS_PATH must be set");
    let conf_threshold = env::var("YOLO_CONFIDENCE_THRES").expect("YOLO_CONFIDENCE_THRES must be set");
    let conf_threshold: f32 = conf_threshold
        .parse()
        .expect("YOLO_CONFIDENCE_THRES must be a valid floating-point number");

    // For each place, check if in database, get its photos and run inference
    for place in all_places.places {
        let (should_process, prob) = collection.should_process_venue(
            &place.id,
            config.processing.months_threshold
        );
        if !should_process && !config.processing.reprocess_all {
            println!("From Database:: Probabiliy of pool table: {:.2}% at {}",
                prob * 100.0,
                &place.display_name.text
            ); 
            continue;
        }

        // Get the Photos
        match photos_client.get_place_photos(&place.id).await {
            Ok(photos) => println!("Found {} photos for place {}", &photos.len(), &place.display_name.text),
            Err(e) => {
                println!("The crab pot gets stuck on a anchor on the ocean floor.");
                println!("Error getting photos for {}: {}", &place.display_name.text, e)}
        }

        // Check for Pool table via YOLO inference
        let folder_path = std::path::Path::new(&output_dir).join(&place.display_name.text);
        // println!("Image Directory: {}", &folder_path.display());
        match run_python_script(
            &folder_path, 
            &model_path, 
            &folder_path,
            config.processing.save_negative_images
        ) {
            Ok(probability) => {
                println!("{} probability of pool table: {:.2}%",&place.display_name.text, probability * 100.0);
                let venue = Venue::new(
                    place.display_name.text,
                    place.id,  // Google Place ID
                    place.formatted_address.clone(),  // Use the actual street address
                    probability
                );

                collection.add_venue(venue);
            },
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }

        // Save to JSON file
        collection.save_to_json(Path::new("venues_database.json"))?;

        // Check if the photos directory is empty 
        // Read directory entries
        // First, let's explicitly list all entries to see what's in there
        let entries: Vec<_> = fs::read_dir(&folder_path)?
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name() != ".DS_Store")
            .collect();

        let is_empty = entries.is_empty();
        
        if is_empty {
            fs::remove_dir_all(&folder_path)?;
            // println!("{} deleted successfully", &folder_path.display());
        } else {
            // println!("{} is not empty", &folder_path.display());
        }
        
    }
    Ok(())
}

// Coding Crab Rangoon //
// If the crabs got your pants, perhaps wear shorts. 
// ******************* //