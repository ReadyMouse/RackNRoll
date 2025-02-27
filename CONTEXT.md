## This is a context file for AI Assistants

## Use Case
user: I'm travelling to a new city and want to find bars, resturants, and hotels that have pool tables.

## Project Overview
Find bars, resturants, and hotels in a given region then give a probability of whether they have a pool table or not, via a fine-tuned YOLOv8n model and the google places api.

## Core Functionalities
- Find venues near given lat/lon/radius using Google Maps API
- Get the photos of the venue via New GooglePlaces API
- Determine if the venue has a pool table with fine-tuned YOLOv8n model
- If so, add it to the database
- If not, add it to the database
- Display pool table photos as evidence and allow users to agree or disagree

## Documentation






