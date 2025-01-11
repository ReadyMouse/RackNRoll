use dotenv::dotenv;
use std::env;
use serde::Deserialize; 
use reqwest;
use tokio;

// Struct definitions
#[derive(Debug, Deserialize)]
struct Location {
    latitude: f64,
    longitude: f64,
}

#[derive(Debug, Deserialize)]
struct DisplayName {
    text: String,
    language_code: String,
}

#[derive(Debug, Deserialize)]
struct Place {
    location: Location,
    display_name: DisplayName,
}

#[derive(Debug, Deserialize)]
struct PlacesResponse {
    places: Vec<Place>
}

async fn search_places(lat: f64, lon: f64, radius: f64, place_type: &str) -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let api_key = env::var("GOOGLE_PLACES_API_KEY")?;
    
    let url = format!(
        "https://places.googleapis.com/v1/places:searchNearby",
        // lat, lon, radius will go in the request body
    );

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("X-Goog-Api-Key", api_key)
        .header("X-Goog-FieldMask", "places.displayName,places.location")
        .json(&serde_json::json!({
            "locationRestriction": {
                "circle": {
                    "center": {
                        "latitude": lat,
                        "longitude": lon
                    },
                    "radius": radius
                }
            },
            "includedTypes": [place_type]
        }))
        .send()
        .await?;

    let result = response.text().await?;
    println!("{}", result);

    Ok(())
}

#[tokio::main]
async fn main() {
    let latitude = 40.7128;
    let longitude = -74.0060;
    let radius_meters = 10000.0; // 10km in meters

    if let Err(e) = search_places(latitude, longitude, radius_meters, "restaurant").await {
        eprintln!("Error: {}", e);
    }
}