use chrono::{DateTime, Utc, Duration};
use serde::{Serialize, Deserialize};
use std::path::Path;
use std::io::Write;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Venue {    
    pub name: String,
    pub place_id: String,
    pub address: String,
    pub pool_table_probability: f32,
    pub processed_date: DateTime<Utc>,
    #[serde(default)] 
    pub human_approved: i32,
    pub latitude: f64,
    pub longitude: f64,
}

impl Venue {
    pub fn new(name: String, place_id: String, address: String, probability: f32, lat: f64, lon: f64) -> Self {
        Venue {
            name,
            place_id,
            address,
            pool_table_probability: probability,
            processed_date: Utc::now(),
            human_approved: 0,
            latitude: lat,
            longitude: lon,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct VenueCollection {
    pub venues: Vec<Venue>,
    pub last_updated: DateTime<Utc>,
}

impl VenueCollection {
    pub fn new() -> Self {
        VenueCollection {
            venues: Vec::new(),
            last_updated: Utc::now(),
        }
    }

    pub fn add_venue(&mut self, venue: Venue) {
        self.venues.push(venue);
        self.last_updated = Utc::now();
    }

    pub fn save_to_json(&self, file_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(file_path, json)?;
        Ok(())
    }

    pub fn load_from_json(file_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        println!("Attempting to load database from: {}", file_path.display());
        let json_str = std::fs::read_to_string(file_path)?;
        println!("Read {} bytes from database file", json_str.len());
        match serde_json::from_str::<VenueCollection>(&json_str) {
            Ok(collection) => {
                println!("Successfully parsed database with {} venues", collection.venues.len());
                Ok(collection)
            },
            Err(e) => {
                eprintln!("Error parsing database JSON: {}", e);
                Err(e.into())
            }
        }
    }

    pub fn should_process_venue(&self, place_id: &str, months_threshold: i64) -> (bool, f32) {
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

    pub fn save_filtered_venues_csv(&self, file_path: &Path, threshold: f32) -> Result<(), Box<dyn std::error::Error>> {
        let filtered_venues: Vec<_> = self.venues
            .iter()
            .filter(|v| v.pool_table_probability >= threshold)
            .collect();
            
        let mut writer = std::fs::File::create(file_path)?;
        
        // Write CSV header
        writeln!(writer, "Name,Address,Pool Table Probability,Place ID")?;
        
        // Write each venue
        for venue in filtered_venues {
            writeln!(
                writer,
                "{},{},{:.2}%,{}",
                venue.name.replace(",", ""),  // Remove commas from names to avoid CSV issues
                venue.address.replace(",", ""),  // Remove commas from addresses
                venue.pool_table_probability * 100.0,
                venue.place_id
            )?;
        }
        
        Ok(())
    }
} 