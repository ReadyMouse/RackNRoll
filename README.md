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

## Setting up the Python Portion Environment
Set up a working environmnet based off the requirements.txt. Using conda or miniconda. 

Recommended order: 
```bash
conda create --name pool python=3.10
conda activate pool
conda install pytorch pandas
pip install -r requirements.txt
```

# Using RackNRoll
## Modify the Lat/Lon/Radius 
Change the lat/lon variables to your geographic region and dd a radius in meters. 

``` rust
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let latitude = 42.4883417;
    let longitude = -71.2235583;
    let radius_meters = 100.0; // 10km in meters
    let months_threshold = 6;

    // The rest of the code...
```

## Running
```bash
cargo run
```