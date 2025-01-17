use serde::{Deserialize, Serialize};
use jsonwebtoken::{encode, EncodingKey, Header};
use std::time::{SystemTime, UNIX_EPOCH};
use std::fs;

pub struct GooglePlacesClient {
    cred_json_path: String,
    api_key: String,
    base_url: String,
    output_dir: String,
}

#[derive(Debug, Deserialize)]
struct ServiceAccountCredentials {
    client_email: String,
    private_key: String,
    //project_id: String,
}

#[derive(Debug, Serialize)]
struct JWTClaims {
    iss: String,  // client_email from service account
    aud: String,  // OAuth token endpoint
    exp: u64,     // Expiration time
    iat: u64,     // Issued at time
    scope: String, // Required scopes
}

#[derive(Debug, Deserialize)]
pub struct PhotoDetails {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct PlaceDetails {
    #[serde(default)]
    pub photos: Vec<PhotoDetails>,
    #[serde(rename = "displayName")]
    pub display_name: DisplayName,
}

#[derive(Debug, Deserialize)]
pub struct DisplayName {
    pub text: String,
}

impl GooglePlacesClient {
    pub fn new(cred_json_path: &str, api_key: &str, output_dir: &str) -> Self {
        // Don't panic if directory already exists
        if let Err(e) = std::fs::create_dir_all(output_dir) {
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                // Only panic for errors other than AlreadyExists
                panic!("Failed to create output directory: {}", e);
            }
        }
        
        Self {
            cred_json_path: cred_json_path.to_string(),
            api_key: api_key.to_string(),
            base_url: "https://places.googleapis.com/v1/".to_string(),
            output_dir: output_dir.to_string(),
        }
    }

    async fn get_access_token(&self) -> Result<String, Box<dyn std::error::Error>> {
        // Read and parse service account JSON
        // TODO: Remove this section \/
        // println!("Attempting to read file from: {}", &self.cred_json_path);
    
        let creds_content = match std::fs::read_to_string(&self.cred_json_path) {
            Ok(content) => content,
            Err(e) => {
                println!("Error reading credentials file: {}", e);
                return Err(Box::new(e));
            }
        };
        
        // println!("Successfully read credentials file");
        // TODO: Remove this section /\

        let creds_content = std::fs::read_to_string(&self.cred_json_path)?;
        let creds: ServiceAccountCredentials = serde_json::from_str(&creds_content)?;

        // Create JWT claims
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let claims = JWTClaims {
            iss: creds.client_email,
            aud: "https://oauth2.googleapis.com/token".to_string(),
            exp: now + 3600,
            iat: now,
            scope: "https://www.googleapis.com/auth/maps-platform.places".to_string(),
        };

        // Create JWT
        let header = Header::new(jsonwebtoken::Algorithm::RS256);
        let key = EncodingKey::from_rsa_pem(creds.private_key.as_bytes())?;
        let jwt = encode(&header, &claims, &key)?;

        // Exchange JWT for access token
        let client = reqwest::Client::new();
        let token_response = client
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &jwt),
            ])
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;

        Ok(token_response["access_token"]
            .as_str()
            .ok_or("No access token in response")?
            .to_string())
    }

    pub async fn get_place_details(&self, place_id: &str) -> Result<PlaceDetails, Box<dyn std::error::Error>> {
        let access_token = self.get_access_token().await?;
        let place_url = format!("{}places/{}", self.base_url, place_id);

        let client = reqwest::Client::new();
        let response = client
            .get(&place_url)
            .header("Authorization", format!("Bearer {}", access_token))
            .header("X-Goog-FieldMask", "photos,displayName")
            .send()
            .await?
            .json()
            .await?;

        Ok(response)
    }

    pub async fn download_photo(&self, photo_name: &str, save_name: &str) -> Result<String, Box<dyn std::error::Error>> {
        let access_token = self.get_access_token().await?;
        let photo_url = format!(
            "{}{}/media?key={}&maxHeightPx=4032&maxWidthPx=4032",
            self.base_url, photo_name, self.api_key
        );

        let client = reqwest::Client::new();
        let response = client
            .get(&photo_url)
            .header("Authorization", format!("Bearer {}", access_token))
            .send()
            .await?;

        let file_path = format!("{}/{}", self.output_dir, save_name);
        println!("Attempting to save photo to: {}", file_path);
        let bytes = response.bytes().await?;
        fs::write(&file_path, &bytes)?;

        Ok(file_path)
    }

    pub async fn get_place_photos(&self, place_id: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
        let place_data = self.get_place_details(place_id).await?;
        let mut photo_results = Vec::new();

        // Create a directory for this place
        let place_dir = format!("{}/{}", self.output_dir, place_data.display_name.text);
        std::fs::create_dir_all(&place_dir)?;  // Create the directory if it doesn't exist


        for (i, photo) in place_data.photos.iter().enumerate() {
            let save_name = format!("{}/photo_{}.jpg", place_data.display_name.text, i);
            let full_path = format!("{}/{}", self.output_dir, save_name);
            
            // Check if file already exists
            if std::path::Path::new(&full_path).exists() {
                println!("Photo {} already exists, skipping download", save_name);
                photo_results.push(full_path);
                continue;
            }

            if let Ok(downloaded_path) = self.download_photo(&photo.name, &save_name).await {
                photo_results.push(downloaded_path);
            }
        }

        println!("Download complete! Successfully downloaded {} place photos", photo_results.len());
        Ok(photo_results)
    }
}