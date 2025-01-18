use dotenv::dotenv;
use std::env;
use tokio;

mod google_places_search;
use google_places_search::{search_places, PlacesResponse};  //just import function call

mod google_places_photos_reviews;
use google_places_photos_reviews::GooglePlacesClient;

// mod yolo_pytorch;
// use yolo_pytorch::YOLOModel;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let latitude = 40.7128;
    let longitude = -74.0060;
    let radius_meters = 10000.0; // 10km in meters

    dotenv().ok();
    let api_key = env::var("GOOGLE_PLACES_API_KEY").expect("GOOGLE_PLACES_API_KEY must be set");
    let cred_path = env::var("GOOGLE_PLACES_CRED_PATH").expect("GOOGLE_PLACES_CRED_PATH must be set");
    let output_dir = env::var("OUTPUT_DIRECTORY").expect("OUTPUT_DIRECTORY must be set");


    // println!("API Key: {:?}", env::var("GOOGLE_PLACES_API_KEY"));
    // First get the places
    let place_types = ["bar", "hotel", "restaurant"];

    // let places = search_places(&api_key, latitude, longitude, radius_meters, "restaurant").await?;
    
    let mut all_places = PlacesResponse { places: Vec::new() };
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

    // For each place, get its photos
    for place in all_places.places {
        match photos_client.get_place_photos(&place.id).await {
            Ok(photos) => println!("Found {} photos for place {}", photos.len(), place.display_name.text),
            Err(e) => eprintln!("Error getting photos for {}: {}", place.display_name.text, e)
        }
    }

    // Check the photos for pool tables (boolean)
    let model_path = env::var("YOLO_WEIGHTS_PATH").expect("YOLO_WEIGHTS_PATH must be set");
    let conf_threshold = env::var("YOLO_CONFIDENCE_THRES").expect("YOLO_CONFIDENCE_THRES must be set");
    let conf_threshold: f32 = conf_threshold
        .parse()
        .expect("YOLO_CONFIDENCE_THRES must be a valid floating-point number");

    // TODO: Create the yolo client
    // Initialize the model with your weights
    // use std::path::PathBuf;
    // let path = PathBuf::from(&model_path);
    // let model = YOLOModel::new(&path)?;

    // Example prediction
    // let is_positive = model.predict(std::path::Path::new(r"/Users/mouse/src/RackNRoll/Pier\ 17/photo_1.jpg"))?;
    // println!("Prediction: {}", if is_positive { "Positive" } else { "Negative" });
        

    Ok(())
}