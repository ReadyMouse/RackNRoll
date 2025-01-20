use dotenv::dotenv;
use std::env;
use tokio;
use std::process::Command;
use std::path::PathBuf;
use std::path::Path;
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

mod google_places_search;
use google_places_search::{search_places, PlacesResponse};  //just import function call

mod google_places_photos_reviews;
use google_places_photos_reviews::GooglePlacesClient;

// TODO: Add Location information to Venue
#[derive(Serialize, Deserialize, Debug)]
struct Venue {    
    name: String,
    place_id: String,
    pool_table_probability: f32,
    processed_date: DateTime<Utc>,}

    #[derive(Serialize, Deserialize, Debug)]
    struct VenueCollection {
        venues: Vec<Venue>,
        last_updated: DateTime<Utc>,
    }

impl Venue {
    fn new(name: String, place_id: String, probability: f32) -> Self {
        Venue {
            name,
            place_id,
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
}


fn run_python_script(file_path: &PathBuf, model_path: &str, output_dir: &PathBuf) -> Result<f32, Box<dyn std::error::Error>> {
    let output = Command::new("python3")
        .arg("PoolTableInference.py")
        .arg(file_path)
        .arg(model_path)
        .arg("-o")
        .arg(output_dir)
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let latitude = 42.4883417;
    let longitude = -71.2235583;
    let radius_meters = 100.0; // 10km in meters

    dotenv().ok();
    let api_key = env::var("GOOGLE_PLACES_API_KEY").expect("GOOGLE_PLACES_API_KEY must be set");
    let cred_path = env::var("GOOGLE_PLACES_CRED_PATH").expect("GOOGLE_PLACES_CRED_PATH must be set");
    let output_dir = env::var("OUTPUT_DIRECTORY").expect("OUTPUT_DIRECTORY must be set");


    println!("API Key: {:?}", env::var("GOOGLE_PLACES_API_KEY"));
    // First get the places
    let place_types = ["bar", "hotel", "restaurant"];

    // let places = search_places(&api_key, latitude, longitude, radius_meters, "restaurant").await?;
    
    let mut all_places = PlacesResponse { places: Vec::new() };
    let mut collection = VenueCollection::new();

    for place_type in place_types {
        match search_places(&api_key, latitude, longitude, radius_meters, place_type).await {
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

    // For each place, get its photos
    for place in all_places.places {
        match photos_client.get_place_photos(&place.id).await {
            Ok(photos) => println!("Found {} photos for place {}", &photos.len(), &place.display_name.text),
            Err(e) => eprintln!("Error getting photos for {}: {}", &place.display_name.text, e)
        }

        // TODO: Remove later for debugging.
        println!("Image Directory: {}", &output_dir);
        println!("Model Path: {}", &model_path);

        // Check for Pool table via YOLO inference
        let folder_path = std::path::Path::new(&output_dir).join(&place.display_name.text);
        println!("Image Directory: {}", &folder_path.display());
        match run_python_script(&folder_path, &model_path, &folder_path) {
            Ok(probability) => {
                println!("{} probability of pool table: {:.2}%",&place.display_name.text, probability * 100.0);
                // TODO: Add Location information to Venue
                let venue = Venue::new(
                    place.display_name.text,
                    place.id,  // Google Place ID
                    probability
                );

                collection.add_venue(venue); // Add the venue to the growing catelogue.
            },
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }

        // Save to JSON file
        collection.save_to_json(Path::new("venues_database.json"))?;
    }
    Ok(())
}