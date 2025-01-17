use dotenv::dotenv;
use std::env;
use tokio;

mod google_places_search;
use google_places_search::search_places; //just import function call

mod google_places_photos_reviews;
use google_places_photos_reviews::GooglePlacesClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let latitude = 40.7128;
    let longitude = -74.0060;
    let radius_meters = 10000.0; // 10km in meters

    dotenv().ok();
    let api_key = env::var("GOOGLE_PLACES_API_KEY").expect("GOOGLE_PLACES_API_KEY must be set");
    let cred_path = env::var("GOOGLE_PLACES_CRED_PATH").expect("GOOGLE_PLACES_CRED_PATH must be set");

    // println!("API Key: {:?}", env::var("GOOGLE_PLACES_API_KEY"));
    // First get the places
    let places = search_places(&api_key, latitude, longitude, radius_meters, "restaurant").await?;

    // Create the photos client
    let photos_client = GooglePlacesClient::new(
        &cred_path,  
        &api_key,
        "google_photos",
    );

    // For each place, get its photos
    for place in places.places {
        match photos_client.get_place_photos(&place.id).await {
            Ok(photos) => println!("Found {} photos for place {}", photos.len(), place.display_name.text),
            Err(e) => eprintln!("Error getting photos for {}: {}", place.display_name.text, e)
        }
    }

    Ok(())
}