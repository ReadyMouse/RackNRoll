# RackNRoll
Finding pool tables in bars and hotels with YOLO image processing, built in Rust.

Automated Finder for Pocket billiards venues. 

Challenges: 
1. Ambiguity in text
2. Limited pixels on object

Finding bars, resturants, and hotels with pool tables is particuarly hard problem because these places often don't list billiards or pool tables as an amentiy on their websites. Open Street Map (OSM) does have a "sport=billiards" tag that can be added to Points of Interest, however without a OSM-enthustist pool player in the area, venues are rarely mapped.  

Text: Sometimes Google reviewers will include a note in their feedback, however there is high ambiguity between "pool tables", "pool", and "table". Eg "I really enjoyed playing in the pool, and got a table right away at the resturant." Rarely will a reviewer use "billiards" for a dive bar, unless it's a pool hall or higher-scale establishment.  

Images: Scanning the photos, pool tables often show as small slivers in the back of full-room shots, making identifying the billiards table difficult. 

-> Text Processing: TODO

-> Image Processing: This repo approaches the problem using an image segmentation model of a pre-trained YOLOv8, fine-tuned on pool tables from OpenImages.  

The developers of PocketFinder would love for users to add the pool tables found to OSM with the "sport=billiards" tag. Query through Overpass Turbo API, or phone apps.  
https://www.openstreetmap.org/
https://overpass-turbo.eu/#

# Note
The YOLO model was developed, and fine-tuned using the [PocketFinder](https://github.com/ReadyMouse/PocketFinder) repo. 

## Prerequisites
- Rust (latest stable version)
- Python 3.10
- Conda or Miniconda
- Google Places API key
- Google Cloud Service Account credentials

## Environment Setup

### 1. Python Environment
```bash
conda create --name pool python=3.10
conda activate pool
conda install pytorch pandas
pip install -r requirements.txt
```

### 2. Environment Variables
Create a `.env` file in the root directory with:
```env
GOOGLE_PLACES_API_KEY=your_api_key_here
GOOGLE_PLACES_CRED_PATH=./path/to/your/service-account.json
YOLO_WEIGHTS_PATH=./yolo_weights.pt
YOLO_CONFIDENCE_THRES=0.5
OUTPUT_DIRECTORY=./google_photos
```

### 3. Required Files
- Place your YOLO model weights at `./yolo_weights.pt`
- Place your Google Cloud service account JSON at the path specified in `.env`

## Configuration
Modify `config.yaml` to set your search parameters:
```yaml
location:
  latitude: your_latitude
  longitude: your_longitude
  radius_meters: search_radius

processing:
  months_threshold: 6
  reprocess_all: false
  save_negative_images: false

place_types:
  - bar
  - hotel
  - restaurant
```

## Running the Application
```bash
cargo run --config your_config.yaml
```
or  for web interface

```bash
cargo run -- --web    
```
## Output
The program generates two main outputs:
1. `venues_database.json` - Contains all processed venues
2. `config_results_pool_tables.csv` - Filtered results of venues with pool tables (>80% confidence)

## Project Structure
- `src/` - Rust source code
- `PoolTableInference.py` - YOLO inference script
- `google_photos/` - Downloaded venue photos (gitignored)

## Contributing
Contributions are welcome! Please feel free to submit a Pull Request.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/AmazingFeature`)
3. Commit your changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request