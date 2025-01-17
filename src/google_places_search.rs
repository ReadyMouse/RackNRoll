use dotenv::dotenv;
use serde::Deserialize;
use reqwest;

// Struct definitions

#[derive(Debug, Deserialize)]
pub struct Location {
    latitude: f64,
    longitude: f64,
}

#[derive(Debug, Deserialize)]
pub struct DisplayName {
    pub text: String
}

#[derive(Debug, Deserialize)]
pub struct Place {
    pub id: String, // Place ID
    #[serde(rename = "displayName")] // Map JSON field "displayName" to Rust field "display_name"
    pub display_name: DisplayName, // Nested display name object
    pub location: Location, // Nested location object
}

#[derive(Debug, Deserialize)]
pub struct PlacesResponse {
    pub places: Vec<Place>,
}

// Get the Places in the Local Geographic Region
pub async fn search_places(api_key: &String, lat: f64, lon: f64, radius: f64, place_type: &str) -> Result<PlacesResponse, Box<dyn std::error::Error>> {
    dotenv().ok();
    //let api_key = env::var("GOOGLE_PLACES_API_KEY")?;
    
    let url = format!(
        "https://places.googleapis.com/v1/places:searchNearby",
        // lat, lon, radius will go in the request body
    );

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("X-Goog-Api-Key", api_key)
        .header("X-Goog-FieldMask", "places.id,places.displayName,places.location") // Include places.id and places.displayName
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
    // println!("API Response: {}", result); // Print the raw API response for debugging

    let places_response: PlacesResponse = serde_json::from_str(&result)?;
    
    for place in &places_response.places {
        println!("ID: {}", place.id); // Print the place ID
        println!("Name: {}", place.display_name.text); // Print the display name text
        println!("Location: {}, {}", place.location.latitude, place.location.longitude); // Print the location
        println!("----------------");
    }

    Ok(places_response)
}